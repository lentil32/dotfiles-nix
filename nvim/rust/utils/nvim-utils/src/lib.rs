use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

pub mod mode;

pub mod path {
    use super::Component;
    use super::Path;
    use super::PathBuf;
    use super::fs;

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

    pub fn split_uri_scheme_and_rest(value: &str) -> Option<(&str, &str)> {
        let (scheme, rest) = value.split_once("://")?;
        let mut chars = scheme.chars();
        let first = chars.next()?;
        if !first.is_ascii_alphabetic() {
            return None;
        }
        if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')) {
            return None;
        }
        Some((scheme, rest))
    }

    pub fn has_uri_scheme(value: &str) -> bool {
        split_uri_scheme_and_rest(value).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::path::has_uri_scheme;
    use super::path::normalize_path;
    use super::path::split_uri_scheme_and_rest;
    use super::path::strip_known_prefixes;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::path::Component;

    #[test]
    fn strip_known_prefixes_cases() {
        let cases = [
            ("oil://foo", "foo"),
            ("file://bar", "bar"),
            ("oil://file://baz", "baz"),
            ("plain", "plain"),
            ("file://oil://path", "oil://path"),
        ];

        for (input, expected) in cases {
            assert_eq!(strip_known_prefixes(input), expected);
        }
    }

    #[test]
    fn has_uri_scheme_cases() {
        let cases = [
            ("http://example.com", true),
            ("git+ssh://host", true),
            ("file://path", true),
            ("oil://path", true),
            ("abc:def", false),
            ("C:\\\\path", false),
            ("1://bad", false),
            ("", false),
        ];

        for (input, expected) in cases {
            assert_eq!(has_uri_scheme(input), expected);
        }
    }

    #[test]
    fn split_uri_scheme_and_rest_cases() {
        let cases = [
            ("http://example.com", Some(("http", "example.com"))),
            ("git+ssh://host/repo", Some(("git+ssh", "host/repo"))),
            ("file://path", Some(("file", "path"))),
            ("C:\\\\path", None),
            ("1://bad", None),
            ("://bad", None),
            ("", None),
        ];

        for (input, expected) in cases {
            assert_eq!(split_uri_scheme_and_rest(input), expected);
        }
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
