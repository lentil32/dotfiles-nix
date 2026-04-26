use crate::events::lru_cache::LruCache;
use crate::host::BufferHandle;
use crate::host::BufferMetadataPort;
use crate::host::BufferMetadataSnapshot;
use crate::host::api;
use nvim_oxi::Result;
use std::sync::Arc;

const BUFFER_METADATA_CACHE_CAPACITY: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::events) struct BufferMetadataCache {
    entries: LruCache<BufferHandle, BufferMetadata>,
}

impl Default for BufferMetadataCache {
    fn default() -> Self {
        Self {
            entries: LruCache::new(BUFFER_METADATA_CACHE_CAPACITY),
        }
    }
}

impl BufferMetadataCache {
    pub(in crate::events) fn read(
        &mut self,
        host: &impl BufferMetadataPort,
        buffer: &api::Buffer,
    ) -> Result<BufferMetadata> {
        let buffer_handle = BufferHandle::from_buffer(buffer);
        match self.entries.get_cloned(&buffer_handle) {
            Some(metadata) => Ok(metadata),
            None => {
                let metadata = BufferMetadata::read_uncached(host, buffer)?;
                self.store(buffer_handle, metadata.clone());
                Ok(metadata)
            }
        }
    }

    pub(in crate::events) fn invalidate_buffer(&mut self, buffer_handle: impl Into<BufferHandle>) {
        let buffer_handle = buffer_handle.into();
        let _ = self.entries.remove(&buffer_handle);
    }

    pub(in crate::events) fn clear(&mut self) {
        self.entries.clear();
    }

    fn store(&mut self, buffer_handle: impl Into<BufferHandle>, metadata: BufferMetadata) {
        let buffer_handle = buffer_handle.into();
        self.entries.insert(buffer_handle, metadata);
    }

    #[cfg(test)]
    pub(in crate::events) fn store_for_test(
        &mut self,
        buffer_handle: impl Into<BufferHandle>,
        metadata: BufferMetadata,
    ) {
        self.store(buffer_handle, metadata);
    }

    #[cfg(test)]
    pub(in crate::events) fn cached_entry_for_test(
        &self,
        buffer_handle: impl Into<BufferHandle>,
    ) -> Option<BufferMetadata> {
        let buffer_handle = buffer_handle.into();
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
    fn read_uncached(host: &impl BufferMetadataPort, buffer: &api::Buffer) -> Result<Self> {
        crate::events::runtime::record_buffer_metadata_read();
        Ok(Self::from_host_snapshot(host.buffer_metadata(buffer)?))
    }

    fn from_host_snapshot(snapshot: BufferMetadataSnapshot) -> Self {
        let (filetype, buftype, buflisted, line_count) = snapshot.into_parts();
        Self {
            filetype: Arc::from(filetype),
            buftype: Arc::from(buftype),
            buflisted,
            line_count,
        }
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
    use crate::host::BufferMetadataCall;
    use crate::host::BufferMetadataSnapshot;
    use crate::host::FakeBufferMetadataPort;
    use crate::host::api;
    use pretty_assertions::assert_eq;

    #[test]
    fn cache_miss_reads_once_through_buffer_metadata_port() {
        let host = FakeBufferMetadataPort::default();
        host.push_buffer_metadata(BufferMetadataSnapshot::new("lua", "", true, 42));
        let buffer = api::Buffer::from(11);
        let mut cache = BufferMetadataCache::default();

        let metadata = cache
            .read(&host, &buffer)
            .expect("metadata cache miss should read through host port");

        assert_eq!(metadata, BufferMetadata::new_for_test("lua", "", true, 42));
        assert_eq!(
            host.calls(),
            vec![BufferMetadataCall::BufferMetadata {
                buffer_handle: 11.into()
            }]
        );

        let cached = cache
            .read(&host, &buffer)
            .expect("metadata cache hit should not reread host port");

        assert_eq!(cached, metadata);
        assert_eq!(
            host.calls(),
            vec![BufferMetadataCall::BufferMetadata {
                buffer_handle: 11.into()
            }]
        );
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
