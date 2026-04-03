use std::collections::VecDeque;

#[derive(Debug, Clone, Eq, PartialEq)]
struct LruCacheEntry<K, V> {
    key: K,
    value: V,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct LruCache<K, V> {
    entries: VecDeque<LruCacheEntry<K, V>>,
    capacity: usize,
}

impl<K, V> LruCache<K, V> {
    pub(super) fn new(capacity: usize) -> Self {
        debug_assert!(capacity > 0);
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
}

impl<K: Eq, V> LruCache<K, V> {
    fn take_entry(&mut self, key: &K) -> Option<LruCacheEntry<K, V>> {
        let existing_index = self.entries.iter().position(|entry| entry.key == *key)?;
        self.entries.remove(existing_index)
    }

    pub(super) fn insert(&mut self, key: K, value: V) {
        let _ = self.take_entry(&key);
        self.entries.push_front(LruCacheEntry { key, value });
        while self.entries.len() > self.capacity {
            let _ = self.entries.pop_back();
        }
    }
}

impl<K: Eq, V: Clone> LruCache<K, V> {
    pub(super) fn peek_cloned(&self, key: &K) -> Option<V> {
        self.entries
            .iter()
            .find(|entry| entry.key == *key)
            .map(|entry| entry.value.clone())
    }

    pub(super) fn get_cloned(&mut self, key: &K) -> Option<V> {
        self.take_entry(key).map(|entry| {
            let value = entry.value.clone();
            self.entries.push_front(entry);
            value
        })
    }
}

#[cfg(test)]
mod tests {
    use super::LruCache;
    use crate::test_support::proptest::stateful_config;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use std::collections::VecDeque;

    #[derive(Clone, Copy, Debug)]
    enum CacheOp {
        Insert { key: u8, value: i16 },
        Get { key: u8 },
        Peek { key: u8 },
        Clear,
    }

    fn cache_op_strategy() -> BoxedStrategy<CacheOp> {
        prop_oneof![
            (0_u8..6, any::<i16>()).prop_map(|(key, value)| CacheOp::Insert { key, value }),
            (0_u8..6).prop_map(|key| CacheOp::Get { key }),
            (0_u8..6).prop_map(|key| CacheOp::Peek { key }),
            Just(CacheOp::Clear),
        ]
        .boxed()
    }

    fn model_insert(model: &mut VecDeque<(u8, i16)>, capacity: usize, key: u8, value: i16) {
        if let Some(index) = model
            .iter()
            .position(|(existing_key, _)| *existing_key == key)
        {
            let _ = model.remove(index);
        }
        model.push_front((key, value));
        while model.len() > capacity {
            let _ = model.pop_back();
        }
    }

    fn model_get(model: &mut VecDeque<(u8, i16)>, key: u8) -> Option<i16> {
        let index = model
            .iter()
            .position(|(existing_key, _)| *existing_key == key)?;
        let (key, value) = model.remove(index)?;
        model.push_front((key, value));
        Some(value)
    }

    fn model_peek(model: &VecDeque<(u8, i16)>, key: u8) -> Option<i16> {
        model
            .iter()
            .find(|(existing_key, _)| *existing_key == key)
            .map(|(_, value)| *value)
    }

    fn snapshot(cache: &LruCache<u8, i16>) -> Vec<(u8, i16)> {
        cache
            .entries
            .iter()
            .map(|entry| (entry.key, entry.value))
            .collect()
    }

    proptest! {
        #![proptest_config(stateful_config())]

        #[test]
        fn prop_lru_cache_matches_reference_model(
            capacity in 1_usize..=5,
            operations in vec(cache_op_strategy(), 1..64),
        ) {
            let mut cache = LruCache::new(capacity);
            let mut model = VecDeque::new();

            for operation in operations {
                match operation {
                    CacheOp::Insert { key, value } => {
                        cache.insert(key, value);
                        model_insert(&mut model, capacity, key, value);
                    }
                    CacheOp::Get { key } => {
                        prop_assert_eq!(cache.get_cloned(&key), model_get(&mut model, key));
                    }
                    CacheOp::Peek { key } => {
                        prop_assert_eq!(cache.peek_cloned(&key), model_peek(&model, key));
                    }
                    CacheOp::Clear => {
                        cache.clear();
                        model.clear();
                    }
                }

                prop_assert_eq!(snapshot(&cache), model.iter().copied().collect::<Vec<_>>());
            }
        }
    }
}
