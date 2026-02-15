use super::events::{WeztermCommand, WeztermCompletion, WeztermEvent, WeztermTransition};
use super::state::{FailureChannel, WeztermState};

impl WeztermState {
    fn complete_title(
        &mut self,
        title: support::TabTitle,
        completion: WeztermCompletion,
    ) -> WeztermTransition {
        let next_title = match completion {
            WeztermCompletion::Success => {
                let completed = self.title.in_flight.as_ref() == Some(&title);
                let next = self.title.complete_success(&title);
                if completed {
                    self.warning_gate.reset_after_success(FailureChannel::Title);
                }
                next
            }
            WeztermCompletion::Failed | WeztermCompletion::Unavailable => {
                self.title.complete_failure(&title)
            }
        };
        if completion == WeztermCompletion::Unavailable {
            self.clear_pending_updates();
        }
        next_title.map_or_else(WeztermTransition::default, |next| {
            WeztermTransition::with_command(WeztermCommand::SetTabTitle(next))
        })
    }

    fn complete_working_dir(
        &mut self,
        cwd: String,
        completion: WeztermCompletion,
    ) -> WeztermTransition {
        let next_cwd = match completion {
            WeztermCompletion::Success => {
                let completed = self.cwd.in_flight.as_ref() == Some(&cwd);
                let next = self.cwd.complete_success(&cwd);
                if completed {
                    self.warning_gate.reset_after_success(FailureChannel::Cwd);
                }
                next
            }
            WeztermCompletion::Failed | WeztermCompletion::Unavailable => {
                self.cwd.complete_failure(&cwd)
            }
        };
        if completion == WeztermCompletion::Unavailable {
            self.clear_pending_updates();
        }
        next_cwd.map_or_else(WeztermTransition::default, |next| {
            WeztermTransition::with_command(WeztermCommand::SetWorkingDir(next))
        })
    }

    pub fn reduce(&mut self, event: WeztermEvent) -> WeztermTransition {
        match event {
            WeztermEvent::RequestTitle { title } => self
                .title
                .request(title)
                .map_or_else(WeztermTransition::default, |next| {
                    WeztermTransition::with_command(WeztermCommand::SetTabTitle(next))
                }),
            WeztermEvent::RequestWorkingDir { cwd } => self
                .cwd
                .request(cwd)
                .map_or_else(WeztermTransition::default, |next| {
                    WeztermTransition::with_command(WeztermCommand::SetWorkingDir(next))
                }),
            WeztermEvent::TitleCompleted { title, completion } => {
                self.complete_title(title, completion)
            }
            WeztermEvent::WorkingDirCompleted { cwd, completion } => {
                self.complete_working_dir(cwd, completion)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use support::TabTitle;

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
