use std::path::{Path, PathBuf};

use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::{OptionOpts, OptionScope};
use nvim_oxi::{Result, String as NvimString};
use nvim_utils::path::{has_uri_scheme, normalize_path, path_is_dir, strip_known_prefixes};

const ROOT_VAR: &str = "project_root";
const ROOT_CACHE_VAR: &str = "project_root_cache";
const ROOT_CACHE_VERSION: &str = "v2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPathKey(String);

impl NormalizedPathKey {
    pub fn from_normalized_path(path: &Path) -> Self {
        Self(path.to_string_lossy().into_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RootCacheRecord {
    key: NormalizedPathKey,
    value: RootCacheValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RootCacheValue {
    Missing,
    Found { root: String, indicator: String },
}

impl RootCacheRecord {
    fn new(key: &NormalizedPathKey, root: Option<&str>, indicator: Option<&str>) -> Option<Self> {
        if key.as_str().is_empty() {
            return None;
        }
        let root = root
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let indicator = indicator
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let value = match (root, indicator) {
            (None, None) => RootCacheValue::Missing,
            (Some(root), Some(indicator)) => RootCacheValue::Found { root, indicator },
            _ => return None,
        };
        Some(Self {
            key: key.clone(),
            value,
        })
    }

    fn encode(&self) -> String {
        let (has_root, root, indicator) = match &self.value {
            RootCacheValue::Missing => ("0", "", ""),
            RootCacheValue::Found { root, indicator } => ("1", root.as_str(), indicator.as_str()),
        };
        format!(
            "{ROOT_CACHE_VERSION}\0{}\0{has_root}\0{root}\0{indicator}",
            self.key.as_str()
        )
    }

    fn decode(raw: &str) -> Option<Self> {
        let mut parts = raw.splitn(5, '\0');
        if parts.next()? != ROOT_CACHE_VERSION {
            return None;
        }
        let key = parts.next()?;
        if key.is_empty() {
            return None;
        }
        let has_root = parts.next()?;
        let root_part = parts.next().unwrap_or_default();
        let indicator_part = parts.next().unwrap_or_default();
        let value = match has_root {
            "0" if root_part.is_empty() && indicator_part.is_empty() => RootCacheValue::Missing,
            "1" if !root_part.is_empty() && !indicator_part.is_empty() => {
                RootCacheValue::Found {
                    root: root_part.to_owned(),
                    indicator: indicator_part.to_owned(),
                }
            }
            _ => return None,
        };
        Some(Self {
            key: NormalizedPathKey(key.to_owned()),
            value,
        })
    }
}

pub fn debug_log<F>(build: F)
where
    F: FnOnce() -> String,
{
    let _ = build;
}

fn get_buf_var(buf: &Buffer, var: &str) -> Option<String> {
    if !buf.is_valid() {
        return None;
    }
    buf.get_var::<NvimString>(var)
        .ok()
        .map(|val| val.to_string_lossy().into_owned())
}

fn get_buf_cache_record(buf: &Buffer) -> Option<RootCacheRecord> {
    let raw = get_buf_var(buf, ROOT_CACHE_VAR)?;
    RootCacheRecord::decode(&raw)
}

fn set_buf_var(buf: &mut Buffer, var: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => buf.set_var(var, value)?,
        None => buf.set_var(var, "")?,
    }
    Ok(())
}

pub fn set_buf_root(
    buf: &Buffer,
    root: Option<&str>,
    key: Option<&NormalizedPathKey>,
    indicator: Option<&str>,
) -> Result<()> {
    if !buf.is_valid() {
        return Ok(());
    }

    let root = root.filter(|value| !value.is_empty());
    let cache_record = key.and_then(|key| RootCacheRecord::new(key, root, indicator));
    let cache_payload = cache_record.map(|record| record.encode());
    let mut buf = buf.clone();
    set_buf_var(&mut buf, ROOT_VAR, root)?;
    set_buf_var(&mut buf, ROOT_CACHE_VAR, cache_payload.as_deref())?;
    Ok(())
}

pub fn get_path_from_buffer(buf: &Buffer) -> Result<Option<PathBuf>> {
    if !buf.is_valid() {
        debug_log(|| "get_path_from_buffer: buffer invalid".to_string());
        return Ok(None);
    }
    let name = buf.get_name()?;
    if name.is_empty() {
        debug_log(|| format!("get_path_from_buffer: buf={} empty name", buf.handle()));
        return Ok(None);
    }
    let raw_path = name.to_string_lossy().into_owned();
    debug_log(|| {
        format!(
            "get_path_from_buffer: buf={} name='{}'",
            buf.handle(),
            raw_path
        )
    });
    let stripped = strip_known_prefixes(&raw_path);
    if stripped != raw_path {
        debug_log(|| {
            format!(
                "get_path_from_buffer: buf={} stripped path='{}'",
                buf.handle(),
                stripped
            )
        });
    }

    let bt: NvimString = api::get_option_value(
        "buftype",
        &OptionOpts::builder()
            .scope(OptionScope::Local)
            .buf(buf.clone())
            .build(),
    )?;
    let allow_nonfile_buftype = raw_path.starts_with("file://") || raw_path.starts_with("oil://");
    if !bt.is_empty() && !allow_nonfile_buftype {
        debug_log(|| {
            format!(
                "get_path_from_buffer: buf={} buftype='{}' -> skip",
                buf.handle(),
                bt.to_string_lossy()
            )
        });
        return Ok(None);
    }

    if has_uri_scheme(&raw_path)
        && !raw_path.starts_with("file://")
        && !raw_path.starts_with("oil://")
    {
        debug_log(|| {
            format!(
                "get_path_from_buffer: buf={} uri scheme in '{}' -> skip",
                buf.handle(),
                raw_path
            )
        });
        return Ok(None);
    }

    let normalized = normalize_path(stripped);
    debug_log(|| {
        let value = normalized.as_ref().map_or_else(
            || "<none>".to_string(),
            |path| path.to_string_lossy().into_owned(),
        );
        format!(
            "get_path_from_buffer: buf={} normalized='{}'",
            buf.handle(),
            value
        )
    });
    Ok(normalized)
}

pub fn cached_root_by_key(buf: &Buffer, key: &NormalizedPathKey) -> Option<String> {
    let record = get_buf_cache_record(buf)?;
    if record.key != *key {
        return None;
    }
    let (root, indicator) = match record.value {
        RootCacheValue::Missing => return None,
        RootCacheValue::Found { root, indicator } => (root, indicator),
    };
    let root_path = Path::new(&root);
    if !path_is_dir(root_path) {
        return None;
    }
    if !root_path.join(indicator).exists() {
        return None;
    }
    Some(root)
}

/// Build the cache key for a path that has already been normalized.
pub fn normalized_path_key(path: &Path) -> NormalizedPathKey {
    NormalizedPathKey::from_normalized_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvim_utils::path::normalize_path;

    #[test]
    fn normalized_path_key_is_equal_for_equivalent_normalized_paths()
    -> std::result::Result<(), &'static str> {
        let left = normalize_path("/tmp/workspace/./repo").ok_or("expected normalized path")?;
        let right = normalize_path("/tmp/workspace/repo").ok_or("expected normalized path")?;
        assert_eq!(
            left, right,
            "test setup requires equivalent normalized paths"
        );
        assert_eq!(normalized_path_key(&left), normalized_path_key(&right));
        Ok(())
    }

    #[test]
    fn normalized_path_key_contract_requires_normalized_input()
    -> std::result::Result<(), &'static str> {
        let raw = "/tmp/workspace/../repo";
        let normalized = normalize_path(raw).ok_or("expected normalized path")?;
        let raw_key = normalized_path_key(Path::new(raw));
        let normalized_key = normalized_path_key(&normalized);
        assert_ne!(raw_key, normalized_key);
        Ok(())
    }

    #[test]
    fn root_cache_record_roundtrip_with_root() -> std::result::Result<(), &'static str> {
        let key = normalized_path_key(Path::new("/tmp/workspace"));
        let record = RootCacheRecord::new(&key, Some("/tmp"), Some(".git"))
            .ok_or("expected valid root cache record")?;
        let encoded = record.encode();
        let decoded = RootCacheRecord::decode(&encoded).ok_or("expected valid encoded payload")?;
        assert_eq!(decoded, record);
        Ok(())
    }

    #[test]
    fn root_cache_record_roundtrip_without_root() -> std::result::Result<(), &'static str> {
        let key = normalized_path_key(Path::new("/tmp/workspace"));
        let record =
            RootCacheRecord::new(&key, None, None).ok_or("expected valid root cache record")?;
        let encoded = record.encode();
        let decoded = RootCacheRecord::decode(&encoded).ok_or("expected valid encoded payload")?;
        assert_eq!(decoded, record);
        Ok(())
    }

    #[test]
    fn root_cache_record_decode_rejects_invalid_payload() {
        assert_eq!(RootCacheRecord::decode(""), None);
        assert_eq!(
            RootCacheRecord::decode(concat!("v1\0k\0", "1\0/root")),
            None
        );
        assert_eq!(
            RootCacheRecord::decode(concat!("v2\0\0", "1\0/root\0.git")),
            None
        );
        assert_eq!(
            RootCacheRecord::decode(concat!("v2\0k\0", "1\0\0.git")),
            None
        );
        assert_eq!(
            RootCacheRecord::decode(concat!("v2\0k\0", "0\0/root\0")),
            None
        );
        assert_eq!(
            RootCacheRecord::decode(concat!("v2\0k\0", "1\0/root\0")),
            None
        );
        assert_eq!(
            RootCacheRecord::decode(concat!("v2\0k\0", "0\0\0.git")),
            None
        );
        assert_eq!(RootCacheRecord::decode("v2\0k\0x\0/root\0.git"), None);
    }

    #[test]
    fn root_cache_record_requires_indicator_with_root() {
        let key = normalized_path_key(Path::new("/tmp/workspace"));
        assert_eq!(RootCacheRecord::new(&key, Some("/tmp"), None), None);
        assert_eq!(RootCacheRecord::new(&key, None, Some(".git")), None);
    }
}
