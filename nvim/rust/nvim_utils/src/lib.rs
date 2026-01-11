use std::fs;
use std::path::{Component, Path, PathBuf};

pub mod path {
    use super::*;

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
        fs::metadata(path)
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
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
