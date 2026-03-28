use crate::models::run::RunState;

#[derive(Debug, Clone)]
pub enum ExecutorEvent {
    Started {
        pid: u32,
    },
    Succeeded {
        exit_code: i32,
        duration_ms: i64,
    },
    Failed {
        exit_code: Option<i32>,
        reason: String,
    },
    TimedOut,
    Cancelled,
}

/// Pure function — all state transitions live here.
/// Returns the next state or an error if the transition is invalid.
pub fn transition(current: &RunState, event: &ExecutorEvent) -> Result<RunState, String> {
    match (current, event) {
        (RunState::Pending, ExecutorEvent::Started { .. }) => Ok(RunState::Running),
        (RunState::Queued, ExecutorEvent::Started { .. }) => Ok(RunState::Running),
        (RunState::Running, ExecutorEvent::Succeeded { .. }) => Ok(RunState::Success),
        (RunState::Running, ExecutorEvent::Failed { .. }) => Ok(RunState::Failure),
        (RunState::Running, ExecutorEvent::TimedOut) => Ok(RunState::TimedOut),
        (RunState::Running, ExecutorEvent::Cancelled) => Ok(RunState::Cancelled),
        (RunState::Pending, ExecutorEvent::Cancelled) => Ok(RunState::Cancelled),
        (RunState::Queued, ExecutorEvent::Cancelled) => Ok(RunState::Cancelled),
        (current, event) => Err(format!(
            "invalid transition: {:?} cannot accept {:?}",
            current, event
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_to_running() {
        let next = transition(&RunState::Pending, &ExecutorEvent::Started { pid: 123 });
        assert_eq!(next.unwrap(), RunState::Running);
    }

    #[test]
    fn running_to_success() {
        let next = transition(
            &RunState::Running,
            &ExecutorEvent::Succeeded {
                exit_code: 0,
                duration_ms: 1000,
            },
        );
        assert_eq!(next.unwrap(), RunState::Success);
    }

    #[test]
    fn invalid_transition_rejected() {
        let result = transition(&RunState::Success, &ExecutorEvent::Started { pid: 1 });
        assert!(result.is_err());
    }
}
