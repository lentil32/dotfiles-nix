use crate::events::lru_cache::LruCache;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::OptionOpts;
use std::sync::Arc;

const BUFFER_METADATA_CACHE_CAPACITY: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::events) struct BufferMetadataCache {
    entries: LruCache<i64, BufferMetadata>,
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
        match self.entries.get_cloned(&buffer_handle) {
            Some(metadata) => Ok(metadata),
            None => {
                let metadata = BufferMetadata::read_uncached(buffer)?;
                self.store(buffer_handle, metadata.clone());
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

    fn store(&mut self, buffer_handle: i64, metadata: BufferMetadata) {
        self.entries.insert(buffer_handle, metadata);
    }

    #[cfg(test)]
    pub(in crate::events) fn store_for_test(
        &mut self,
        buffer_handle: i64,
        metadata: BufferMetadata,
    ) {
        self.store(buffer_handle, metadata);
    }

    #[cfg(test)]
    pub(in crate::events) fn cached_entry_for_test(
        &self,
        buffer_handle: i64,
    ) -> Option<BufferMetadata> {
        self.entries.peek_cloned(&buffer_handle)
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
#[cfg(test)]
mod tests {
    use super::BufferMetadata;
    use super::BufferMetadataCache;
    use pretty_assertions::assert_eq;

    #[test]
    fn cache_hit_reuses_metadata_until_explicit_invalidation() {
        let mut cache = BufferMetadataCache::default();
        let metadata = BufferMetadata::new_for_test("lua", "", true, 42);
        cache.store_for_test(11, metadata.clone());

        assert_eq!(cache.cached_entry_for_test(11), Some(metadata));
    }

    #[test]
    fn invalidation_removes_only_the_target_buffer() {
        let mut cache = BufferMetadataCache::default();
        let lua = BufferMetadata::new_for_test("lua", "", true, 10);
        let rust = BufferMetadata::new_for_test("rust", "terminal", false, 99);
        cache.store_for_test(1, lua);
        cache.store_for_test(2, rust.clone());

        cache.invalidate_buffer(1);

        assert_eq!(cache.cached_entry_for_test(1), None);
        assert_eq!(cache.cached_entry_for_test(2), Some(rust));
    }
}
