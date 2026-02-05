use std::path::Path;
use std::process::ExitStatus;

use support::{NonEmptyString, ProjectRoot, TabTitle};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAvailability {
    Enabled,
    Disabled,
}

impl CliAvailability {
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Debug)]
pub struct WeztermTabState {
    last_title: Option<TabTitle>,
    cli_availability: CliAvailability,
    warned_cli_unavailable: bool,
    warned_cli_failed: bool,
}

impl WeztermTabState {
    pub const fn new() -> Self {
        Self {
            last_title: None,
            cli_availability: CliAvailability::Enabled,
            warned_cli_unavailable: false,
            warned_cli_failed: false,
        }
    }

    pub const fn cli_enabled(&self) -> bool {
        self.cli_availability.is_enabled()
    }

    pub const fn disable_cli(&mut self) {
        self.cli_availability = CliAvailability::Disabled;
    }

    pub const fn take_warn_cli_unavailable(&mut self) -> bool {
        if self.warned_cli_unavailable {
            false
        } else {
            self.warned_cli_unavailable = true;
            true
        }
    }

    pub const fn take_warn_cli_failed(&mut self) -> bool {
        if self.warned_cli_failed {
            false
        } else {
            self.warned_cli_failed = true;
            true
        }
    }

    pub fn should_update(&self, title: &TabTitle) -> bool {
        self.last_title.as_ref() != Some(title)
    }

    pub fn record_title(&mut self, title: TabTitle) {
        self.last_title = Some(title);
    }
}

impl Default for WeztermTabState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
