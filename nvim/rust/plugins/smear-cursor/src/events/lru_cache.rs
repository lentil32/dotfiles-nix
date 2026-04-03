use std::collections::HashMap;
use std::hash::Hash;

#[derive(Debug, Clone, Eq, PartialEq)]
struct LruCacheNode<K, V> {
    key: K,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct LruCache<K: Eq + Hash, V> {
    nodes: Vec<Option<LruCacheNode<K, V>>>,
    free_indices: Vec<usize>,
    indices_by_key: HashMap<K, usize>,
    head: Option<usize>,
    tail: Option<usize>,
    len: usize,
    capacity: usize,
}

impl<K: Eq + Hash, V> LruCache<K, V> {
    pub(super) fn new(capacity: usize) -> Self {
        debug_assert!(capacity > 0);
        Self {
            nodes: Vec::with_capacity(capacity),
            free_indices: Vec::new(),
            indices_by_key: HashMap::with_capacity(capacity),
            head: None,
            tail: None,
            len: 0,
            capacity,
        }
    }

    pub(super) fn clear(&mut self) {
        self.nodes.clear();
        self.free_indices.clear();
        self.indices_by_key.clear();
        self.head = None;
        self.tail = None;
        self.len = 0;
    }

    pub(super) fn remove(&mut self, key: &K) -> Option<V> {
        let index = *self.indices_by_key.get(key)?;
        self.remove_node(index).map(|node| node.value)
    }

    fn detach(&mut self, index: usize) {
        let Some(node) = self.nodes.get(index).and_then(Option::as_ref) else {
            return;
        };
        let (prev, next) = (node.prev, node.next);

        if let Some(prev_index) = prev {
            if let Some(prev_node) = self.nodes.get_mut(prev_index).and_then(Option::as_mut) {
                prev_node.next = next;
            }
        } else {
            self.head = next;
        }

        if let Some(next_index) = next {
            if let Some(next_node) = self.nodes.get_mut(next_index).and_then(Option::as_mut) {
                next_node.prev = prev;
            }
        } else {
            self.tail = prev;
        }

        if let Some(node) = self.nodes.get_mut(index).and_then(Option::as_mut) {
            node.prev = None;
            node.next = None;
        }
    }

    fn attach_front(&mut self, index: usize) {
        let previous_head = self.head;
        if let Some(node) = self.nodes.get_mut(index).and_then(Option::as_mut) {
            node.prev = None;
            node.next = previous_head;
        }

        if let Some(head_index) = previous_head {
            if let Some(head_node) = self.nodes.get_mut(head_index).and_then(Option::as_mut) {
                head_node.prev = Some(index);
            }
        } else {
            self.tail = Some(index);
        }

        self.head = Some(index);
    }

    fn move_to_front(&mut self, index: usize) {
        if self.head == Some(index) {
            return;
        }
        self.detach(index);
        self.attach_front(index);
    }

    fn allocate_node(&mut self, key: K, value: V) -> usize {
        let node = LruCacheNode {
            key,
            value,
            prev: None,
            next: None,
        };
        if let Some(index) = self.free_indices.pop() {
            self.nodes[index] = Some(node);
            index
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
        }
    }

    fn remove_node(&mut self, index: usize) -> Option<LruCacheNode<K, V>> {
        self.detach(index);
        let node = self.nodes.get_mut(index)?.take()?;
        let _ = self.indices_by_key.remove(&node.key);
        self.free_indices.push(index);
        self.len = self.len.saturating_sub(1);
        Some(node)
    }
}

impl<K: Eq + Hash + Clone, V> LruCache<K, V> {
    pub(super) fn insert(&mut self, key: K, value: V) {
        if let Some(&existing_index) = self.indices_by_key.get(&key) {
            if let Some(existing_node) = self.nodes.get_mut(existing_index).and_then(Option::as_mut)
            {
                existing_node.value = value;
            }
            self.move_to_front(existing_index);
            return;
        }

        if self.len >= self.capacity {
            let _ = self
                .tail
                .and_then(|tail_index| self.remove_node(tail_index));
        }

        let index = self.allocate_node(key, value);
        let Some(node_key) = self
            .nodes
            .get(index)
            .and_then(Option::as_ref)
            .map(|node| node.key.clone())
        else {
            return;
        };
        self.attach_front(index);
        let _ = self.indices_by_key.insert(node_key, index);
        self.len += 1;
    }
}

impl<K: Eq + Hash, V: Clone> LruCache<K, V> {
    pub(super) fn peek_cloned(&self, key: &K) -> Option<V> {
        let index = *self.indices_by_key.get(key)?;
        self.nodes
            .get(index)
            .and_then(Option::as_ref)
            .map(|node| node.value.clone())
    }

    pub(super) fn get_cloned(&mut self, key: &K) -> Option<V> {
        let index = *self.indices_by_key.get(key)?;
        let value = self
            .nodes
            .get(index)
            .and_then(Option::as_ref)
            .map(|node| node.value.clone())?;
        self.move_to_front(index);
        Some(value)
    }
}

impl<K: Eq + Hash, V: Copy> LruCache<K, V> {
    pub(super) fn peek_copy(&self, key: &K) -> Option<V> {
        let index = *self.indices_by_key.get(key)?;
        self.nodes
            .get(index)
            .and_then(Option::as_ref)
            .map(|node| node.value)
    }

    pub(super) fn get_copy(&mut self, key: &K) -> Option<V> {
        let index = *self.indices_by_key.get(key)?;
        let value = self
            .nodes
            .get(index)
            .and_then(Option::as_ref)
            .map(|node| node.value)?;
        self.move_to_front(index);
        Some(value)
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
        let mut snapshot = Vec::new();
        let mut current = cache.head;
        while let Some(index) = current {
            let Some(node) = cache.nodes.get(index).and_then(Option::as_ref) else {
                break;
            };
            snapshot.push((node.key, node.value));
            current = node.next;
        }
        snapshot
    }

    #[test]
    fn remove_detaches_the_requested_key_and_preserves_remaining_order() {
        let mut cache = LruCache::new(3);
        cache.insert(1, 10);
        cache.insert(2, 20);
        cache.insert(3, 30);

        assert_eq!(cache.remove(&2), Some(20));
        assert_eq!(cache.remove(&2), None);
        assert_eq!(snapshot(&cache), vec![(3, 30), (1, 10)]);

        assert_eq!(cache.get_cloned(&1), Some(10));
        assert_eq!(snapshot(&cache), vec![(1, 10), (3, 30)]);
    }

    #[test]
    fn copy_accessors_return_the_cached_value_and_preserve_lru_behavior() {
        let mut cache = LruCache::new(3);
        cache.insert(1, 10);
        cache.insert(2, 20);
        cache.insert(3, 30);

        assert_eq!(cache.peek_copy(&2), Some(20));
        assert_eq!(snapshot(&cache), vec![(3, 30), (2, 20), (1, 10)]);

        assert_eq!(cache.get_copy(&2), Some(20));
        assert_eq!(snapshot(&cache), vec![(2, 20), (3, 30), (1, 10)]);
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
                        prop_assert_eq!(cache.get_copy(&key), model_get(&mut model, key));
                    }
                    CacheOp::Peek { key } => {
                        prop_assert_eq!(cache.peek_copy(&key), model_peek(&model, key));
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
