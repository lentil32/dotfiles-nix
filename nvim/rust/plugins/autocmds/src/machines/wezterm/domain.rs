use std::path::Path;
use std::process::ExitStatus;

use support::{NonEmptyString, ProjectRoot, TabTitle};

fn path_basename(path: &Path) -> Option<NonEmptyString> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    NonEmptyString::try_new(name).ok()
}

pub(super) fn tilde_path(path: &Path, home: Option<&Path>) -> String {
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

fn tab_title_for_root(root: &ProjectRoot, home: Option<&Path>) -> Option<TabTitle> {
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
