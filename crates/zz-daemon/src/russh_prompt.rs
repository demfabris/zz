use std::time::Duration;

use tokio::{
    sync::watch,
    time::{Instant, sleep_until},
};

use crate::askpass::{AskpassPrompt, AskpassReply, SshPrompts};

pub(crate) struct HandshakeDeadline {
    deadline: watch::Sender<Option<Instant>>,
}

impl HandshakeDeadline {
    pub(crate) fn new(timeout: Duration) -> Self {
        let (deadline, _) = watch::channel(Some(Instant::now() + timeout));
        Self { deadline }
    }

    pub(crate) fn prompts(&self, prompts: SshPrompts) -> AsyncSshPrompts {
        AsyncSshPrompts {
            prompts,
            deadline: self.deadline.clone(),
        }
    }

    pub(crate) async fn expired(&self) {
        let mut changes = self.deadline.subscribe();
        loop {
            let deadline = *changes.borrow_and_update();
            match deadline {
                Some(deadline) => {
                    tokio::select! {
                        biased;
                        _ = changes.changed() => {},
                        () = sleep_until(deadline) => {
                            if self.deadline.borrow().is_some_and(|deadline| deadline <= Instant::now()) {
                                return;
                            }
                        },
                    }
                }
                None => {
                    let _ = changes.changed().await;
                }
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct AsyncSshPrompts {
    prompts: SshPrompts,
    deadline: watch::Sender<Option<Instant>>,
}

impl AsyncSshPrompts {
    pub(crate) async fn respond(&self, prompt: AskpassPrompt) -> AskpassReply {
        let remaining = self
            .deadline
            .send_replace(None)
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let prompts = self.prompts.clone();
        let reply = tokio::task::spawn_blocking(move || prompts.respond(&prompt))
            .await
            .unwrap_or(AskpassReply::Cancel);
        self.deadline
            .send_replace(remaining.map(|remaining| Instant::now() + remaining));
        reply
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, mpsc};

    use tokio::{sync::oneshot, time::advance};

    use super::*;
    use crate::askpass::AskpassMode;

    #[tokio::test(start_paused = true)]
    async fn slow_prompts_keep_runtime_running_and_preserve_network_budget() {
        let deadline = Arc::new(HandshakeDeadline::new(Duration::from_secs(60)));
        let timer = {
            let deadline = Arc::clone(&deadline);
            tokio::spawn(async move { deadline.expired().await })
        };
        for (network_seconds, prompt_seconds) in [(10, 90), (15, 80)] {
            advance(Duration::from_secs(network_seconds)).await;
            let (entered_tx, entered_rx) = oneshot::channel();
            let entered_tx = Mutex::new(Some(entered_tx));
            let (answer_tx, answer_rx) = mpsc::channel();
            let answer_rx = Mutex::new(answer_rx);
            let prompts = deadline.prompts(SshPrompts::new(Default::default(), move |_| {
                entered_tx.lock().unwrap().take().unwrap().send(()).unwrap();
                answer_rx.lock().unwrap().recv().unwrap()
            }));
            let answer = tokio::spawn(async move {
                prompts
                    .respond(AskpassPrompt::new(AskpassMode::Answer, "Password:"))
                    .await
            });
            entered_rx.await.unwrap();

            advance(Duration::from_secs(prompt_seconds)).await;
            assert!(!timer.is_finished());
            answer_tx.send(AskpassReply::answer("test reply")).unwrap();
            assert!(matches!(answer.await.unwrap(), AskpassReply::Answer(_)));
        }

        advance(Duration::from_secs(34)).await;
        assert!(!timer.is_finished());
        advance(Duration::from_secs(1)).await;
        timer.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn stalled_handshake_without_prompts_still_times_out() {
        let deadline = HandshakeDeadline::new(Duration::from_secs(60));
        let started = Instant::now();
        deadline.expired().await;
        assert_eq!(started.elapsed(), Duration::from_secs(60));
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_prompt_resumes_the_deadline() {
        let deadline = HandshakeDeadline::new(Duration::from_secs(60));
        let prompts = deadline.prompts(SshPrompts::new(Default::default(), |_| {
            AskpassReply::Cancel
        }));
        assert!(matches!(
            prompts
                .respond(AskpassPrompt::new(AskpassMode::Answer, "Trust this host?"))
                .await,
            AskpassReply::Cancel
        ));
        let resumed = Instant::now();
        deadline.expired().await;
        assert_eq!(resumed.elapsed(), Duration::from_secs(60));
    }
}
