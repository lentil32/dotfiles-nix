use super::events::WeztermCommand;
use super::events::WeztermCompletion;
use super::events::WeztermEvent;
use super::events::WeztermTransition;
use super::state::FailureChannel;
use super::state::UpdateSlot;
use super::state::WeztermState;

impl WeztermState {
    fn transition_from_next<T>(
        next: Option<T>,
        make_command: impl FnOnce(T) -> WeztermCommand,
    ) -> WeztermTransition {
        next.map_or_else(WeztermTransition::default, |value| {
            WeztermTransition::with_command(make_command(value))
        })
    }

    fn request_slot<T>(
        &mut self,
        target: T,
        slot_access: impl FnOnce(&mut Self) -> &mut UpdateSlot<T>,
        make_command: impl FnOnce(T) -> WeztermCommand,
    ) -> WeztermTransition
    where
        T: Clone + Eq,
    {
        let next = {
            let slot = slot_access(self);
            slot.request(target)
        };
        Self::transition_from_next(next, make_command)
    }

    fn complete_slot<T>(
        &mut self,
        target: T,
        completion: WeztermCompletion,
        channel: FailureChannel,
        slot_access: impl FnOnce(&mut Self) -> &mut UpdateSlot<T>,
        make_command: impl FnOnce(T) -> WeztermCommand,
    ) -> WeztermTransition
    where
        T: Clone + Eq,
    {
        let (next, completed_successfully) = {
            let slot = slot_access(self);
            match completion {
                WeztermCompletion::Success => {
                    let completed_successfully = slot.in_flight.as_ref() == Some(&target);
                    (slot.complete_success(&target), completed_successfully)
                }
                WeztermCompletion::Failed | WeztermCompletion::Unavailable => {
                    (slot.complete_failure(&target), false)
                }
            }
        };

        if completed_successfully {
            self.warning_gate.reset_after_success(channel);
        }
        if completion == WeztermCompletion::Unavailable {
            self.clear_pending_updates();
        }

        Self::transition_from_next(next, make_command)
    }

    pub fn reduce(&mut self, event: WeztermEvent) -> WeztermTransition {
        match event {
            WeztermEvent::RequestTitle { title } => {
                self.request_slot(title, |state| &mut state.title, WeztermCommand::SetTabTitle)
            }
            WeztermEvent::RequestWorkingDir { cwd } => {
                self.request_slot(cwd, |state| &mut state.cwd, WeztermCommand::SetWorkingDir)
            }
            WeztermEvent::TitleCompleted { title, completion } => self.complete_slot(
                title,
                completion,
                FailureChannel::Title,
                |state| &mut state.title,
                WeztermCommand::SetTabTitle,
            ),
            WeztermEvent::WorkingDirCompleted { cwd, completion } => self.complete_slot(
                cwd,
                completion,
                FailureChannel::Cwd,
                |state| &mut state.cwd,
                WeztermCommand::SetWorkingDir,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvimrs_support::TabTitle;

    fn title(value: &str) -> Result<TabTitle, &'static str> {
        TabTitle::try_new(value.to_string()).map_err(|_| "expected non-empty tab title")
    }

    #[test]
    fn wezterm_state_deduplicates_cwd_updates() {
        let mut state = WeztermState::new();
        let tmp = "/tmp".to_string();
        let var = "/var".to_string();
        let tmp_in_flight = "/tmp".to_string();
        assert_eq!(
            state
                .reduce(WeztermEvent::RequestWorkingDir { cwd: tmp.clone() })
                .command,
            Some(WeztermCommand::SetWorkingDir(tmp.clone()))
        );
        assert!(
            state
                .reduce(WeztermEvent::RequestWorkingDir { cwd: tmp })
                .is_empty()
        );
        assert!(
            state
                .reduce(WeztermEvent::RequestWorkingDir { cwd: var.clone() })
                .is_empty()
        );
        assert_eq!(
            state
                .reduce(WeztermEvent::WorkingDirCompleted {
                    cwd: tmp_in_flight,
                    completion: WeztermCompletion::Success
                })
                .command,
            Some(WeztermCommand::SetWorkingDir(var.clone()))
        );
        assert!(
            state
                .reduce(WeztermEvent::WorkingDirCompleted {
                    cwd: var.clone(),
                    completion: WeztermCompletion::Success
                })
                .is_empty()
        );
        assert!(
            state
                .reduce(WeztermEvent::RequestWorkingDir { cwd: var })
                .is_empty()
        );
    }

    #[test]
    fn wezterm_state_title_success_advances_latest_queued_value() -> Result<(), &'static str> {
        let mut state = WeztermState::new();
        let a = title("a")?;
        let b = title("b")?;
        let c = title("c")?;

        assert_eq!(
            state
                .reduce(WeztermEvent::RequestTitle { title: a.clone() })
                .command,
            Some(WeztermCommand::SetTabTitle(a.clone()))
        );
        assert!(
            state
                .reduce(WeztermEvent::RequestTitle { title: b })
                .is_empty()
        );
        assert!(
            state
                .reduce(WeztermEvent::RequestTitle { title: c.clone() })
                .is_empty()
        );

        assert_eq!(
            state
                .reduce(WeztermEvent::TitleCompleted {
                    title: a,
                    completion: WeztermCompletion::Success
                })
                .command,
            Some(WeztermCommand::SetTabTitle(c.clone()))
        );
        assert!(
            state
                .reduce(WeztermEvent::TitleCompleted {
                    title: c.clone(),
                    completion: WeztermCompletion::Success
                })
                .is_empty()
        );
        assert!(
            state
                .reduce(WeztermEvent::RequestTitle { title: c })
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn wezterm_state_title_failure_still_advances_queued_value() -> Result<(), &'static str> {
        let mut state = WeztermState::new();
        let a = title("a")?;
        let b = title("b")?;

        assert_eq!(
            state
                .reduce(WeztermEvent::RequestTitle { title: a.clone() })
                .command,
            Some(WeztermCommand::SetTabTitle(a.clone()))
        );
        assert!(
            state
                .reduce(WeztermEvent::RequestTitle { title: b.clone() })
                .is_empty()
        );

        assert_eq!(
            state
                .reduce(WeztermEvent::TitleCompleted {
                    title: a,
                    completion: WeztermCompletion::Failed
                })
                .command,
            Some(WeztermCommand::SetTabTitle(b.clone()))
        );
        assert!(
            state
                .reduce(WeztermEvent::TitleCompleted {
                    title: b.clone(),
                    completion: WeztermCompletion::Success
                })
                .is_empty()
        );
        assert!(
            state
                .reduce(WeztermEvent::RequestTitle { title: b })
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn wezterm_warning_gate_resets_after_successful_completion() -> Result<(), &'static str> {
        let mut state = WeztermState::new();
        assert!(state.take_warn_cli_unavailable());
        assert!(!state.take_warn_cli_unavailable());
        assert!(state.take_warn_title_failed());
        assert!(!state.take_warn_title_failed());
        assert!(state.take_warn_cwd_failed());
        assert!(!state.take_warn_cwd_failed());

        let current = title("current")?;
        assert_eq!(
            state
                .reduce(WeztermEvent::RequestTitle {
                    title: current.clone()
                })
                .command,
            Some(WeztermCommand::SetTabTitle(current.clone()))
        );
        assert!(
            state
                .reduce(WeztermEvent::TitleCompleted {
                    title: current,
                    completion: WeztermCompletion::Success
                })
                .is_empty()
        );

        assert!(state.take_warn_cli_unavailable());
        assert!(state.take_warn_title_failed());
        assert!(!state.take_warn_cwd_failed());
        Ok(())
    }
}
