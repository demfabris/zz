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
    #[cfg(any(test, windows))]
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
        #[cfg(any(test, windows))]
        self.completion.changed.notify_all();
        drop(ready);
        if let Some(owner) = self.owner.as_ref().and_then(Weak::upgrade) {
            owner.notify_one();
        }
        true
    }

    pub(super) fn ready(&self) -> bool {
        *self.completion.ready.lock()
    }

    #[cfg(any(test, windows))]
    pub(super) fn wait(&self) {
        let mut ready = self.completion.ready.lock();
        while !*ready {
            self.completion.changed.wait(&mut ready);
        }
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
