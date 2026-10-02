use super::*;

pub(super) struct PaneActor {
    control_rx: Receiver<Command>,
    input_rx: InputReceiver,
    slot: Arc<Mutex<ControlSlot>>,
    publisher: Publisher,
    max_scrollback: usize,
    geometry: Geometry,
    #[cfg(unix)]
    shell_process_id: Option<u32>,
    #[cfg(unix)]
    writer: PtyWriter,
    #[cfg(not(unix))]
    writer: Box<dyn Write + Send>,
    #[cfg(unix)]
    killer: UnixChildKiller,
    #[cfg(not(unix))]
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    #[cfg(unix)]
    master: Arc<unix_pty::UnixMaster>,
    #[cfg(not(unix))]
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    #[cfg(all(unix, not(target_os = "linux")))]
    child_watch: ChildExitWatch,
    #[cfg(unix)]
    wake_rx: Option<std::os::fd::OwnedFd>,
    #[cfg(unix)]
    drain_fd: Option<filedescriptor::FileDescriptor>,
    #[cfg(unix)]
    read_buffer: Vec<u8>,
    #[cfg(unix)]
    bridge_spins: u32,
    #[cfg(any(target_os = "linux", not(unix)))]
    exit_rx: Receiver<std::io::Result<ExitStatus>>,
    #[cfg(not(unix))]
    no_exit: Receiver<std::io::Result<ExitStatus>>,
    #[cfg(any(target_os = "linux", not(unix)))]
    output_rx: Receiver<ReaderMessage>,
    #[cfg(not(unix))]
    no_output: Receiver<ReaderMessage>,
    #[cfg(any(target_os = "linux", not(unix)))]
    recycle_tx: Sender<Vec<u8>>,
    #[cfg(windows)]
    master_close_tx: Sender<Box<dyn portable_pty::MasterPty + Send>>,
    effects: Rc<RefCell<PtyEffects>>,
    reported_size: Rc<Cell<SizeReportSize>>,
    terminal: Terminal<'static, 'static>,
    reported_color_scheme: Rc<Cell<ColorScheme>>,
    frames: Frames<'static>,
    compression: IdleCompression,
    captures: VecDeque<CaptureWork>,
    echo: EchoWindow,
    key_encoder: key::Encoder<'static>,
    key_event: key::Event<'static>,
    mouse_encoder: mouse::Encoder<'static>,
    mouse_event: mouse::Event<'static>,
    input_bytes: Vec<u8>,
    word_separators: WordSeparators,
    wrap_search: bool,
    passthrough: PassthroughFilter,
    engine_knobs: EngineKnobs,
    pending_copy_source: Option<Box<CapturedCopySource>>,
    pane_search: Option<CopyModeSearch>,
    engine_filter: EngineFilter,
    engine_renames: Vec<String>,
    engine_bar: Option<ProgressBar>,
    engine_last_command_status: Option<CommandStatusUpdate>,
    active_views: ActiveTerminalViews,
    inactive_views: InactiveTerminalViews,
    pasted_image_bindings: PastedImageBindings,
    reader_eof: bool,
    exit_status: Option<ExitStatus>,
    terminating: bool,
    termination_deadline: Option<Instant>,
    termination_escalated: bool,
    search_worker: SearchWorker,
    search_results: Receiver<SearchResults>,
    search_refresh_due: Option<Instant>,
    last_content_publish: Instant,
    output_pending: bool,
    vt_diagnostics: VtWriteDiagnostics,
    raw_output_tap: Option<(u64, RawOutputTapSender)>,
    raw_output_parse_backlog: VecDeque<(Arc<[u8]>, usize)>,
    raw_output_parse_backlog_bytes: usize,
    raw_output_parse_buffer: Vec<u8>,
    #[cfg(unix)]
    active_input_permit: Option<InputPermit>,
    published_facts: Option<TerminalFacts>,
    publish_interval: Duration,
    sharded: bool,
    commands_closed: bool,
    #[cfg(target_os = "linux")]
    linux_child: Option<LinuxChildWatch>,
}

impl PaneActor {
    pub(super) fn spawn(
        control_rx: Receiver<Command>,
        input_rx: InputReceiver,
        slot: Arc<Mutex<ControlSlot>>,
        publisher: Publisher,
        max_scrollback: usize,
        appearance: &TerminalAppearance,
        spawn: &TerminalSpawn,
        wake: &ActorWake,
        wake_rx: WakeReceiver,
        sharded: bool,
    ) -> Result<Self, WorkerError> {
        install_kitty_png_decoder();
        #[cfg(not(unix))]
        let () = wake_rx;
        let geometry = spawn
            .initial_size
            .map(Geometry::from_size)
            .unwrap_or_default();
        #[cfg(not(unix))]
        let pair = native_pty_system()
            .openpty(geometry.pty_size())
            .map_err(|error| WorkerError::Pty(error.to_string()))?;
        #[cfg(unix)]
        let pty = unix_pty::open(geometry.pty_size())
            .map_err(|error| WorkerError::Pty(error.to_string()))?;
        #[cfg(unix)]
        let tty = Some(pty.tty.clone());
        #[cfg(not(unix))]
        let tty = None;

        #[cfg(unix)]
        force_pty_erase(pty.master.as_raw_fd(), spawn.knobs.verase_byte);

        let mut command = terminal_command(spawn);
        command.env(
            "TERM",
            spawn.terminal_type.as_deref().unwrap_or("tmux-256color"),
        );
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "zz");
        command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        for (key, value) in &spawn.env {
            if let Some(value) = value {
                command.env(key, value);
            } else {
                command.env_remove(key);
            }
        }
        if let Some(shell) = &spawn.shell {
            command.env("SHELL", shell);
        }
        if let Some(working_directory) = &spawn.working_directory {
            command.cwd(working_directory);
        }

        #[cfg(not(unix))]
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| WorkerError::Spawn(error.to_string()))?;
        #[cfg(not(unix))]
        drop(pair.slave);
        #[cfg(not(unix))]
        let shell_process_id = child.process_id();
        #[cfg(not(unix))]
        let killer = child.clone_killer();
        #[cfg(unix)]
        let (shell_process_id, spawned) = {
            let environment = unix_pty::command_environment(
                &command,
                spawn
                    .env
                    .iter()
                    .map(|(key, _)| key.as_os_str())
                    .chain(PANE_ENVIRONMENT_KEYS.iter().map(std::ffi::OsStr::new))
                    .chain(
                        crate::shell_integration::ENVIRONMENT_KEYS
                            .iter()
                            .map(std::ffi::OsStr::new),
                    ),
            );
            let spawned = unix_pty::spawn(&command, environment, &pty.slave)
                .map_err(|error| WorkerError::Spawn(error.to_string()))?;
            (Some(spawned.pid), spawned)
        };
        #[cfg(unix)]
        drop(pty.slave);
        #[cfg(unix)]
        let killer = UnixChildKiller(shell_process_id);
        #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
        let child_watch = ChildExitWatch::new(shell_process_id)?;
        #[cfg(target_os = "macos")]
        let child_watch = if sharded {
            ChildExitWatch::for_shard(shell_process_id)?
        } else {
            ChildExitWatch::new(shell_process_id)?
        };
        #[cfg(any(target_os = "linux", not(unix)))]
        let (exit_tx, exit_rx) = crossbeam_channel::bounded(1);
        #[cfg(target_os = "linux")]
        let mut linux_child = watch_child_linux(shell_process_id, exit_tx, wake.clone())?;
        #[cfg(windows)]
        let (master_close_tx, master_close_rx) = crossbeam_channel::bounded(1);
        #[cfg(not(unix))]
        {
            let mut child = child;
            let exit_wake = wake.clone();
            thread::Builder::new()
                .name("zz-child-wait".into())
                .spawn(move || {
                    let status = child.wait();
                    let _ = exit_tx.send(status);
                    exit_wake.notify();
                    #[cfg(windows)]
                    if let Ok(master) = master_close_rx.recv() {
                        drop(master);
                    }
                })
                .map_err(WorkerError::Io)?;
        }

        #[cfg(unix)]
        let wake_rx = wake_rx.transpose().map_err(|error| {
            WorkerError::Pty(format!("failed to configure terminal wake pipe: {error}"))
        })?;
        #[cfg(unix)]
        let (drain_fd, writer) = {
            let dup = || {
                filedescriptor::FileDescriptor::dup(&pty.master.as_raw_fd())
                    .map_err(|_| WorkerError::Pty("failed to duplicate the PTY master".to_owned()))
            };
            let drain_fd = dup()?;
            let writer_fd = dup()?;
            let _ = rustix::io::fcntl_setfd(&drain_fd, rustix::io::FdFlags::CLOEXEC);
            let _ = rustix::io::fcntl_setfd(&writer_fd, rustix::io::FdFlags::CLOEXEC);
            rustix::io::ioctl_fionbio(&drain_fd, true).map_err(|errno| {
                WorkerError::Pty(format!(
                    "failed to make the PTY master nonblocking: {errno}"
                ))
            })?;
            let writer = PtyWriter::new(writer_fd);
            (drain_fd, writer)
        };
        #[cfg(not(unix))]
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| WorkerError::Pty(error.to_string()))?;
        #[cfg(not(unix))]
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| WorkerError::Pty(error.to_string()))?;
        #[cfg(not(unix))]
        let master = Some(pair.master);
        #[cfg(unix)]
        let master = Arc::new(pty.master);
        publisher.set_foreground_source(Some(Box::new(ForegroundSource {
            #[cfg(unix)]
            master: Arc::clone(&master),
            shell: shell_process_id,
            tty,
        })));

        #[cfg(target_os = "linux")]
        let mut drain_fd = Some(drain_fd);
        #[cfg(all(unix, not(target_os = "linux")))]
        let drain_fd = Some(drain_fd);
        #[cfg(any(target_os = "linux", not(unix)))]
        let (output_rx, recycle_tx) = {
            #[cfg(target_os = "linux")]
            let gather = std::env::var_os("ZZ_PTY_GATHER").is_none_or(|value| value != "0");
            #[cfg(not(unix))]
            let gather = true;
            if gather {
                let (output_tx, output_rx) = crossbeam_channel::bounded(PTY_BUFFER_POOL_SIZE);
                let (recycle_tx, recycle_rx) = crossbeam_channel::bounded(PTY_BUFFER_POOL_SIZE);
                for _ in 0..PTY_BUFFER_POOL_SIZE {
                    recycle_tx
                        .send(vec![0_u8; PTY_READ_BUFFER_BYTES])
                        .map_err(|error| WorkerError::Thread(error.to_string()))?;
                }
                #[cfg(target_os = "linux")]
                thread::Builder::new()
                    .name("zz-pty-gather".into())
                    .spawn({
                        let gather_child = if sharded { None } else { linux_child.take() };
                        let drain_fd = drain_fd.take().expect("gather owns the PTY master");
                        let output_wake = wake.clone();
                        move || {
                            gather_pty_linux(
                                drain_fd,
                                output_tx,
                                recycle_rx,
                                gather_child,
                                output_wake,
                            );
                        }
                    })
                    .map_err(WorkerError::Io)?;
                #[cfg(not(unix))]
                {
                    let pending_output: Box<dyn Fn() -> usize + Send> = Box::new(|| 0);
                    let output_wake = wake.clone();
                    thread::Builder::new()
                        .name("zz-pty-reader".into())
                        .spawn(move || {
                            read_pty(reader, pending_output, output_tx, recycle_rx, output_wake);
                        })
                        .map_err(WorkerError::Io)?;
                }
                (output_rx, recycle_tx)
            } else {
                let (recycle_tx, _) = crossbeam_channel::unbounded();
                (crossbeam_channel::never(), recycle_tx)
            }
        };

        let effects = Rc::new(RefCell::new(PtyEffects::new()));
        let effect_sink = Rc::clone(&effects);
        let reported_size = Rc::new(Cell::new(geometry.size_report()));
        let size_source = Rc::clone(&reported_size);

        let mut terminal = new_terminal(geometry.columns, geometry.rows, max_scrollback)?;
        terminal.resize(
            geometry.columns,
            geometry.rows,
            geometry.cell_width_px,
            geometry.cell_height_px,
        )?;
        terminal.on_pty_write(move |_, bytes| {
            effect_sink.borrow_mut().push(bytes);
        })?;
        configure_kitty_storage(&mut terminal)?;
        register_device_attributes(&mut terminal)?;
        terminal.on_size(move |_| Some(size_source.get()))?;
        let reported_color_scheme =
            Rc::new(Cell::new(ghostty_color_scheme(appearance.color_scheme)));
        let color_scheme_source = Rc::clone(&reported_color_scheme);
        terminal.on_color_scheme(move |_| Some(color_scheme_source.get()))?;
        terminal.on_xtversion(|_| Some(concat!("zz ", env!("CARGO_PKG_VERSION"))))?;
        register_clipboard_write(&mut terminal, publisher.clone())?;
        register_bell(&mut terminal, publisher.clone())?;
        apply_terminal_appearance(&mut terminal, appearance)?;

        let mut frames = Frames::new(appearance)?;
        let compression = IdleCompression::default();
        let echo = EchoWindow::default();
        let key_encoder = key::Encoder::new()?;
        let key_event = key::Event::new()?;
        let mouse_encoder = mouse::Encoder::new()?;
        let mouse_event = mouse::Event::new()?;
        let input_bytes = Vec::with_capacity(LINK_URI_SCRATCH_BYTES);
        let word_separators = spawn.word_separators.clone().unwrap_or_default();
        let wrap_search = spawn.wrap_search.unwrap_or(true);
        let mut passthrough = PassthroughFilter::default();
        if spawn.allow_passthrough == Some(true) {
            passthrough.set_mode(AllowPassthrough::All);
        }
        let engine_knobs = spawn.knobs;
        let pending_copy_source: Option<Box<CapturedCopySource>> = None;
        let pane_search: Option<CopyModeSearch> = None;
        let engine_filter = EngineFilter::default();
        let engine_renames = Vec::new();
        let engine_bar: Option<ProgressBar> = None;
        let engine_last_command_status: Option<CommandStatusUpdate> = None;
        let mut active_views = ActiveTerminalViews::new();
        let inactive_views = InactiveTerminalViews::new();
        let pasted_image_bindings = PastedImageBindings::default();
        let reader_eof = false;
        let exit_status = None;
        let terminating = false;
        let termination_deadline = None::<Instant>;
        let termination_escalated = false;
        #[cfg(not(unix))]
        let no_exit = crossbeam_channel::never();
        #[cfg(not(unix))]
        let no_output = crossbeam_channel::never();
        #[cfg(unix)]
        let read_buffer = if drain_fd.is_some() {
            vec![0_u8; PTY_READ_BUFFER_BYTES]
        } else {
            Vec::new()
        };
        #[cfg(unix)]
        let bridge_spins = PTY_BRIDGE_SPIN_MAX;
        let (search_worker, search_results) = SearchWorker::spawn(wake.clone());
        let search_refresh_due = None::<Instant>;
        let last_content_publish = Instant::now();
        let output_pending = false;
        let vt_diagnostics = VtWriteDiagnostics::default();
        let raw_output_tap = None;
        let raw_output_parse_backlog = VecDeque::<(Arc<[u8]>, usize)>::new();
        let raw_output_parse_backlog_bytes = 0_usize;
        let raw_output_parse_buffer = Vec::new();
        #[cfg(unix)]
        let active_input_permit = None::<InputPermit>;

        #[cfg(unix)]
        spawned.wait_for_exec(PANE_EXEC_WAIT);
        publish_active_views(
            &mut terminal,
            &publisher,
            &mut frames,
            SnapshotChange::Content,
            &mut active_views,
            &word_separators,
            SessionStatus::Running,
        )?;

        Ok(Self {
            control_rx,
            input_rx,
            slot,
            publisher,
            max_scrollback,
            geometry,
            #[cfg(unix)]
            shell_process_id,
            #[cfg(unix)]
            writer,
            #[cfg(not(unix))]
            writer,
            #[cfg(unix)]
            killer,
            #[cfg(not(unix))]
            killer,
            #[cfg(unix)]
            master,
            #[cfg(not(unix))]
            master,
            #[cfg(all(unix, not(target_os = "linux")))]
            child_watch,
            #[cfg(unix)]
            wake_rx,
            #[cfg(unix)]
            drain_fd,
            #[cfg(unix)]
            read_buffer,
            #[cfg(unix)]
            bridge_spins,
            #[cfg(any(target_os = "linux", not(unix)))]
            exit_rx,
            #[cfg(not(unix))]
            no_exit,
            #[cfg(any(target_os = "linux", not(unix)))]
            output_rx,
            #[cfg(not(unix))]
            no_output,
            #[cfg(any(target_os = "linux", not(unix)))]
            recycle_tx,
            #[cfg(windows)]
            master_close_tx,
            effects,
            reported_size,
            terminal,
            reported_color_scheme,
            frames,
            compression,
            captures: VecDeque::new(),
            echo,
            key_encoder,
            key_event,
            mouse_encoder,
            mouse_event,
            input_bytes,
            word_separators,
            wrap_search,
            passthrough,
            engine_knobs,
            pending_copy_source,
            pane_search,
            engine_filter,
            engine_renames,
            engine_bar,
            engine_last_command_status,
            active_views,
            inactive_views,
            pasted_image_bindings,
            reader_eof,
            exit_status,
            terminating,
            termination_deadline,
            termination_escalated,
            search_worker,
            search_results,
            search_refresh_due,
            last_content_publish,
            output_pending,
            vt_diagnostics,
            raw_output_tap,
            raw_output_parse_backlog,
            raw_output_parse_backlog_bytes,
            raw_output_parse_buffer,
            #[cfg(unix)]
            active_input_permit,
            published_facts: None,
            publish_interval: CONTENT_PUBLISH_STALENESS,
            sharded,
            commands_closed: false,
            #[cfg(target_os = "linux")]
            linux_child,
        })
    }

    pub(super) fn on_deadline(&mut self) -> Result<bool, WorkerError> {
        step_capture_work(&mut self.captures);
        let facts = self.engine_filter.facts(&self.terminal)?;
        if self.published_facts != Some(facts) {
            self.publisher.set_facts(facts);
            self.published_facts = Some(facts);
        }
        #[cfg(unix)]
        {
            self.writer.flush_pending()?;
            if !self.writer.has_pending() {
                self.active_input_permit.take();
            }
            drain_effects_if_writer_ready(&self.effects, &mut self.writer)?;
        }
        let now = Instant::now();
        if self
            .termination_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            if self.termination_escalated {
                return Ok(false);
            }
            #[cfg(unix)]
            signal_terminal_process_groups(
                &self.master,
                self.shell_process_id,
                rustix::process::Signal::KILL,
            );
            #[cfg(not(unix))]
            let _ = self.killer.kill();
            self.termination_escalated = true;
            self.termination_deadline = Some(now + TERMINATION_KILL_WAIT);
        }
        if self.search_refresh_due.is_some_and(|due| now >= due) {
            self.search_refresh_due = None;
            self.compression.rearm();
            let mut view_ids = self.active_views.keys().copied().collect::<Vec<_>>();
            view_ids.sort_by_key(|view| view.0);
            for view_id in view_ids {
                let view = self
                    .active_views
                    .get_mut(&view_id)
                    .expect("active search view was collected from the same map");
                if view.search.is_some() && view.copy_mode.is_none() {
                    let _ = refresh_view_search(
                        &self.terminal,
                        view_id,
                        view,
                        &mut self.search_worker,
                    )?;
                }
            }
        }
        let pending_window_due = self
            .pasted_image_bindings
            .next_deadline()
            .is_some_and(|deadline| deadline <= now);
        if let Some(bar) = self.engine_bar.take() {
            self.publisher.set_progress_bar(bar);
        }
        if let Some(status) = self.engine_last_command_status.take() {
            self.publisher.set_last_command_status(status.code());
        }
        let synchronized_output_deadline = self.frames.synchronized_output_deadline;
        let synchronized_output_due =
            synchronized_output_deadline.is_some_and(|deadline| now >= deadline);
        if synchronized_output_due || (self.reader_eof && synchronized_output_deadline.is_some()) {
            self.output_pending = true;
        }
        if self.reader_eof {
            self.terminal.set_mode(Mode::SYNC_OUTPUT, false)?;
        }
        let publish_interval = if self.frames.unwatched(&self.active_views) {
            UNWATCHED_NOTIFY_INTERVAL
        } else {
            CONTENT_PUBLISH_STALENESS
        };
        self.publish_interval = publish_interval;
        let echo_due = self.output_pending && self.echo.due();
        if self.output_pending
            && (self.reader_eof
                || synchronized_output_due
                || pending_window_due
                || echo_due
                || self.engine_filter.metadata_hint
                || self.last_content_publish.elapsed() >= publish_interval)
        {
            if echo_due {
                self.echo.spend();
            }
            self.engine_filter.metadata_hint = false;
            #[cfg(unix)]
            drain_effects_if_writer_ready(&self.effects, &mut self.writer)?;
            #[cfg(not(unix))]
            drain_effects(&self.effects, &mut self.writer)?;
            if let Some((token, number)) = self.pasted_image_bindings.observe(&self.terminal)? {
                self.publisher.placeholder_bound(token, number)?;
            }
            let output_screen = self.terminal.active_screen()?;
            for view in self.inactive_views.values_mut() {
                view.note_output(output_screen);
            }
            let mut refresh_search = false;
            for (view_id, view) in &mut self.active_views {
                note_output_and_revalidate_image_hover(
                    &mut self.terminal,
                    view,
                    output_screen,
                    &self.word_separators,
                    self.pasted_image_bindings.bound_numbers(),
                )?;
                if view.search.is_some() && view.copy_mode.is_none() {
                    self.search_worker.cancel(*view_id);
                    refresh_search = true;
                }
            }
            if refresh_search {
                self.search_refresh_due
                    .get_or_insert_with(|| Instant::now() + SEARCH_REFRESH_DEBOUNCE);
            } else {
                self.search_refresh_due = None;
            }
            self.publisher.mark_output_activity();
            publish_active_views(
                &mut self.terminal,
                &self.publisher,
                &mut self.frames,
                SnapshotChange::Content,
                &mut self.active_views,
                &self.word_separators,
                SessionStatus::Running,
            )?;
            self.last_content_publish = Instant::now();
            self.output_pending = false;
            self.vt_diagnostics.emit();
        }
        for token in self.pasted_image_bindings.expire(Instant::now()) {
            self.publisher.pending_paste_expired(token)?;
        }
        for name in self.engine_renames.drain(..) {
            self.publisher.rename_window(name)?;
        }
        if !self.output_pending {
            settle_unwatched(
                &mut self.terminal,
                &self.publisher,
                &mut self.frames,
                &mut self.active_views,
                &self.word_separators,
                SessionStatus::Running,
            )?;
        }
        self.compression.observe(&self.terminal, now);
        if !self.output_pending && self.raw_output_parse_backlog.is_empty() {
            self.compression.run(&mut self.terminal);
        }

        Ok(true)
    }

    pub(super) fn next_deadline(&self) -> Instant {
        let mut deadline = Instant::now() + IDLE_SLEEP;
        if !self.captures.is_empty() {
            deadline = Instant::now();
        }
        if !self.output_pending {
            if let Some(due) = self.frames.settle_due() {
                deadline = deadline.min(due);
            }
            if let Some(due) = self.compression.due() {
                deadline = deadline.min(due);
            }
        }
        if let Some(due) = self.frames.synchronized_output_deadline {
            deadline = deadline.min(due);
        }
        if self.output_pending {
            deadline = deadline.min(if self.echo.due() || self.engine_filter.metadata_hint {
                Instant::now()
            } else {
                self.last_content_publish + self.publish_interval
            });
        }
        if let Some(due) = self.search_refresh_due {
            deadline = deadline.min(due);
        }
        if let Some(due) = self.pasted_image_bindings.next_deadline() {
            deadline = deadline.min(due);
        }
        if let Some(due) = self.termination_deadline {
            deadline = deadline.min(due);
        }
        #[cfg(target_os = "macos")]
        if let Some(due) = self.child_watch.retry {
            deadline = deadline.min(due);
        }
        if !self.raw_output_parse_backlog.is_empty() {
            deadline = Instant::now();
        }
        #[cfg(unix)]
        if self.writer.has_pending() {
            deadline = deadline.min(Instant::now() + PTY_WRITE_RETRY);
        }

        deadline
    }

    pub(super) fn wait_for_wake(&mut self) -> Result<Wake, WorkerError> {
        let timeout = self
            .next_deadline()
            .saturating_duration_since(Instant::now());
        #[cfg(not(unix))]
        let child_exit = if self.exit_status.is_some() {
            &self.no_exit
        } else {
            &self.exit_rx
        };
        #[cfg(all(unix, not(target_os = "linux")))]
        let child_exit = self.exit_status.is_none().then_some(&mut self.child_watch);
        #[cfg(all(unix, not(target_os = "linux")))]
        let available_input = (!self.writer.has_pending()).then_some(&self.input_rx.commands);
        #[cfg(not(unix))]
        let available_input = Some(&self.input_rx.commands);
        #[cfg(not(target_os = "linux"))]
        let raw_output_read_ahead = !self.reader_eof
            && self
                .raw_output_tap
                .as_ref()
                .is_none_or(|(_, tap)| !tap.is_full())
            && self.raw_output_parse_backlog_bytes
                <= RAW_OUTPUT_PARSE_BACKLOG_BYTES
                    .saturating_sub(RAW_OUTPUT_PARSE_READ_RESERVE_BYTES);
        #[cfg(all(unix, not(target_os = "linux")))]
        let wakeup = wait_for_wake(
            &self.control_rx,
            available_input,
            &self.search_results,
            child_exit,
            self.drain_fd
                .as_ref()
                .filter(|_| !self.reader_eof && raw_output_read_ahead),
            self.wake_rx
                .as_ref()
                .expect("per-pane worker has a wake pipe"),
            timeout,
        )?;
        #[cfg(target_os = "linux")]
        let wakeup = self.wait_for_wake_linux(timeout)?;
        #[cfg(not(unix))]
        let available_output = if self.reader_eof || !raw_output_read_ahead {
            &self.no_output
        } else {
            &self.output_rx
        };
        #[cfg(not(unix))]
        let wakeup = wait_for_wake(
            &self.control_rx,
            available_input,
            &self.search_results,
            child_exit,
            available_output,
            timeout,
        )?;

        Ok(wakeup)
    }

    #[cfg(target_os = "linux")]
    fn wait_for_wake_linux(&mut self, timeout: Duration) -> Result<Wake, WorkerError> {
        use rustix::event::{PollFd, PollFlags};
        if let Some(wake) = self.try_wake()? {
            return Ok(wake);
        }
        let (pty_ready, child_ready) = {
            let (pty, child) = self.poll_sources();
            let mut fds = SmallVec::<[PollFd<'_>; 3]>::new();
            fds.push(PollFd::new(
                self.wake_rx
                    .as_ref()
                    .expect("per-pane worker has a wake pipe"),
                PollFlags::IN,
            ));
            let pty_index = pty.map(|fd| {
                fds.push(PollFd::new(fd, PollFlags::IN));
                fds.len() - 1
            });
            let child_index = child.map(|fd| {
                fds.push(PollFd::new(fd, PollFlags::IN));
                fds.len() - 1
            });
            let timespec = rustix::event::Timespec::try_from(timeout.min(IDLE_SLEEP))
                .expect("bounded poll timeout");
            match rustix::event::poll(&mut fds, Some(&timespec)) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => return Ok(Wake::Deadline),
                Err(error) => return Err(WorkerError::Io(error.into())),
            }
            if !fds[0].revents().is_empty() {
                drain_wake_pipe(self.wake_rx.as_ref().expect("per-pane wake pipe"))?;
            }
            let ready =
                |index: Option<usize>| index.is_some_and(|index| !fds[index].revents().is_empty());
            (ready(pty_index), ready(child_index))
        };
        if child_ready {
            self.poll_child()?;
        }
        if let Some(wake) = self.try_wake()? {
            return Ok(wake);
        }
        Ok(if pty_ready {
            Wake::PtyReadable
        } else {
            Wake::Deadline
        })
    }

    pub(super) fn try_wake(&mut self) -> Result<Option<Wake>, WorkerError> {
        if !self.commands_closed {
            match self.control_rx.try_recv() {
                Ok(command) => return Ok(Some(Wake::Command(command))),
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    return Ok(Some(Wake::CommandsClosed));
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
        }
        #[cfg(unix)]
        let input_ready = !self.writer.has_pending();
        #[cfg(not(unix))]
        let input_ready = true;
        if input_ready && let Ok(input) = self.input_rx.commands.try_recv() {
            return Ok(Some(Wake::Input(input)));
        }
        match self.search_results.try_recv() {
            Ok(result) => return Ok(Some(Wake::Search(result))),
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                return Err(WorkerError::Thread(
                    "terminal search worker stopped".to_owned(),
                ));
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
        }
        #[cfg(all(unix, not(target_os = "linux")))]
        if self.exit_status.is_none()
            && let Some(status) = self.child_watch.take_ready()
        {
            return Ok(Some(Wake::ChildExit(status)));
        }
        #[cfg(any(target_os = "linux", not(unix)))]
        {
            if self.exit_status.is_none()
                && let Ok(status) = self.exit_rx.try_recv()
            {
                return Ok(Some(Wake::ChildExit(status)));
            }
            #[cfg(target_os = "linux")]
            let reader_thread = self.drain_fd.is_none();
            #[cfg(not(unix))]
            let reader_thread = true;
            if reader_thread && self.output_read_ahead() {
                match self.output_rx.try_recv() {
                    Ok(message) => return Ok(Some(Wake::PtyMessage(message))),
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        return Ok(Some(Wake::PtyMessage(ReaderMessage::Eof)));
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => {}
                }
            }
        }
        Ok(None)
    }

    pub(super) fn output_read_ahead(&self) -> bool {
        !self.reader_eof
            && self
                .raw_output_tap
                .as_ref()
                .is_none_or(|(_, tap)| !tap.is_full())
            && self.raw_output_parse_backlog_bytes
                <= RAW_OUTPUT_PARSE_BACKLOG_BYTES
                    .saturating_sub(RAW_OUTPUT_PARSE_READ_RESERVE_BYTES)
    }

    fn shutdown(&mut self) {
        self.commands_closed = true;
        let _ = self.killer.kill();
        if !self.terminating {
            self.terminating = true;
            self.termination_deadline = Some(Instant::now() + TERMINATION_KILL_WAIT);
        }
    }

    #[cfg(unix)]
    pub(super) fn poll_sources(
        &self,
    ) -> (
        Option<&filedescriptor::FileDescriptor>,
        Option<&std::os::fd::OwnedFd>,
    ) {
        #[cfg(target_os = "linux")]
        let child = self.linux_child.as_ref().map(|watch| &watch.pidfd);
        #[cfg(not(target_os = "linux"))]
        let child = self
            .exit_status
            .is_none()
            .then_some(self.child_watch.poll_fd())
            .flatten();
        (
            self.drain_fd.as_ref().filter(|_| self.output_read_ahead()),
            child,
        )
    }

    #[cfg(unix)]
    pub(super) fn poll_child(&mut self) -> Result<(), WorkerError> {
        #[cfg(target_os = "linux")]
        if self.linux_child.as_ref().is_some_and(LinuxChildWatch::reap) {
            self.linux_child = None;
            if let Ok(status) = self.exit_rx.try_recv() {
                self.on_child_exit(status)?;
            }
        }
        #[cfg(not(target_os = "linux"))]
        if let Some(status) = self.child_watch.on_readable() {
            self.on_child_exit(status)?;
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn retry_child(&mut self) -> Result<(), WorkerError> {
        if self.exit_status.is_none()
            && let Some(status) = self.child_watch.retry_due()
        {
            self.on_child_exit(status)?;
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn shard_child_pid(&self) -> Option<rustix::process::Pid> {
        (self.exit_status.is_none() && !self.child_watch.reaped && self.child_watch.retry.is_none())
            .then_some(self.child_watch.pid)
    }

    #[cfg(unix)]
    pub(super) fn queued_input_ready(&self) -> bool {
        !self.writer.has_pending() && !self.input_rx.commands.is_empty()
    }

    #[cfg(unix)]
    pub(super) fn on_pty_ready(&mut self, only_ready: bool) -> Result<(), WorkerError> {
        self.on_readable_with_spin(only_ready, || false)
    }

    pub(super) fn echo_pending(&self) -> bool {
        self.echo.due()
    }

    pub(super) fn on_wake(&mut self, wakeup: Wake) -> Result<bool, WorkerError> {
        let mut input_permit = None;
        let (commands, wakeup) = match wakeup {
            Wake::Input(QueuedInput { command, permit }) => {
                input_permit = Some(permit);
                self.echo.open();
                (take_control_slot(&self.slot, Some(command), false), None)
            }
            Wake::Command(command) => (take_control_slot(&self.slot, Some(command), true), None),
            wakeup => (take_control_slot(&self.slot, None, false), Some(wakeup)),
        };
        for command in commands {
            if !self.on_command(command)? {
                return Ok(false);
            }
        }
        match wakeup {
            None => {}
            Some(Wake::CommandsClosed) => {
                if !self.sharded {
                    self.on_commands_closed();
                    return Ok(false);
                }
                self.shutdown();
            }
            Some(Wake::Command(_) | Wake::Input(_)) => {
                unreachable!("commands and PTY input are dispatched above")
            }
            Some(Wake::Search(result)) => self.on_search(result)?,
            #[cfg(unix)]
            Some(Wake::PtyReadable) => self.on_pty_ready(true)?,
            #[cfg(any(target_os = "linux", not(unix)))]
            Some(Wake::PtyMessage(message)) => self.on_reader_message(message)?,
            Some(Wake::ChildExit(status)) => self.on_child_exit(status)?,
            Some(Wake::Deadline) => self.on_parse_deadline(),
        }
        #[cfg(unix)]
        if input_permit.is_some() && self.writer.has_pending() {
            self.active_input_permit = input_permit.take();
        }
        #[cfg(not(unix))]
        drop(input_permit);

        Ok(true)
    }

    fn on_command(&mut self, command: Command) -> Result<bool, WorkerError> {
        match command {
            Command::Text { view, text } => {
                if self.exit_status.is_none() {
                    let viewport_changed = if let Some(view) = view
                        && let Some(state) = self.active_views.get_mut(&view)
                    {
                        restore_view_state(&mut self.terminal, state, &self.word_separators)?;
                        if state.copy_mode.is_none() && state.search.is_some() {
                            self.search_worker.cancel(view);
                        }
                        prepare_live_input(&mut self.terminal, state)?
                    } else {
                        false
                    };
                    self.writer.write_all(text.as_bytes())?;
                    self.writer.flush()?;
                    if viewport_changed {
                        publish_active_views(
                            &mut self.terminal,
                            &self.publisher,
                            &mut self.frames,
                            SnapshotChange::View,
                            &mut self.active_views,
                            &self.word_separators,
                            SessionStatus::Running,
                        )?;
                    }
                }
            }
            Command::Key { view, input } => {
                if self.exit_status.is_none() {
                    let viewport_changed = if let Some(view) = view
                        && let Some(state) = self.active_views.get_mut(&view)
                    {
                        restore_view_state(&mut self.terminal, state, &self.word_separators)?;
                        if state.copy_mode.is_none() && state.search.is_some() {
                            self.search_worker.cancel(view);
                        }
                        prepare_live_input(&mut self.terminal, state)?
                    } else {
                        false
                    };
                    encode_key(
                        &self.terminal,
                        &mut self.key_encoder,
                        &mut self.key_event,
                        *input,
                        self.engine_knobs.erase_byte,
                        &mut self.writer,
                        &mut self.input_bytes,
                    )?;
                    if viewport_changed {
                        publish_active_views(
                            &mut self.terminal,
                            &self.publisher,
                            &mut self.frames,
                            SnapshotChange::View,
                            &mut self.active_views,
                            &self.word_separators,
                            SessionStatus::Running,
                        )?;
                    }
                }
            }
            Command::PastePreparedBytes {
                view,
                bytes,
                bracketed,
            } => {
                if self.exit_status.is_none() {
                    let viewport_changed = if let Some(view) = view
                        && let Some(state) = self.active_views.get_mut(&view)
                    {
                        restore_view_state(&mut self.terminal, state, &self.word_separators)?;
                        if state.copy_mode.is_none() && state.search.is_some() {
                            self.search_worker.cancel(view);
                        }
                        prepare_live_input(&mut self.terminal, state)?
                    } else {
                        false
                    };
                    write_prepared_paste_bytes(
                        &self.terminal,
                        &bytes,
                        bracketed,
                        &mut self.writer,
                    )?;
                    if viewport_changed {
                        publish_active_views(
                            &mut self.terminal,
                            &self.publisher,
                            &mut self.frames,
                            SnapshotChange::View,
                            &mut self.active_views,
                            &self.word_separators,
                            SessionStatus::Running,
                        )?;
                    }
                }
            }
            Command::Output(_) | Command::Wake => {}
            Command::RawInput(bytes) => {
                if self.exit_status.is_none() {
                    self.writer.write_all(&bytes)?;
                    self.writer.flush()?;
                    self.echo.open();
                }
            }
            Command::ArmRawOutputTap {
                token,
                output,
                reply,
            } => {
                self.raw_output_tap = Some((token, output));
                let _ = reply.send(true);
            }
            Command::Settle { reply } => {
                let _ = reply.send(());
            }
            Command::DisarmRawOutputTap { token, reply } => {
                if self
                    .raw_output_tap
                    .as_ref()
                    .is_some_and(|(armed, _)| *armed == token)
                {
                    self.raw_output_tap = None;
                }
                let _ = reply.send(());
            }
            Command::Resize(next) => {
                if next != self.geometry {
                    self.search_refresh_due = None;
                    self.geometry = next;
                    self.reported_size.set(self.geometry.size_report());
                    #[cfg(unix)]
                    self.master
                        .resize(self.geometry.pty_size())
                        .map_err(|error| WorkerError::Pty(error.to_string()))?;
                    #[cfg(not(unix))]
                    if let Some(master) = &self.master {
                        master
                            .resize(self.geometry.pty_size())
                            .map_err(|error| WorkerError::Pty(error.to_string()))?;
                    }
                    self.terminal.resize(
                        self.geometry.columns.max(1),
                        self.geometry.rows.max(1),
                        self.geometry.cell_width_px,
                        self.geometry.cell_height_px,
                    )?;
                    self.terminal
                        .set_scrollback_max_bytes(Some(scrollback_backstop_bytes(
                            self.max_scrollback.min(MAX_HISTORY_LIMIT),
                            self.geometry.columns.max(1),
                        )))?;
                    resize_copy_modes(
                        &mut self.inactive_views,
                        self.geometry.columns.max(1),
                        self.geometry.rows.max(1),
                        &mut self.search_worker,
                    )?;
                    for view in self.inactive_views.values_mut() {
                        view.invalidate_layout();
                    }
                    #[cfg(unix)]
                    drain_effects_if_writer_ready(&self.effects, &mut self.writer)?;
                    #[cfg(not(unix))]
                    drain_effects(&self.effects, &mut self.writer)?;
                    resize_copy_modes(
                        &mut self.active_views,
                        self.geometry.columns.max(1),
                        self.geometry.rows.max(1),
                        &mut self.search_worker,
                    )?;
                    for (view_id, view) in &mut self.active_views {
                        view.invalidate_layout();
                        reconcile_view_screen(&mut self.terminal, view, &self.word_separators)?;
                        if view.copy_mode.is_none() {
                            let _ = refresh_view_search(
                                &self.terminal,
                                *view_id,
                                view,
                                &mut self.search_worker,
                            )?;
                        }
                    }
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::Content,
                        &mut self.active_views,
                        &self.word_separators,
                        SessionStatus::Running,
                    )?;
                }
            }
            Command::SetWordSeparators(next) => {
                self.word_separators = *next;
                let mut selection_changed = false;
                for view in self.active_views.values_mut() {
                    if view.copy_mode.is_none()
                        && view
                            .selection
                            .as_ref()
                            .is_some_and(|selection| selection.mode == SelectionMode::Word)
                    {
                        restore_view_state(&mut self.terminal, view, &self.word_separators)?;
                        install_view_selection(&self.terminal, view, &self.word_separators)?;
                        selection_changed = true;
                    }
                }
                if selection_changed {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        SessionStatus::Running,
                    )?;
                }
            }
            Command::SetAllowPassthrough(next) => {
                self.passthrough.set_mode(next);
            }
            Command::SetWrapSearch(next) => {
                self.wrap_search = next;
            }
            Command::SetEngineKnobs(next) => {
                self.engine_knobs = next;
            }
            Command::CaptureCopySource { reply } => {
                let _ = reply.send(
                    capture_copy_source(&mut self.terminal)
                        .map_err(|_| TerminalCaptureError::ActorStopped),
                );
                self.compression.rearm();
            }
            Command::SetPendingCopySource(source) => {
                self.pending_copy_source = source;
            }
            Command::WriteDeadNotice(_) => {
                log::debug!("discarding a dead notice for a live pane");
            }
            Command::ResetScreen => {
                reset_pane_screen(&mut self.terminal)?;
                for view in self
                    .active_views
                    .values_mut()
                    .chain(self.inactive_views.values_mut())
                {
                    let state = view.active_mut();
                    state.selection = None;
                    state.hover_link = None;
                }
                self.terminal.set_selection(None)?;
                publish_active_views(
                    &mut self.terminal,
                    &self.publisher,
                    &mut self.frames,
                    SnapshotChange::Content,
                    &mut self.active_views,
                    &self.word_separators,
                    SessionStatus::Running,
                )?;
            }
            Command::SetAppearance(next) => {
                self.reported_color_scheme
                    .set(ghostty_color_scheme(next.color_scheme));
                apply_terminal_appearance(&mut self.terminal, &next)?;
                self.frames.dictionary.class_hints = ClassHints::new(&next);
                self.frames.reset_render();
                for view in self
                    .active_views
                    .values_mut()
                    .chain(self.inactive_views.values_mut())
                {
                    refresh_frozen_view_appearance(&mut self.terminal, view)?;
                }
                publish_active_views(
                    &mut self.terminal,
                    &self.publisher,
                    &mut self.frames,
                    SnapshotChange::Content,
                    &mut self.active_views,
                    &self.word_separators,
                    SessionStatus::Running,
                )?;
            }
            Command::AttachView(view) => {
                self.search_worker.cancel(view);
                activate_view(
                    &mut self.terminal,
                    view,
                    &mut self.active_views,
                    &mut self.inactive_views,
                    &self.word_separators,
                )?;
                if let Some(state) = self.active_views.get_mut(&view) {
                    let _ =
                        refresh_view_search(&self.terminal, view, state, &mut self.search_worker)?;
                    sync_viewport_anchor(&self.terminal, state)?;
                }
                if self.frames.shows(view, &self.active_views) {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        SessionStatus::Running,
                    )?;
                }
            }
            Command::DetachView(view) => {
                self.search_worker.cancel(view);
                let shown = self.frames.shows(view, &self.active_views);
                deactivate_view(
                    &mut self.terminal,
                    view,
                    &mut self.active_views,
                    &mut self.inactive_views,
                    &self.word_separators,
                )?;
                if shown {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        SessionStatus::Running,
                    )?;
                }
            }
            Command::ReleaseView(view) => {
                self.search_worker.forget(view);
                let shown = self.frames.shows(view, &self.active_views);
                self.frames.forget_view(view);
                if release_view(
                    &mut self.terminal,
                    view,
                    &mut self.active_views,
                    &mut self.inactive_views,
                )? && shown
                {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        SessionStatus::Running,
                    )?;
                }
            }
            Command::ViewAction { view, action } => {
                self.compression.rearm();
                if self.active_views.contains_key(&view) {
                    let explicitly_enters_copy_mode = matches!(
                        &action,
                        TerminalViewAction::EnterCopyMode
                            | TerminalViewAction::EnterCopyModeScrollExit
                            | TerminalViewAction::EnterCopyModeWith { .. }
                    );
                    let clears_history = matches!(&action, TerminalViewAction::ClearHistory);
                    let was_in_copy_mode = self
                        .active_views
                        .get(&view)
                        .is_some_and(|state| state.copy_mode.is_some());
                    if matches!(
                        &action,
                        TerminalViewAction::SearchBegin(_)
                            | TerminalViewAction::SearchUpdate(_)
                            | TerminalViewAction::SearchClose
                            | TerminalViewAction::ClearHistory
                    ) {
                        self.search_refresh_due = None;
                    }
                    let result = {
                        let state = self
                            .active_views
                            .get_mut(&view)
                            .expect("active view was checked above");
                        restore_view_state(&mut self.terminal, state, &self.word_separators)?;
                        normalize_view_action_result(apply_view_action(
                            &mut self.terminal,
                            view,
                            state,
                            action,
                            self.geometry,
                            &mut self.writer,
                            &mut self.mouse_encoder,
                            &mut self.mouse_event,
                            &mut self.input_bytes,
                            &mut self.search_worker,
                            self.wrap_search,
                            self.engine_knobs.mode_keys_vi,
                            &self.word_separators,
                            self.pasted_image_bindings.bound_numbers(),
                            &mut self.pending_copy_source,
                            &mut self.pane_search,
                        ))?
                    };
                    self.publisher
                        .publish_search_string(self.pane_search.as_ref());
                    let is_in_copy_mode = self
                        .active_views
                        .get(&view)
                        .is_some_and(|state| state.copy_mode.is_some());
                    let entered_copy_mode = !was_in_copy_mode && is_in_copy_mode;
                    let leaves_copy_mode = clears_history || (was_in_copy_mode && !is_in_copy_mode);
                    if explicitly_enters_copy_mode || entered_copy_mode || leaves_copy_mode {
                        self.frames.mark_full_dirty(&self.terminal)?;
                    }
                    if leaves_copy_mode {
                        let state = self
                            .active_views
                            .get_mut(&view)
                            .expect("active view was checked above");
                        reconcile_view_screen(&mut self.terminal, state, &self.word_separators)?;
                    }
                    if matches!(
                        &result,
                        ViewActionResult::Snapshot
                            | ViewActionResult::ContentSnapshot
                            | ViewActionResult::Copy(_)
                    ) {
                        let state = self
                            .active_views
                            .get_mut(&view)
                            .expect("active view was checked above");
                        sync_viewport_anchor(&self.terminal, state)?;
                    }
                    match result {
                        ViewActionResult::None => {}
                        ViewActionResult::Snapshot => publish_active_views(
                            &mut self.terminal,
                            &self.publisher,
                            &mut self.frames,
                            SnapshotChange::View,
                            &mut self.active_views,
                            &self.word_separators,
                            SessionStatus::Running,
                        )?,
                        ViewActionResult::OverlaySnapshot => publish_active_views(
                            &mut self.terminal,
                            &self.publisher,
                            &mut self.frames,
                            SnapshotChange::Overlay,
                            &mut self.active_views,
                            &self.word_separators,
                            SessionStatus::Running,
                        )?,
                        ViewActionResult::ContentSnapshot => publish_active_views(
                            &mut self.terminal,
                            &self.publisher,
                            &mut self.frames,
                            SnapshotChange::Content,
                            &mut self.active_views,
                            &self.word_separators,
                            SessionStatus::Running,
                        )?,
                        ViewActionResult::Copy(copy) => {
                            let view_changed = copy.view_changed;
                            self.publisher.copy_ready(view, copy)?;
                            if view_changed {
                                publish_active_views(
                                    &mut self.terminal,
                                    &self.publisher,
                                    &mut self.frames,
                                    SnapshotChange::View,
                                    &mut self.active_views,
                                    &self.word_separators,
                                    SessionStatus::Running,
                                )?;
                            }
                        }
                        ViewActionResult::OpenUri(uri) => self.publisher.open_uri(view, uri)?,
                    }
                }
            }
            Command::Capture(request) => {
                let mut copy_modes = self
                    .active_views
                    .values()
                    .filter_map(|view| view.copy_mode.as_deref());
                let mode = match (copy_modes.next(), copy_modes.next()) {
                    (Some(mode), None) => Some(mode),
                    _ => None,
                };
                if let Some(capture) = CaptureWork::start(&self.terminal, mode, *request) {
                    self.captures.push_back(capture);
                }
                self.compression.rearm();
            }
            Command::PointerContext(request) => {
                let PointerContextRequest {
                    view,
                    column,
                    row,
                    reply,
                } = *request;
                let mode = self
                    .active_views
                    .get(&view)
                    .and_then(|view| view.copy_mode.as_deref());
                let result =
                    pointer_context(&self.terminal, mode, column, row, &self.word_separators)
                        .unwrap_or_default();
                let _ = reply.send(result);
            }
            Command::SemanticCapture(request) => {
                let _ = request.reply.send(capture_last_command(&self.terminal));
                self.compression.rearm();
            }
            Command::History(request) => {
                let HistoryCommand {
                    start,
                    count,
                    reply,
                } = *request;
                let _ = reply.send(capture_history(
                    &self.terminal,
                    start,
                    count,
                    &self.frames.dictionary.class_hints,
                ));
                self.compression.rearm();
            }
            Command::KittyImage(request) => {
                let image = self
                    .frames
                    .generations
                    .kitty
                    .as_mut()
                    .map_or(Ok(None), |kitty| {
                        kitty.image(&self.terminal, request.image_id)
                    })
                    .unwrap_or_else(|error| {
                        log::warn!("could not export Kitty image {}: {error}", request.image_id);
                        None
                    });
                let _ = request.reply.send(image);
            }
            Command::KittyImageGeneration(request) => {
                let generation = self
                    .frames
                    .generations
                    .kitty
                    .as_ref()
                    .map_or(Ok(None), |_| {
                        KittyGraphicsState::image_generation(&self.terminal, request.image_id)
                    })
                    .unwrap_or_else(|error| {
                        log::warn!(
                            "could not read Kitty image {} generation: {error}",
                            request.image_id
                        );
                        None
                    });
                let _ = request.reply.send(generation);
            }
            Command::PendingPasteOpened { token } => {
                for expired in self.pasted_image_bindings.expire(Instant::now()) {
                    self.publisher.pending_paste_expired(expired)?;
                }
                self.pasted_image_bindings
                    .open(&self.terminal, token, Instant::now())?;
            }
            Command::UnbindPastedImage { number } => {
                if self.pasted_image_bindings.unbind(number) {
                    let mut active_hover_changed = false;
                    for view in self.active_views.values_mut() {
                        active_hover_changed |= clear_pasted_image_hover(view, number);
                    }
                    for view in self.inactive_views.values_mut() {
                        clear_pasted_image_hover(view, number);
                    }
                    if active_hover_changed {
                        publish_active_views(
                            &mut self.terminal,
                            &self.publisher,
                            &mut self.frames,
                            SnapshotChange::Overlay,
                            &mut self.active_views,
                            &self.word_separators,
                            SessionStatus::Running,
                        )?;
                    }
                }
            }
            Command::Terminate => {
                if !self.terminating {
                    self.terminating = true;
                    self.termination_deadline = Some(Instant::now() + TERMINATION_GRACE);
                    #[cfg(unix)]
                    signal_terminal_process_groups(
                        &self.master,
                        self.shell_process_id,
                        rustix::process::Signal::TERM,
                    );
                    #[cfg(not(unix))]
                    let _ = self.killer.kill();
                }
            }
            Command::Shutdown if self.sharded => self.shutdown(),
            Command::Shutdown => {
                let _ = self.killer.kill();
                if self.exit_status.is_none() {
                    #[cfg(all(unix, not(target_os = "linux")))]
                    let _ = self.child_watch.wait_timeout(TERMINATION_KILL_WAIT);
                    #[cfg(any(target_os = "linux", not(unix)))]
                    let _ = self.exit_rx.recv_timeout(TERMINATION_KILL_WAIT);
                }
                return Ok(false);
            }
            Command::SetViewStream(view, stream) => {
                if self.frames.set_stream(view, stream) && self.active_views.contains_key(&view) {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        SessionStatus::Running,
                    )?;
                }
            }
            Command::SetPreviewWatch(watch) => {
                let started = watch && !self.frames.preview;
                self.frames.preview = watch;
                if started {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        SessionStatus::Running,
                    )?;
                } else {
                    self.frames.release_unused(&self.active_views);
                }
            }
            Command::FreshViewport(reply) => {
                self.frames.force_fallback = true;
                publish_views(
                    &mut self.terminal,
                    &self.publisher,
                    &mut self.frames,
                    SnapshotChange::View,
                    &mut self.active_views,
                    &self.word_separators,
                    SessionStatus::Running,
                    false,
                )?;
                self.frames.force_fallback = false;
                let _ = reply.send(self.publisher.latest_fallback());
            }
        }

        Ok(true)
    }

    #[cfg(target_os = "linux")]
    fn wait_child_linux(&mut self, timeout: Duration) -> bool {
        if let Some(watch) = &self.linux_child {
            let mut fds = [rustix::event::PollFd::new(
                &watch.pidfd,
                rustix::event::PollFlags::IN,
            )];
            let deadline = Instant::now() + timeout;
            loop {
                if watch.reap() {
                    return true;
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return false;
                }
                let timespec =
                    rustix::event::Timespec::try_from(remaining).expect("bounded child wait");
                match rustix::event::poll(&mut fds, Some(&timespec)) {
                    Ok(_) | Err(rustix::io::Errno::INTR) => {}
                    Err(_) => return false,
                }
            }
        }
        self.exit_rx.recv_timeout(timeout).is_ok()
    }

    fn on_commands_closed(&mut self) {
        if self.terminating {
            let remaining = self
                .termination_deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_default();
            #[cfg(all(unix, not(target_os = "linux")))]
            let exited = self.child_watch.wait_timeout(remaining).is_some();
            #[cfg(target_os = "linux")]
            let exited = self.wait_child_linux(remaining);
            #[cfg(not(unix))]
            let exited = self.exit_rx.recv_timeout(remaining).is_ok();
            if !exited && !self.termination_escalated {
                #[cfg(unix)]
                signal_terminal_process_groups(
                    &self.master,
                    self.shell_process_id,
                    rustix::process::Signal::KILL,
                );
                #[cfg(not(unix))]
                let _ = self.killer.kill();
                #[cfg(all(unix, not(target_os = "linux")))]
                let _ = self.child_watch.wait_timeout(TERMINATION_KILL_WAIT);
                #[cfg(target_os = "linux")]
                let _ = self.wait_child_linux(TERMINATION_KILL_WAIT);
                #[cfg(not(unix))]
                let _ = self.exit_rx.recv_timeout(TERMINATION_KILL_WAIT);
            }
        } else {
            let _ = self.killer.kill();
            if self.exit_status.is_none() {
                #[cfg(all(unix, not(target_os = "linux")))]
                let _ = self.child_watch.wait_timeout(TERMINATION_KILL_WAIT);
                #[cfg(target_os = "linux")]
                let _ = self.wait_child_linux(TERMINATION_KILL_WAIT);
                #[cfg(not(unix))]
                let _ = self.exit_rx.recv_timeout(TERMINATION_KILL_WAIT);
            }
        }
    }

    fn on_search(&mut self, result: SearchResults) -> Result<(), WorkerError> {
        if apply_search_results(
            &mut self.terminal,
            &mut self.active_views,
            &mut self.inactive_views,
            &mut self.search_worker,
            result,
        )? {
            publish_active_views(
                &mut self.terminal,
                &self.publisher,
                &mut self.frames,
                SnapshotChange::View,
                &mut self.active_views,
                &self.word_separators,
                SessionStatus::Running,
            )?;
        }

        Ok(())
    }

    #[cfg(unix)]
    pub(super) fn on_readable_with_spin(
        &mut self,
        only_ready: bool,
        mut should_yield: impl FnMut() -> bool,
    ) -> Result<(), WorkerError> {
        let mut burst = 0_usize;
        let mut spins = 0_u32;
        let turn_started = Instant::now();
        loop {
            if !self.output_read_ahead() {
                break;
            }
            match self.read_pty_buffer() {
                Ok(0) => {
                    self.reader_eof = true;
                    break;
                }
                Ok(length) => {
                    self.consume_read_buffer(length)?;
                    if spins > 0 {
                        self.bridge_spins = (self.bridge_spins * 2).min(PTY_BRIDGE_SPIN_MAX);
                    }
                    burst += length;
                    spins = 0;
                    if burst >= PTY_DRAIN_TURN_BYTES
                        || self.raw_output_parse_backlog_bytes >= RAW_OUTPUT_PARSE_BACKLOG_BYTES
                        || turn_started.elapsed() >= PTY_DRAIN_TURN_TIME
                        || should_yield()
                    {
                        break;
                    }
                }
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => {
                    if burst >= PTY_BRIDGE_THRESHOLD_BYTES {
                        if only_ready
                            && spins < self.bridge_spins
                            && turn_started.elapsed() < PTY_DRAIN_TURN_TIME
                            && (!spins.is_multiple_of(32) || !should_yield())
                        {
                            spins += 1;
                            continue;
                        }
                        self.bridge_spins = (self.bridge_spins / 2).max(PTY_BRIDGE_SPIN_MIN);
                    }
                    break;
                }
                Err(_) => {
                    self.reader_eof = true;
                    break;
                }
            }
        }

        Ok(())
    }

    #[cfg(unix)]
    fn read_pty_buffer(&mut self) -> Result<usize, rustix::io::Errno> {
        let fd = self.drain_fd.as_ref().expect("direct PTY reader");
        let length = rustix::io::read(fd, &mut self.read_buffer[..])?;
        #[cfg(target_os = "linux")]
        let mut length = length;
        #[cfg(target_os = "linux")]
        {
            let mut reads = 1;
            while length > 0 && length < self.read_buffer.len() && reads < 8 {
                match rustix::io::read(fd, &mut self.read_buffer[length..]) {
                    Ok(0) => break,
                    Ok(extra) => {
                        length += extra;
                        reads += 1;
                    }
                    Err(rustix::io::Errno::INTR) => {}
                    Err(_) => break,
                }
            }
        }
        Ok(length)
    }

    #[cfg(unix)]
    fn consume_read_buffer(&mut self, length: usize) -> Result<(), WorkerError> {
        log::trace!(
            target: "zz_terminal::diagnostics::pty",
            "read length={length} bytes={:?} text={:?}",
            &self.read_buffer[..length],
            String::from_utf8_lossy(&self.read_buffer[..length]),
        );
        if self.raw_output_tap.is_some() || !self.raw_output_parse_backlog.is_empty() {
            let bytes = Arc::<[u8]>::from(&self.read_buffer[..length]);
            if let Some(token) = tap_raw_output_arc(&mut self.raw_output_tap, &bytes) {
                self.publisher.raw_output_tap_closed(token)?;
            }
            self.raw_output_parse_backlog_bytes = self
                .raw_output_parse_backlog_bytes
                .saturating_add(bytes.len());
            self.raw_output_parse_backlog.push_back((bytes, 0));
        } else {
            let started = diagnostic_timer();
            let parsed = feed_pty_output(
                &mut self.terminal,
                &mut self.passthrough,
                &mut EngineOutput {
                    filter: &mut self.engine_filter,
                    knobs: self.engine_knobs,
                    renames: &mut self.engine_renames,
                    bar: &mut self.engine_bar,
                    last_command_status: &mut self.engine_last_command_status,
                },
                &self.read_buffer[..length],
            );
            self.vt_diagnostics.record(parsed, started);
            self.output_pending |= parsed > 0;
        }
        Ok(())
    }

    #[cfg(any(target_os = "linux", not(unix)))]
    fn on_reader_message(&mut self, message: ReaderMessage) -> Result<(), WorkerError> {
        match message {
            ReaderMessage::Data { buffer, length } => {
                let mut closed_tap = None;
                let mut consumed_output = false;
                let max_chunks = self
                    .raw_output_tap
                    .as_ref()
                    .map_or(PTY_BUFFER_POOL_SIZE, |(_, tap)| {
                        RAW_OUTPUT_TAP_PENDING_CHUNKS.saturating_sub(tap.sender.len())
                    });
                self.reader_eof |= drain_pty_output_burst(
                    &self.output_rx,
                    buffer,
                    length,
                    cfg!(not(unix)) && self.sharded,
                    max_chunks,
                    |buffer, length| {
                        if self.raw_output_tap.is_some()
                            || !self.raw_output_parse_backlog.is_empty()
                        {
                            log::trace!(
                                target: "zz_terminal::diagnostics::pty",
                                "read length={length} bytes={:?} text={:?}",
                                &buffer[..length],
                                String::from_utf8_lossy(&buffer[..length]),
                            );
                            let bytes = Arc::<[u8]>::from(&buffer[..length]);
                            closed_tap = closed_tap
                                .or_else(|| tap_raw_output_arc(&mut self.raw_output_tap, &bytes));
                            self.raw_output_parse_backlog_bytes = self
                                .raw_output_parse_backlog_bytes
                                .saturating_add(bytes.len());
                            self.raw_output_parse_backlog.push_back((bytes, 0));
                            let _ = self.recycle_tx.try_send(buffer);
                        } else {
                            let started = diagnostic_timer();
                            let (closed, parsed) = consume_pty_output(
                                &mut self.terminal,
                                &mut self.passthrough,
                                &mut EngineOutput {
                                    filter: &mut self.engine_filter,
                                    knobs: self.engine_knobs,
                                    renames: &mut self.engine_renames,
                                    bar: &mut self.engine_bar,
                                    last_command_status: &mut self.engine_last_command_status,
                                },
                                &mut self.raw_output_tap,
                                buffer,
                                length,
                                &self.recycle_tx,
                            );
                            closed_tap = closed_tap.or(closed);
                            self.vt_diagnostics.record(parsed, started);
                            consumed_output |= parsed > 0;
                        }
                    },
                );
                if let Some(token) = closed_tap {
                    self.publisher.raw_output_tap_closed(token)?;
                }
                self.output_pending |= consumed_output;
            }
            ReaderMessage::Eof => self.reader_eof = true,
        }
        Ok(())
    }

    fn on_child_exit(&mut self, status: std::io::Result<ExitStatus>) -> Result<(), WorkerError> {
        self.exit_status = Some(status?);
        #[cfg(windows)]
        if let Some(master) = self.master.take() {
            let _ = self.master_close_tx.send(master);
        }

        Ok(())
    }

    pub(super) fn on_parse_deadline(&mut self) {
        if self.raw_output_parse_backlog.is_empty() {
            return;
        }
        let started = diagnostic_timer();
        let parsed = drain_raw_output_parse_backlog(
            &mut self.terminal,
            &mut self.passthrough,
            &mut EngineOutput {
                filter: &mut self.engine_filter,
                knobs: self.engine_knobs,
                renames: &mut self.engine_renames,
                bar: &mut self.engine_bar,
                last_command_status: &mut self.engine_last_command_status,
            },
            &mut self.raw_output_parse_backlog,
            &mut self.raw_output_parse_backlog_bytes,
            &mut self.raw_output_parse_buffer,
        );
        self.output_pending |= parsed > 0;
        self.vt_diagnostics.record(parsed, started);
    }

    pub(super) fn ready_to_finish(&self) -> bool {
        self.exit_status.is_some()
            && self.reader_eof
            && self.raw_output_parse_backlog.is_empty()
            && self.captures.is_empty()
    }

    #[cfg(any(target_os = "linux", not(unix)))]
    fn drain_remaining_output(&mut self) -> Result<bool, WorkerError> {
        let mut had_output = false;
        while let Ok(ReaderMessage::Data { buffer, length }) = self.output_rx.try_recv() {
            let (closed, parsed) = consume_pty_output(
                &mut self.terminal,
                &mut self.passthrough,
                &mut EngineOutput {
                    filter: &mut self.engine_filter,
                    knobs: self.engine_knobs,
                    renames: &mut self.engine_renames,
                    bar: &mut self.engine_bar,
                    last_command_status: &mut self.engine_last_command_status,
                },
                &mut self.raw_output_tap,
                buffer,
                length,
                &self.recycle_tx,
            );
            if let Some(token) = closed {
                self.publisher.raw_output_tap_closed(token)?;
            }
            had_output |= parsed > 0;
        }
        Ok(had_output)
    }

    pub(super) fn finish(mut self) -> Result<DeadPane, WorkerError> {
        #[cfg(all(unix, not(target_os = "linux")))]
        let had_output = false;
        #[cfg(any(target_os = "linux", not(unix)))]
        let had_output = self.drain_remaining_output()?;
        #[cfg(unix)]
        drain_effects_if_writer_ready(&self.effects, &mut self.writer)?;
        #[cfg(not(unix))]
        drain_effects(&self.effects, &mut self.writer)?;
        let Self {
            control_rx,
            slot,
            publisher,
            max_scrollback,
            geometry,
            mut terminal,
            reported_color_scheme,
            mut frames,
            word_separators,
            wrap_search,
            engine_knobs,
            pending_copy_source,
            pane_search,
            engine_filter,
            mut engine_last_command_status,
            mut active_views,
            mut inactive_views,
            mut pasted_image_bindings,
            mut exit_status,
            terminating,
            mut search_worker,
            search_results,
            output_pending,
            ..
        } = self;
        if (had_output || output_pending)
            && let Some((token, number)) = pasted_image_bindings.observe(&terminal)?
        {
            publisher.placeholder_bound(token, number)?;
        }
        let output_screen = terminal.active_screen()?;
        if had_output {
            for view in inactive_views.values_mut() {
                view.note_output(output_screen);
            }
        }
        for (view_id, view) in &mut active_views {
            if had_output {
                note_output_and_revalidate_image_hover(
                    &mut terminal,
                    view,
                    output_screen,
                    &word_separators,
                    pasted_image_bindings.bound_numbers(),
                )?;
            } else {
                reconcile_view_screen(&mut terminal, view, &word_separators)?;
            }
            if view.copy_mode.is_none() {
                search_worker.cancel(*view_id);
                complete_view_search(&mut terminal, view)?;
            }
        }
        publisher.set_facts(engine_filter.facts(&terminal)?);
        if let Some(status) = engine_last_command_status.take() {
            publisher.set_last_command_status(status.code());
        }
        let status = exit_status.take().expect("checked above");
        let signal = status.signal().and_then(signal_number);
        publisher.set_completion(TerminalProcessExit {
            code: status.exit_code(),
            signal,
        });
        if had_output || output_pending {
            publisher.mark_output_activity();
        }
        frames.force_fallback = !terminating;
        publish_active_views(
            &mut terminal,
            &publisher,
            &mut frames,
            SnapshotChange::Content,
            &mut active_views,
            &word_separators,
            SessionStatus::exited(status.exit_code(), status.signal().map(str::to_owned)),
        )?;
        Ok(DeadPane {
            control_rx,
            slot,
            publisher,
            engine_filter,
            notice_deadline: Instant::now() + DEAD_NOTICE_WAIT,
            retained: false,
            surface: SurfaceTerminal {
                terminal,
                geometry,
                frames,
                active_views,
                inactive_views,
                word_separators,
                wrap_search,
                mode_keys_vi: engine_knobs.mode_keys_vi,
                reported_color_scheme,
                max_scrollback,
                status: SessionStatus::exited(
                    status.exit_code(),
                    status.signal().map(str::to_owned),
                ),
                pending_commands: Vec::new(),
                captures: self.captures,
                pending_copy_source,
                pane_search,
                search: Some((search_worker, search_results)),
            },
        })
    }
}

pub(super) struct DeadPane {
    control_rx: Receiver<Command>,
    slot: Arc<Mutex<ControlSlot>>,
    publisher: Publisher,
    engine_filter: EngineFilter,
    notice_deadline: Instant,
    retained: bool,
    surface: SurfaceTerminal<'static, 'static>,
}

impl DeadPane {
    pub(super) fn next_deadline(&self) -> Instant {
        if self.surface.captures.is_empty() {
            self.notice_deadline
        } else {
            Instant::now()
        }
    }

    pub(super) fn try_wake(&self) -> Option<Wake> {
        match self.control_rx.try_recv() {
            Ok(command) => Some(Wake::Command(command)),
            Err(crossbeam_channel::TryRecvError::Disconnected) => Some(Wake::CommandsClosed),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
        }
    }

    pub(super) fn wait_for_wake(&self) -> Wake {
        self.control_rx
            .recv_timeout(
                self.next_deadline()
                    .saturating_duration_since(Instant::now()),
            )
            .map_or(Wake::Deadline, Wake::Command)
    }

    pub(super) fn on_wake(&mut self, wake: Wake) -> Result<bool, WorkerError> {
        match wake {
            Wake::Command(Command::WriteDeadNotice(text)) => {
                complete_dead_notice_command(&self.slot);
                if let Some(text) = text {
                    self.retained = true;
                    write_dead_notice(&mut self.surface.terminal, &text)?;
                    self.publisher
                        .set_facts(self.engine_filter.facts(&self.surface.terminal)?);
                    self.surface.frames.force_fallback = true;
                    publish_active_views(
                        &mut self.surface.terminal,
                        &self.publisher,
                        &mut self.surface.frames,
                        SnapshotChange::Content,
                        &mut self.surface.active_views,
                        &self.surface.word_separators,
                        self.surface.status.clone(),
                    )?;
                }
                return Ok(false);
            }
            Wake::Command(Command::Shutdown | Command::Terminate) | Wake::CommandsClosed => {
                return Ok(false);
            }
            Wake::Command(Command::Capture(request)) => {
                let mut modes = self
                    .surface
                    .active_views
                    .values()
                    .filter_map(|view| view.copy_mode.as_deref());
                let mode = match (modes.next(), modes.next()) {
                    (Some(mode), None) => Some(mode),
                    _ => None,
                };
                if let Some(capture) = CaptureWork::start(&self.surface.terminal, mode, *request) {
                    self.surface.captures.push_back(capture);
                }
                complete_dead_notice_command(&self.slot);
            }
            Wake::Command(command) => {
                if let Some(command) = reply_before_dead_notice(
                    &mut self.surface.terminal,
                    &self.surface.frames.dictionary.class_hints,
                    &self.surface.active_views,
                    &self.surface.word_separators,
                    &self.slot,
                    command,
                ) {
                    self.surface.pending_commands.push(command);
                }
            }
            Wake::Deadline => {
                step_capture_work(&mut self.surface.captures);
                if self.surface.captures.is_empty() && Instant::now() >= self.notice_deadline {
                    return Ok(false);
                }
            }
            _ => {}
        }
        Ok(true)
    }

    pub(super) fn into_surface(
        self,
    ) -> Result<Option<surface_actor::SurfaceActor<'static, 'static>>, WorkerError> {
        if !self.retained {
            return Ok(None);
        }
        self.publisher.set_foreground_source(None);
        let mut surface = self.surface;
        if !*NO_COMPRESS {
            surface.terminal.compress(CompressionMode::Full)?;
        }
        if self
            .slot
            .lock()
            .deferred
            .iter()
            .any(|(remaining, _)| *remaining == 0)
        {
            surface.pending_commands.push(Command::Wake);
        }
        surface_actor::SurfaceActor::new(self.control_rx, self.slot, self.publisher, surface, false)
            .map(Some)
    }
}
