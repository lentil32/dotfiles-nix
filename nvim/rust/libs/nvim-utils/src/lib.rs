use std::fs;
use std::path::{Component, Path, PathBuf};

pub mod path {
    use super::{Component, Path, PathBuf, fs};

    const KNOWN_PREFIXES: [&str; 2] = ["oil://", "file://"];

    pub fn strip_known_prefixes(mut path: &str) -> &str {
        for prefix in KNOWN_PREFIXES {
            if let Some(stripped) = path.strip_prefix(prefix) {
                path = stripped;
            }
        }
        path
    }

    pub fn normalize_path(path: &str) -> Option<PathBuf> {
        let path = strip_known_prefixes(path);
        if path.is_empty() {
            return None;
        }
        let normalized = Path::new(path)
            .components()
            .fold(PathBuf::new(), |mut acc, component| {
                match component {
                    Component::CurDir => {}
                    Component::ParentDir => {
                        acc.pop();
                    }
                    Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                        acc.push(component.as_os_str());
                    }
                }
                acc
            });
        Some(normalized)
    }

    pub fn path_is_dir(path: &Path) -> bool {
        fs::metadata(path).is_ok_and(|meta| meta.is_dir())
    }

    pub fn has_uri_scheme(value: &str) -> bool {
        let mut chars = value.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        if !first.is_ascii_alphabetic() {
            return false;
        }
        for ch in chars {
            if ch == ':' {
                return value.contains("://");
            }
            if !(ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.') {
                return false;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::path::{has_uri_scheme, normalize_path, strip_known_prefixes};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use rstest::rstest;
    use std::path::Component;

    #[rstest]
    #[case("oil://foo", "foo")]
    #[case("file://bar", "bar")]
    #[case("oil://file://baz", "baz")]
    #[case("plain", "plain")]
    #[case("file://oil://path", "oil://path")]
    fn strip_known_prefixes_cases(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(strip_known_prefixes(input), expected);
    }

    #[rstest]
    #[case("http://example.com", true)]
    #[case("git+ssh://host", true)]
    #[case("file://path", true)]
    #[case("oil://path", true)]
    #[case("abc:def", false)]
    #[case("C:\\\\path", false)]
    #[case("1://bad", false)]
    #[case("", false)]
    fn has_uri_scheme_cases(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(has_uri_scheme(input), expected);
    }

    #[test]
    fn normalize_path_empty_is_none() {
        assert_eq!(normalize_path(""), None);
        assert_eq!(normalize_path("oil://"), None);
        assert_eq!(normalize_path("file://"), None);
    }

    fn segment_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(".".to_string()),
            Just("..".to_string()),
            "[a-z]{1,8}".prop_map(|s| s),
        ]
    }

    proptest! {
        #[test]
        fn normalize_path_drops_dot_segments(
            is_abs in any::<bool>(),
            segments in prop::collection::vec(segment_strategy(), 0..8),
        ) {
            let mut path = segments.join("/");
            if is_abs {
                path = format!("/{path}");
            }
            let normalized = normalize_path(&path);
            if let Some(normalized) = normalized {
                for comp in normalized.components() {
                    prop_assert!(
                        !matches!(comp, Component::CurDir | Component::ParentDir)
                    );
                }
            }
        }
    }
}
