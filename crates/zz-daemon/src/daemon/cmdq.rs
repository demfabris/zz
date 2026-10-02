use super::*;

static NEXT_ITEM: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ItemId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct QueueId(u64);

impl ItemId {
    pub(super) const NONE: Self = Self(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ContinuationToken {
    item: ItemId,
    generation: u64,
}

#[derive(Clone)]
pub(super) struct WaitContinuation {
    pub(super) token: ContinuationToken,
    completion: Arc<WaitCompletion>,
    owner: Option<Weak<OutboundMailbox>>,
}

#[derive(Default)]
struct WaitCompletion {
    ready: Mutex<bool>,
    changed: parking_lot::Condvar,
}

impl WaitContinuation {
    pub(super) fn new(
        token: Option<ContinuationToken>,
        owner: Option<Weak<OutboundMailbox>>,
    ) -> Self {
        Self {
            token: token.unwrap_or_else(|| ContinuationToken {
                item: ItemId(NEXT_ITEM.fetch_add(1, Ordering::Relaxed)),
                generation: 1,
            }),
            completion: Arc::new(WaitCompletion::default()),
            owner,
        }
    }

    pub(super) fn complete(&self) -> bool {
        let mut ready = self.completion.ready.lock();
        if *ready {
            return false;
        }
        *ready = true;
        self.completion.changed.notify_all();
        drop(ready);
        if let Some(owner) = self.owner.as_ref().and_then(Weak::upgrade) {
            owner.notify_one();
        }
        true
    }

    pub(super) fn rearm(&self) {
        *self.completion.ready.lock() = false;
    }

    pub(super) fn ready(&self) -> bool {
        *self.completion.ready.lock()
    }

    pub(super) fn wait(&self) {
        let mut ready = self.completion.ready.lock();
        while !*ready {
            self.completion.changed.wait(&mut ready);
        }
    }
}

type ReplyCallback<T> = Box<dyn FnOnce(Option<T>) + Send>;

pub(crate) struct Reply<T> {
    callback: Mutex<Option<ReplyCallback<T>>>,
}

impl<T> std::fmt::Debug for Reply<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reply").finish_non_exhaustive()
    }
}

impl<T> Reply<T> {
    pub(crate) fn new(callback: impl FnOnce(Option<T>) + Send + 'static) -> Self {
        Self {
            callback: Mutex::new(Some(Box::new(callback))),
        }
    }

    pub(crate) fn close(&self) {
        if let Some(callback) = self.callback.lock().take() {
            callback(None);
        }
    }

    pub(crate) fn try_send(&self, value: T) {
        if let Some(callback) = self.callback.lock().take() {
            callback(Some(value));
        }
    }
}

impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        if let Some(callback) = self.callback.get_mut().take() {
            callback(None);
        }
    }
}

#[cfg(test)]
impl<T: Send + 'static> From<crossbeam_channel::Sender<T>> for Reply<T> {
    fn from(sender: crossbeam_channel::Sender<T>) -> Self {
        Self::new(move |value| {
            if let Some(value) = value {
                let _ = sender.try_send(value);
            }
        })
    }
}

#[cfg(test)]
impl<T: Send + 'static> From<mpsc::Sender<T>> for Reply<T> {
    fn from(sender: mpsc::Sender<T>) -> Self {
        Self::new(move |value| {
            if let Some(value) = value {
                let _ = sender.send(value);
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Ready,
    Waiting(ContinuationToken),
    Done,
}

pub(super) struct CommandItem<T> {
    pub(super) id: ItemId,
    pub(super) queue: QueueId,
    state: Cell<State>,
    generation: Cell<u64>,
    value: T,
}

impl<T> CommandItem<T> {
    pub(super) fn new(queue: Option<QueueId>, value: T) -> Self {
        let id = NEXT_ITEM.fetch_add(1, Ordering::Relaxed);
        Self {
            id: ItemId(id),
            queue: queue.unwrap_or(QueueId(id)),
            state: Cell::new(State::Ready),
            generation: Cell::new(0),
            value,
        }
    }

    pub(super) fn state(&self) -> State {
        self.state.get()
    }

    pub(super) fn wait(&self) -> Option<ContinuationToken> {
        match self.state.get() {
            State::Done => None,
            State::Waiting(token) => Some(token),
            State::Ready => {
                let generation = self
                    .generation
                    .get()
                    .checked_add(1)
                    .expect("continuation generation");
                self.generation.set(generation);
                let token = ContinuationToken {
                    item: self.id,
                    generation,
                };
                self.state.set(State::Waiting(token));
                Some(token)
            }
        }
    }

    pub(super) fn resume(&self, token: ContinuationToken) -> bool {
        if self.state.get() != State::Waiting(token) {
            return false;
        }
        self.state.set(State::Ready);
        true
    }

    pub(super) fn finish(&self) -> bool {
        if self.state.replace(State::Done) == State::Done {
            return false;
        }
        true
    }
}

impl<T> std::ops::Deref for CommandItem<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> std::ops::DerefMut for CommandItem<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

#[cfg(test)]
#[path = "cmdq_tests.rs"]
mod tests;

pub(super) fn start_key_listing(
    engine: &MuxEngine,
    context: &ExecutionContext,
    args: &[RawText],
    hooks: &mut impl StatusHooks,
    pending: &mut Option<zz_mux::KeyListing>,
) -> Result<Execution, ServerError> {
    let mut listing = engine
        .start_key_listing(context, args, hooks)
        .map_err(|error| {
            zz_protocol::catalog_command_spec("list-keys")
                .unwrap()
                .classify_usage_error(error)
        })?;
    if let Some(execution) = engine.step_key_listing(&mut listing, hooks) {
        Ok(execution)
    } else {
        *pending = Some(listing);
        Ok(Execution::default())
    }
}

pub(super) fn render_key_listing(
    shared: &Arc<Shared>,
    client: ClientId,
    mut listing: zz_mux::KeyListing,
    facts: FormatHookFacts,
    variables: BTreeMap<String, String>,
    state: Arc<terminal_requests::CommandState>,
) {
    shared
        .terminal_requests
        .schedule(Instant::now(), move |shared| {
            if shared.command_queue_cancelled(client) || shared.stopping.load(Ordering::Acquire) {
                state.resolve(Ok(Execution::default()));
                return;
            }
            let execution = {
                let inner = shared.inner.lock();
                let mut hooks = DaemonFormatHooks::command_with_optional_variables(
                    &facts,
                    (!variables.is_empty()).then_some(&variables),
                )
                .with_command_item("list-keys");
                inner.engine.step_key_listing(&mut listing, &mut hooks)
            };
            if let Some(execution) = execution {
                state.resolve(Ok(execution));
            } else {
                render_key_listing(shared, client, listing, facts, variables, state);
            }
        });
}

#[cfg(all(test, unix))]
#[path = "listing_e22_tests.rs"]
mod listing_e22_tests;
