use std::path::{Path, PathBuf};

use nonempty::NonEmpty;

pub type RootIndicators = NonEmpty<String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootMatch {
    pub root: PathBuf,
    pub indicator: String,
}

pub fn default_root_indicators() -> RootIndicators {
    RootIndicators::from((
        ".git".to_string(),
        vec![
            "package.json".to_string(),
            "Cargo.toml".to_string(),
            "flake.nix".to_string(),
            "Makefile".to_string(),
        ],
    ))
}

pub fn root_indicators_from_vec(values: Vec<String>) -> Option<RootIndicators> {
    RootIndicators::from_vec(values)
}

pub fn root_match_from_path_with<F, G>(
    path: &Path,
    indicators: &RootIndicators,
    is_dir: F,
    exists: G,
) -> Option<RootMatch>
where
    F: Fn(&Path) -> bool,
    G: Fn(&Path) -> bool,
{
    let mut current = path.to_path_buf();
    if !is_dir(&current) {
        current = current.parent()?.to_path_buf();
    }

    loop {
        for indicator in indicators {
            let candidate = current.join(indicator);
            if exists(&candidate) {
                return Some(RootMatch {
                    root: current.clone(),
                    indicator: indicator.clone(),
                });
            }
        }
        if !current.pop() {
            break;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn root_from_path_with_finds_nearest_indicator() {
        let indicators = RootIndicators::new("root".to_string());
        let mut existing = HashSet::new();
        existing.insert(PathBuf::from("/a/root"));
        let root = root_match_from_path_with(
            Path::new("/a/b/c"),
            &indicators,
            |_| true,
            |candidate| existing.contains(candidate),
        )
        .map(|value| value.root);
        assert_eq!(root, Some(PathBuf::from("/a")));
    }

    #[test]
    fn root_from_path_with_none_when_missing() {
        let indicators = RootIndicators::new("root".to_string());
        let root = root_match_from_path_with(
            Path::new("/a/b/c"),
            &indicators,
            |_| true,
            |_candidate| false,
        )
        .map(|value| value.root);
        assert_eq!(root, None);
    }

    #[test]
    fn root_match_from_path_with_reports_indicator() -> Result<(), &'static str> {
        let indicators = RootIndicators::from(("x".to_string(), vec!["root".to_string()]));
        let mut existing = HashSet::new();
        existing.insert(PathBuf::from("/a/root"));
        let found = root_match_from_path_with(
            Path::new("/a/b/c"),
            &indicators,
            |_| true,
            |candidate| existing.contains(candidate),
        );
        let found = found.ok_or("expected root match")?;
        assert_eq!(found.root, PathBuf::from("/a"));
        assert_eq!(found.indicator, "root");
        Ok(())
    }
}
