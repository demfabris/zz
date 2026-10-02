use super::*;

#[derive(Default)]
pub(super) struct Inbox {
    pending: AtomicBool,
    releases: Mutex<Vec<(ClientId, bool)>>,
    shutdown: Mutex<Option<bool>>,
    hooks: Mutex<Vec<PendingHookEvent>>,
}

impl Inbox {
    pub(super) fn release(&self, client: ClientId, exec: bool) {
        self.releases.lock().push((client, exec));
        self.pending.store(true, Ordering::Release);
    }

    pub(super) fn shutdown(&self, hooks: bool) {
        let mut pending = self.shutdown.lock();
        *pending = Some(pending.unwrap_or(false) || hooks);
        self.pending.store(true, Ordering::Release);
    }

    pub(super) fn hooks(&self, events: Vec<PendingHookEvent>) {
        self.hooks.lock().extend(events);
        self.pending.store(true, Ordering::Release);
    }

    pub(super) fn take_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire) && self.pending.swap(false, Ordering::AcqRel)
    }

    pub(super) fn turn(&self, shared: &Arc<Shared>) -> Option<bool> {
        for (client, exec) in std::mem::take(&mut *self.releases.lock()) {
            if !exec || !detach_is_inert(&shared.inner.lock(), client) {
                shared.detach(client);
            }
            shared.unregister(client);
        }
        self.shutdown.lock().take()
    }

    pub(super) fn take_hooks(&self) -> Vec<PendingHookEvent> {
        std::mem::take(&mut *self.hooks.lock())
    }
}

pub(super) struct Startup {
    shared: Arc<Shared>,
    execution: CommandQueueExecution,
    files: VecDeque<PathBuf>,
    read: Option<mpsc::Receiver<std::io::Result<Vec<u8>>>>,
    roots: VecDeque<(PathBuf, PreparedConfig)>,
    replay: Option<source_queue::Replay>,
    report: ConfigLoadReport,
    invocations: SourceInvocationAccounting,
    warnings: Vec<DeferredControlConfigWarning>,
    context: ExecutionContext,
    base: Option<PathBuf>,
    explicit: bool,
}

impl Startup {
    pub(super) fn new(
        shared: &Arc<Shared>,
        load_user_config: bool,
        files: Option<&[PathBuf]>,
        base: Option<&Path>,
    ) -> Self {
        let shared = shared.command_item(None);
        {
            let mut item = shared.command_item.as_ref().unwrap().lock();
            item.loop_wait = true;
            item.loop_leaf = true;
        }
        shared.log_initialization_knobs();
        *shared.mux_config_selection.lock() = (load_user_config, files.map(<[PathBuf]>::to_vec));
        let configs = shared.selected_mux_config_files();
        {
            let mut inner = shared.inner.lock();
            inner.config_files = format_config_files(&configs);
            inner.startup_source_client_working_directory = base.map(Path::to_owned);
        }
        let execution = shared.config_child_execution(None);
        Self {
            shared,
            execution,
            files: configs.into(),
            read: None,
            roots: VecDeque::new(),
            replay: None,
            report: ConfigLoadReport::startup(),
            invocations: SourceInvocationAccounting::Startup { used: 0 },
            warnings: Vec::new(),
            context: ExecutionContext::default(),
            base: base.map(Path::to_owned),
            explicit: files.is_some(),
        }
    }

    pub(super) fn turn(&mut self) -> Option<Result<(), DaemonError>> {
        let shared = &self.shared;
        loop {
            if let Some(path) = self.files.front() {
                if let Some(read) = &self.read {
                    let bytes = match read.try_recv() {
                        Ok(bytes) => bytes,
                        Err(mpsc::TryRecvError::Empty) => return None,
                        Err(error) => return Some(Err(DaemonError::Thread(error.to_string()))),
                    };
                    self.read = None;
                    let path = self.files.pop_front().unwrap();
                    match bytes {
                        Ok(bytes) => {
                            if let Some(parsed) = shared.parse_config_input(
                                &path,
                                ConfigInput::startup(&bytes),
                                &mut self.report,
                                SourceFileLoadOptions::default(),
                                true,
                            ) {
                                self.roots.push_back((path, parsed));
                            } else {
                                self.report.note_startup_root_read_error(
                                    &path,
                                    &non_utf8_config_error(),
                                    self.explicit,
                                );
                            }
                        }
                        Err(error) => {
                            self.report
                                .note_startup_root_read_error(&path, &error, self.explicit);
                        }
                    }
                    continue;
                }
                let (reply, read) = mpsc::sync_channel(1);
                if let Err(error) = shared.helpers.submit(helpers::Task::Read {
                    path: path.clone(),
                    reply,
                }) {
                    return Some(Err(error.into()));
                }
                self.read = Some(read);
                return None;
            }
            if let Some(replay) = &mut self.replay {
                if shared
                    .command_item
                    .as_ref()
                    .unwrap()
                    .lock()
                    .pending_wait
                    .as_ref()
                    .is_some_and(|wait| !wait.continuation.ready())
                {
                    return None;
                }
                let result = replay.run(shared, &self.execution)?;
                let mut replay = self.replay.take().unwrap();
                self.context = replay.take_context().unwrap_or_default();
                self.report = replay.report;
                self.invocations = replay.invocations;
                self.warnings = replay.warnings;
                shared.finish_command_queue_execution(&self.execution, None);
                self.execution = shared.config_child_execution(None);
                if let Err(error) = result {
                    return Some(Err(error));
                }
                continue;
            }
            if let Some((path, parsed)) = self.roots.pop_front() {
                self.replay = Some(source_queue::Replay::new(
                    shared,
                    PendingConfigFile {
                        path,
                        context: self.context.clone(),
                        options: SourceFileLoadOptions::default(),
                        stdin: None,
                    },
                    parsed,
                    std::mem::replace(&mut self.report, ConfigLoadReport::startup()),
                    std::mem::take(&mut self.invocations),
                    std::mem::take(&mut self.warnings),
                    ClientTerminal::NoClient,
                    self.base.clone(),
                    &self.execution,
                ));
                continue;
            }
            shared.publish_deferred_control_config_warnings(std::mem::take(&mut self.warnings));
            shared.inner.lock().startup_source_client_working_directory = None;
            *shared.startup_config_causes.lock() = self.report.take_startup_causes();
            shared.finish_initialization();
            return Some(Ok(()));
        }
    }
}

#[cfg(test)]
pub(super) fn wait_for_cleanup(shared: &Shared) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !shared.shutdown_cleanup_complete.load(Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "owner-loop cleanup did not complete"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
