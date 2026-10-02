use super::*;

#[derive(Default)]
pub(super) struct Load {
    pub(super) source: Option<(PathBuf, usize)>,
    waiters: Vec<Arc<terminal_requests::CommandState>>,
    loading: bool,
}

#[derive(Default)]
pub(super) struct Saves {
    pending: VecDeque<Save>,
    running: bool,
}

struct Save {
    path: PathBuf,
    command: Vec<String>,
    search: Vec<String>,
    done: Option<Arc<terminal_requests::CommandState>>,
}

impl Shared {
    pub(super) fn ensure_prompt_history(self: &Arc<Self>) {
        if self.prompt_history_settled.load(Ordering::Acquire) {
            return;
        }
        let wait = terminal_requests::CommandWait::new(self);
        let state = wait.start();
        {
            let mut load = self.prompt_history_source.lock();
            if self.prompt_history_settled.load(Ordering::Acquire) {
                state.resolve(Ok(Execution::default()));
            } else {
                load.waiters.push(state);
                if let Some((path, limit)) = load.source.take() {
                    load.loading = true;
                    if let Err(error) = self.submit_helper(helpers::Task::HistoryLoad {
                        path: path.clone(),
                        limit,
                        complete: Box::new(|shared, (command, search)| {
                            let waiters = {
                                let mut load = shared.prompt_history_source.lock();
                                load.loading = false;
                                let mut inner = shared.inner.lock();
                                inner.command_history = command;
                                inner.search_history = search;
                                shared.prompt_history_settled.store(true, Ordering::Release);
                                std::mem::take(&mut load.waiters)
                            };
                            for waiter in waiters {
                                waiter.resolve(Ok(Execution::default()));
                            }
                        }),
                    }) {
                        load.loading = false;
                        load.source = Some((path, limit));
                        log::warn!("could not load prompt history: {error}");
                        for waiter in load.waiters.drain(..) {
                            waiter.resolve(Err(std::io::Error::other(error.to_string()).into()));
                        }
                    }
                } else if !load.loading {
                    self.prompt_history_settled.store(true, Ordering::Release);
                    for waiter in load.waiters.drain(..) {
                        waiter.resolve(Ok(Execution::default()));
                    }
                }
            }
        }
        let _ = wait.finish(self, Execution::default());
    }

    pub(super) fn persist_prompt_history_with_observer<BeforeLock, AfterLock>(
        self: &Arc<Self>,
        before_lock: BeforeLock,
        after_lock: AfterLock,
    ) where
        BeforeLock: FnOnce(),
        AfterLock: FnOnce(),
    {
        before_lock();
        let queued = self.command_item.as_ref().is_some_and(|item| {
            let item = item.lock();
            item.loop_wait || {
                #[cfg(unix)]
                {
                    item.loop_leaf
                }
                #[cfg(not(unix))]
                {
                    false
                }
            }
        });
        let wait = (!self.helpers.on_loop_thread() || queued)
            .then(|| terminal_requests::CommandWait::new(self));
        let done = wait.as_ref().map(terminal_requests::CommandWait::start);
        {
            let mut saves = self.prompt_history_effects.lock();
            after_lock();
            let save = {
                let inner = self.inner.lock();
                prompt_history_path(inner.engine.history_file()).map(|path| Save {
                    path,
                    command: inner.command_history.clone(),
                    search: inner.search_history.clone(),
                    done: done.clone(),
                })
            };
            if let Some(save) = save {
                saves.pending.push_back(save);
                self.start_prompt_history_save(&mut saves);
            } else if let Some(done) = &done {
                done.resolve(Ok(Execution::default()));
            }
        }
        if let Some(wait) = wait {
            let _ = wait.finish(self, Execution::default());
        }
    }

    fn start_prompt_history_save(self: &Arc<Self>, saves: &mut Saves) {
        if saves.running {
            return;
        }
        while let Some(save) = saves.pending.pop_front() {
            let failed = save.done.clone();
            saves.running = true;
            let submitted = self.submit_helper(helpers::Task::HistorySave {
                path: save.path,
                command: save.command,
                search: save.search,
                complete: Box::new(move |shared, ()| {
                    {
                        let mut saves = shared.prompt_history_effects.lock();
                        saves.running = false;
                        shared.start_prompt_history_save(&mut saves);
                    }
                    if let Some(done) = save.done {
                        done.resolve(Ok(Execution::default()));
                    }
                }),
            });
            if let Err(error) = submitted {
                saves.running = false;
                log::warn!("could not save prompt history: {error}");
                if let Some(failed) = failed {
                    failed.resolve(Err(error.into()));
                }
            } else {
                break;
            }
        }
    }
}
