use crate::events::lru_cache::LruCache;
use crate::lua::i64_from_object;
use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::OptionOpts;
use std::sync::Arc;

const BUFFER_METADATA_CACHE_CAPACITY: usize = 32;

#[derive(Debug, Clone, Eq, PartialEq)]
struct BufferMetadataCacheEntry {
    changedtick: u64,
    metadata: BufferMetadata,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::events) struct BufferMetadataCache {
    entries: LruCache<i64, BufferMetadataCacheEntry>,
}

impl Default for BufferMetadataCache {
    fn default() -> Self {
        Self {
            entries: LruCache::new(BUFFER_METADATA_CACHE_CAPACITY),
        }
    }
}

impl BufferMetadataCache {
    pub(in crate::events) fn read(&mut self, buffer: &api::Buffer) -> Result<BufferMetadata> {
        let buffer_handle = i64::from(buffer.handle());
        let changedtick = current_buffer_changedtick(buffer_handle)?;

        match self.entries.get_cloned(&buffer_handle) {
            Some(entry) if entry.changedtick == changedtick => Ok(entry.metadata),
            Some(entry) => {
                let metadata = entry.metadata.with_line_count(buffer.line_count()?);
                self.store(buffer_handle, changedtick, metadata.clone());
                Ok(metadata)
            }
            None => {
                let metadata = BufferMetadata::read_uncached(buffer)?;
                self.store(buffer_handle, changedtick, metadata.clone());
                Ok(metadata)
            }
        }
    }

    pub(in crate::events) fn invalidate_buffer(&mut self, buffer_handle: i64) {
        let _ = self.entries.remove(&buffer_handle);
    }

    pub(in crate::events) fn clear(&mut self) {
        self.entries.clear();
    }

    fn store(&mut self, buffer_handle: i64, changedtick: u64, metadata: BufferMetadata) {
        self.entries.insert(
            buffer_handle,
            BufferMetadataCacheEntry {
                changedtick,
                metadata,
            },
        );
    }

    #[cfg(test)]
    fn store_for_test(&mut self, buffer_handle: i64, changedtick: u64, metadata: BufferMetadata) {
        self.store(buffer_handle, changedtick, metadata);
    }

    #[cfg(test)]
    fn cached_entry_for_test(&self, buffer_handle: i64) -> Option<(u64, BufferMetadata)> {
        self.entries
            .peek_cloned(&buffer_handle)
            .map(|entry| (entry.changedtick, entry.metadata))
    }

    #[cfg(test)]
    fn cached_for_changedtick_for_test(
        &mut self,
        buffer_handle: i64,
        changedtick: u64,
    ) -> Option<BufferMetadata> {
        self.entries
            .get_cloned(&buffer_handle)
            .and_then(|entry| (entry.changedtick == changedtick).then_some(entry.metadata))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BufferMetadata {
    filetype: Arc<str>,
    buftype: Arc<str>,
    buflisted: bool,
    line_count: usize,
}

impl BufferMetadata {
    fn read_uncached(buffer: &api::Buffer) -> Result<Self> {
        crate::events::runtime::record_buffer_metadata_read();
        let opts = OptionOpts::builder().buf(buffer.clone()).build();
        let filetype: String = api::get_option_value("filetype", &opts)?;
        let buftype: String = api::get_option_value("buftype", &opts)?;
        let buflisted: bool = api::get_option_value("buflisted", &opts)?;

        Ok(Self {
            filetype: Arc::from(filetype),
            buftype: Arc::from(buftype),
            buflisted,
            line_count: buffer.line_count()?,
        })
    }

    fn with_line_count(mut self, line_count: usize) -> Self {
        self.line_count = line_count;
        self
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        filetype: &str,
        buftype: &str,
        buflisted: bool,
        line_count: usize,
    ) -> Self {
        Self {
            filetype: Arc::from(filetype),
            buftype: Arc::from(buftype),
            buflisted,
            line_count,
        }
    }

    pub(crate) fn filetype(&self) -> &str {
        self.filetype.as_ref()
    }

    pub(crate) fn buftype(&self) -> &str {
        self.buftype.as_ref()
    }

    pub(crate) const fn buflisted(&self) -> bool {
        self.buflisted
    }

    pub(crate) const fn line_count(&self) -> usize {
        self.line_count
    }
}

fn current_buffer_changedtick(buffer_handle: i64) -> Result<u64> {
    crate::events::runtime::record_current_buffer_changedtick_read();
    let args = Array::from_iter([Object::from(buffer_handle), Object::from("changedtick")]);
    let value = api::call_function("getbufvar", args)?;
    let changedtick = i64_from_object("getbufvar(changedtick)", value)?;
    if changedtick < 0 {
        return Err(
            nvim_oxi::api::Error::Other("buffer changedtick must be non-negative".into()).into(),
        );
    }

    Ok(changedtick as u64)
}

#[cfg(test)]
mod tests {
    use super::BufferMetadata;
    use super::BufferMetadataCache;
    use pretty_assertions::assert_eq;

    #[test]
    fn cache_hit_requires_matching_buffer_changedtick() {
        let mut cache = BufferMetadataCache::default();
        let metadata = BufferMetadata::new_for_test("lua", "", true, 42);
        cache.store_for_test(11, 7, metadata.clone());

        assert_eq!(
            cache.cached_for_changedtick_for_test(11, 7),
            Some(metadata.clone())
        );
        assert_eq!(cache.cached_for_changedtick_for_test(11, 8), None);
        assert_eq!(
            cache.cached_entry_for_test(11),
            Some((7, metadata)),
            "changedtick mismatches should not evict the cached cold metadata"
        );
    }

    #[test]
    fn invalidation_removes_only_the_target_buffer() {
        let mut cache = BufferMetadataCache::default();
        let lua = BufferMetadata::new_for_test("lua", "", true, 10);
        let rust = BufferMetadata::new_for_test("rust", "terminal", false, 99);
        cache.store_for_test(1, 3, lua);
        cache.store_for_test(2, 5, rust.clone());

        cache.invalidate_buffer(1);

        assert_eq!(cache.cached_entry_for_test(1), None);
        assert_eq!(cache.cached_entry_for_test(2), Some((5, rust)));
    }
}
