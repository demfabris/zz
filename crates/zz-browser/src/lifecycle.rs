#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RuntimePhase {
    #[default]
    Uninitialized,
    Initializing,
    Running,
    Closing,
    Closed,
    Failed,
}

impl RuntimePhase {
    #[must_use]
    pub const fn may_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Uninitialized, Self::Initializing | Self::Closed)
                | (
                    Self::Initializing,
                    Self::Running | Self::Failed | Self::Closing
                )
                | (Self::Running, Self::Closing)
                | (Self::Closing, Self::Closed)
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionPhase {
    #[default]
    Creating,
    Ready,
    Crashed,
    Closing,
    Closed,
}

impl SessionPhase {
    #[must_use]
    pub const fn may_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Creating, Self::Ready | Self::Closing)
                | (Self::Ready, Self::Crashed | Self::Closing)
                | (Self::Crashed, Self::Closing)
                | (Self::Closing, Self::Closed)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_transitions_are_strict() {
        assert!(RuntimePhase::Uninitialized.may_transition_to(RuntimePhase::Initializing));
        assert!(RuntimePhase::Uninitialized.may_transition_to(RuntimePhase::Closed));
        assert!(!RuntimePhase::Uninitialized.may_transition_to(RuntimePhase::Running));
        assert!(RuntimePhase::Initializing.may_transition_to(RuntimePhase::Running));
        assert!(RuntimePhase::Initializing.may_transition_to(RuntimePhase::Failed));
        assert!(!RuntimePhase::Failed.may_transition_to(RuntimePhase::Running));
        assert!(!RuntimePhase::Failed.may_transition_to(RuntimePhase::Initializing));
        assert!(!RuntimePhase::Closed.may_transition_to(RuntimePhase::Running));
    }

    #[test]
    fn crashed_session_must_close_before_replacement() {
        assert!(SessionPhase::Ready.may_transition_to(SessionPhase::Crashed));
        assert!(SessionPhase::Crashed.may_transition_to(SessionPhase::Closing));
        assert!(!SessionPhase::Crashed.may_transition_to(SessionPhase::Creating));
    }
}
