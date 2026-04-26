use super::lru_cache::LruCache;
use crate::core::types::Generation;
use crate::host::BufferHandle;

const BUFFER_TEXT_REVISION_CACHE_CAPACITY: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::events) struct BufferTextRevisionCache {
    entries: LruCache<BufferHandle, Generation>,
}

impl Default for BufferTextRevisionCache {
    fn default() -> Self {
        Self {
            entries: LruCache::new(BUFFER_TEXT_REVISION_CACHE_CAPACITY),
        }
    }
}

impl BufferTextRevisionCache {
    pub(in crate::events) fn current(
        &mut self,
        buffer_handle: impl Into<BufferHandle>,
    ) -> Generation {
        let buffer_handle = buffer_handle.into();
        self.entries
            .get_copy(&buffer_handle)
            .unwrap_or(Generation::INITIAL)
    }

    pub(in crate::events) fn advance(
        &mut self,
        buffer_handle: impl Into<BufferHandle>,
    ) -> Generation {
        let buffer_handle = buffer_handle.into();
        let next = self.current(buffer_handle).next();
        self.entries.insert(buffer_handle, next);
        next
    }

    pub(in crate::events) fn clear_buffer(&mut self, buffer_handle: impl Into<BufferHandle>) {
        let buffer_handle = buffer_handle.into();
        let _ = self.entries.remove(&buffer_handle);
    }

    pub(in crate::events) fn clear(&mut self) {
        self.entries.clear();
    }

    #[cfg(test)]
    pub(in crate::events) fn cached_entry_for_test(
        &self,
        buffer_handle: impl Into<BufferHandle>,
    ) -> Option<Generation> {
        let buffer_handle = buffer_handle.into();
        self.entries.peek_copy(&buffer_handle)
    }
}

#[cfg(test)]
mod tests {
    use super::BufferTextRevisionCache;
    use crate::core::types::Generation;
    use pretty_assertions::assert_eq;

    #[test]
    fn revisions_default_to_initial_advance_monotonically_and_clear_per_buffer() {
        let mut cache = BufferTextRevisionCache::default();

        assert_eq!(cache.current(11), Generation::INITIAL);
        assert_eq!(cache.advance(11), Generation::new(1));
        assert_eq!(cache.advance(11), Generation::new(2));
        assert_eq!(cache.current(29), Generation::INITIAL);
        assert_eq!(cache.cached_entry_for_test(11), Some(Generation::new(2)));

        cache.clear_buffer(11);

        assert_eq!(cache.cached_entry_for_test(11), None);
        assert_eq!(cache.current(11), Generation::INITIAL);
    }
}
