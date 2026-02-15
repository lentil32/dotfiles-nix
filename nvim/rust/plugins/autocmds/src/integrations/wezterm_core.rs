use std::path::Path;
use std::process::ExitStatus;

use nvim_oxi_utils::state_machine::{NoEffect, Transition};
use support::{NonEmptyString, ProjectRoot, TabTitle};

#[derive(Debug)]
struct UpdateSlot<T> {
    applied: Option<T>,
    in_flight: Option<T>,
    queued: Option<T>,
}

impl<T> UpdateSlot<T> {
    const fn new() -> Self {
        Self {
            applied: None,
            in_flight: None,
            queued: None,
        }
    }

    fn clear_pending(&mut self) {
        self.in_flight = None;
        self.queued = None;
    }
}

impl<T> Default for UpdateSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> UpdateSlot<T>
where
    T: Clone + Eq,
{
    fn request(&mut self, target: T) -> Option<T> {
        if self.applied.as_ref() == Some(&target)
            || self.in_flight.as_ref() == Some(&target)
            || self.queued.as_ref() == Some(&target)
        {
            return None;
        }
        if self.in_flight.is_some() {
            self.queued = Some(target);
            return None;
        }
        self.in_flight = Some(target.clone());
        Some(target)
    }

    fn complete_success(&mut self, target: &T) -> Option<T> {
        if self.in_flight.as_ref() != Some(target) {
            return None;
        }
        self.applied = Some(target.clone());
        self.in_flight = None;
        self.start_queued()
    }

    fn complete_failure(&mut self, target: &T) -> Option<T> {
        if self.in_flight.as_ref() != Some(target) {
            return None;
        }
        self.in_flight = None;
        self.start_queued()
    }

    fn start_queued(&mut self) -> Option<T> {
        let queued = self.queued.take()?;
        if self.applied.as_ref() == Some(&queued) {
            return None;
        }
        self.in_flight = Some(queued.clone());
        Some(queued)
    }
}

fn path_basename(path: &Path) -> Option<NonEmptyString> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    NonEmptyString::try_new(name).ok()
}

fn tilde_path(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home {
        if path == home {
            return "~".to_string();
        }
        if let Ok(stripped) = path.strip_prefix(home) {
            let tail = stripped.to_string_lossy();
            if tail.is_empty() {
                return "~".to_string();
            }
            return format!("~{}{}", std::path::MAIN_SEPARATOR, tail);
        }
    }
    path.to_string_lossy().into_owned()
}

pub fn tab_title_for_root(root: &ProjectRoot, home: Option<&Path>) -> Option<TabTitle> {
    let path = root.as_path();
    path_basename(path)
        .map(TabTitle::from)
        .or_else(|| TabTitle::try_new(tilde_path(path, home)).ok())
}

pub fn derive_tab_title(root: Option<ProjectRoot>, home: Option<&Path>) -> Option<TabTitle> {
    root.and_then(|root| tab_title_for_root(&root, home))
}

pub fn format_cli_failure(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "wezterm cli failed with signal".to_string(),
        |code| format!("wezterm cli failed with exit code {code}"),
    )
}

pub fn format_set_working_dir_failure(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "wezterm set-working-directory failed with signal".to_string(),
        |code| format!("wezterm set-working-directory failed with exit code {code}"),
    )
}

#[derive(Debug, Default)]
struct CliWarningGate {
    unavailable_reported: bool,
    title_failed_reported: bool,
    cwd_failed_reported: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureChannel {
    Title,
    Cwd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeztermCompletion {
    Success,
    Failed,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeztermCommand {
    SetTabTitle(TabTitle),
    SetWorkingDir(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeztermEvent {
    RequestTitle {
        title: TabTitle,
    },
    RequestWorkingDir {
        cwd: String,
    },
    TitleCompleted {
        title: TabTitle,
        completion: WeztermCompletion,
    },
    WorkingDirCompleted {
        cwd: String,
        completion: WeztermCompletion,
    },
}

pub type WeztermTransition = Transition<NoEffect, WeztermCommand>;

impl CliWarningGate {
    const fn reset_after_success(&mut self, channel: FailureChannel) {
        self.unavailable_reported = false;
        match channel {
            FailureChannel::Title => self.title_failed_reported = false,
            FailureChannel::Cwd => self.cwd_failed_reported = false,
        }
    }

    const fn take_unavailable(&mut self) -> bool {
        if self.unavailable_reported {
            false
        } else {
            self.unavailable_reported = true;
            true
        }
    }

    const fn take_failed(&mut self, channel: FailureChannel) -> bool {
        let failed_reported = match channel {
            FailureChannel::Title => &mut self.title_failed_reported,
            FailureChannel::Cwd => &mut self.cwd_failed_reported,
        };
        if *failed_reported {
            false
        } else {
            *failed_reported = true;
            true
        }
    }
}

#[derive(Debug)]
pub struct WeztermState {
    title: UpdateSlot<TabTitle>,
    cwd: UpdateSlot<String>,
    warning_gate: CliWarningGate,
}

impl WeztermState {
    pub const fn new() -> Self {
        Self {
            title: UpdateSlot::new(),
            cwd: UpdateSlot::new(),
            warning_gate: CliWarningGate {
                unavailable_reported: false,
                title_failed_reported: false,
                cwd_failed_reported: false,
            },
        }
    }

    pub fn clear_pending_updates(&mut self) {
        self.title.clear_pending();
        self.cwd.clear_pending();
    }

    pub const fn take_warn_cli_unavailable(&mut self) -> bool {
        self.warning_gate.take_unavailable()
    }

    pub const fn take_warn_title_failed(&mut self) -> bool {
        self.warning_gate.take_failed(FailureChannel::Title)
    }

    pub const fn take_warn_cwd_failed(&mut self) -> bool {
        self.warning_gate.take_failed(FailureChannel::Cwd)
    }

    fn complete_title(
        &mut self,
        title: TabTitle,
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

impl Default for WeztermState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(value: &str) -> Result<TabTitle, &'static str> {
        TabTitle::try_new(value.to_string()).map_err(|_| "expected non-empty tab title")
    }

    #[test]
    fn derive_tab_title_prefers_basename() -> Result<(), &'static str> {
        let root = ProjectRoot::try_new("/tmp/root".to_string()).ok();
        let title = derive_tab_title(root, None).ok_or("expected tab title")?;
        assert_eq!(title.as_str(), "root");
        Ok(())
    }

    #[test]
    fn tilde_path_rewrites_home() {
        let home = Path::new("/tmp/home");
        let path = Path::new("/tmp/home/projects");
        let value = tilde_path(path, Some(home));
        assert_eq!(value, format!("~{}projects", std::path::MAIN_SEPARATOR));
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
