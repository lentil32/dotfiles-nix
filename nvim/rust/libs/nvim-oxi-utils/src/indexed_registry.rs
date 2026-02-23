use std::collections::HashMap;
use std::hash::Hash;

/// Value contract for registries keyed by one primary key and two unique indexes.
pub trait IndexedValue<I1, I2> {
    fn index_one(&self) -> I1;
    fn index_two(&self) -> I2;
}

/// Why an existing entry was evicted during `insert_replacing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionReason {
    Key,
    IndexOne,
    IndexTwo,
    KeyAndIndexOne,
    KeyAndIndexTwo,
    IndexOneAndIndexTwo,
    KeyAndIndexOneAndIndexTwo,
}

/// One evicted entry returned by `insert_replacing`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictedEntry<K, V> {
    pub key: K,
    pub value: V,
    pub reason: EvictionReason,
}

/// Structured result for `insert_replacing`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InsertOutcome<K, V> {
    evicted: Vec<EvictedEntry<K, V>>,
}

impl<K, V> InsertOutcome<K, V> {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.evicted.is_empty()
    }

    #[must_use]
    pub fn evicted(&self) -> &[EvictedEntry<K, V>] {
        &self.evicted
    }

    pub fn into_evicted(self) -> Vec<EvictedEntry<K, V>> {
        self.evicted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConflictReasonKind {
    Key,
    IndexOne,
    IndexTwo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ConflictReasonFlags {
    by_key: bool,
    by_index_one: bool,
    by_index_two: bool,
}

impl ConflictReasonFlags {
    fn add(&mut self, reason: ConflictReasonKind) {
        match reason {
            ConflictReasonKind::Key => self.by_key = true,
            ConflictReasonKind::IndexOne => self.by_index_one = true,
            ConflictReasonKind::IndexTwo => self.by_index_two = true,
        }
    }

    const fn to_eviction_reason(self) -> EvictionReason {
        match (self.by_key, self.by_index_one, self.by_index_two) {
            (true, false, false) => EvictionReason::Key,
            (false, true, false) => EvictionReason::IndexOne,
            (false, false, true) => EvictionReason::IndexTwo,
            (true, true, false) => EvictionReason::KeyAndIndexOne,
            (true, false, true) => EvictionReason::KeyAndIndexTwo,
            (false, true, true) => EvictionReason::IndexOneAndIndexTwo,
            (true, true, true) => EvictionReason::KeyAndIndexOneAndIndexTwo,
            (false, false, false) => {
                // Internal-only: this state must never be produced by merge.
                EvictionReason::Key
            }
        }
    }
}

/// One primary-key map plus two unique secondary indexes.
///
/// The registry guarantees that each index points to at most one primary key.
/// Insertions can atomically evict conflicting entries via `insert_replacing`.
#[derive(Debug, Clone)]
pub struct IndexedRegistry<K, V, I1, I2> {
    by_key: HashMap<K, V>,
    by_index_one: HashMap<I1, K>,
    by_index_two: HashMap<I2, K>,
}

impl<K, V, I1, I2> Default for IndexedRegistry<K, V, I1, I2> {
    fn default() -> Self {
        Self {
            by_key: HashMap::new(),
            by_index_one: HashMap::new(),
            by_index_two: HashMap::new(),
        }
    }
}

impl<K, V, I1, I2> IndexedRegistry<K, V, I1, I2>
where
    K: Copy + Eq + Hash,
    I1: Copy + Eq + Hash,
    I2: Copy + Eq + Hash,
    V: IndexedValue<I1, I2>,
{
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    pub fn clear(&mut self) {
        self.by_key.clear();
        self.by_index_one.clear();
        self.by_index_two.clear();
        self.debug_assert_consistent();
    }

    pub fn get(&self, key: K) -> Option<&V> {
        self.by_key.get(&key)
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        self.by_key.get_mut(&key)
    }

    pub fn key_by_index_one(&self, index: I1) -> Option<K> {
        self.by_index_one.get(&index).copied()
    }

    pub fn key_by_index_two(&self, index: I2) -> Option<K> {
        self.by_index_two.get(&index).copied()
    }

    pub fn get_by_index_one(&self, index: I1) -> Option<(K, &V)> {
        let key = self.key_by_index_one(index)?;
        let value = self.by_key.get(&key)?;
        Some((key, value))
    }

    pub fn get_by_index_two(&self, index: I2) -> Option<(K, &V)> {
        let key = self.key_by_index_two(index)?;
        let value = self.by_key.get(&key)?;
        Some((key, value))
    }

    #[must_use]
    pub fn contains_index_two(&self, index: I2) -> bool {
        self.by_index_two.contains_key(&index)
    }

    pub fn take_by_key(&mut self, key: K) -> Option<V> {
        let value = self.by_key.remove(&key)?;
        let removed_index_one = self.by_index_one.remove(&value.index_one());
        let removed_index_two = self.by_index_two.remove(&value.index_two());
        debug_assert!(removed_index_one == Some(key));
        debug_assert!(removed_index_two == Some(key));
        self.debug_assert_consistent();
        Some(value)
    }

    pub fn take_by_index_one(&mut self, index: I1) -> Option<(K, V)> {
        let key = self.by_index_one.get(&index).copied()?;
        self.take_by_key(key).map(|value| (key, value))
    }

    pub fn take_by_index_two(&mut self, index: I2) -> Option<(K, V)> {
        let key = self.by_index_two.get(&index).copied()?;
        self.take_by_key(key).map(|value| (key, value))
    }

    pub fn insert_replacing(&mut self, key: K, value: V) -> InsertOutcome<K, V> {
        let mut conflicts: Vec<(K, ConflictReasonFlags)> = Vec::with_capacity(3);
        if self.by_key.contains_key(&key) {
            merge_conflict_reason(&mut conflicts, key, ConflictReasonKind::Key);
        }
        if let Some(existing) = self.key_by_index_one(value.index_one()) {
            merge_conflict_reason(&mut conflicts, existing, ConflictReasonKind::IndexOne);
        }
        if let Some(existing) = self.key_by_index_two(value.index_two()) {
            merge_conflict_reason(&mut conflicts, existing, ConflictReasonKind::IndexTwo);
        }

        let mut evicted = Vec::with_capacity(conflicts.len());
        for (conflict_key, reason_flags) in conflicts {
            if let Some(old) = self.take_by_key(conflict_key) {
                let reason = reason_flags.to_eviction_reason();
                evicted.push(EvictedEntry {
                    key: conflict_key,
                    value: old,
                    reason,
                });
            }
        }

        self.insert_without_conflict(key, value);
        self.debug_assert_consistent();
        InsertOutcome { evicted }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.by_key.iter()
    }

    pub fn iter_index_one(&self) -> impl Iterator<Item = (&I1, &K)> {
        self.by_index_one.iter()
    }

    pub fn iter_index_two(&self) -> impl Iterator<Item = (&I2, &K)> {
        self.by_index_two.iter()
    }

    fn insert_without_conflict(&mut self, key: K, value: V) {
        let index_one = value.index_one();
        let index_two = value.index_two();
        let replaced_key = self.by_key.insert(key, value);
        let replaced_index_one = self.by_index_one.insert(index_one, key);
        let replaced_index_two = self.by_index_two.insert(index_two, key);
        debug_assert!(replaced_key.is_none());
        debug_assert!(replaced_index_one.is_none());
        debug_assert!(replaced_index_two.is_none());
    }

    fn debug_assert_consistent(&self) {
        debug_assert!(self.by_key.len() == self.by_index_one.len());
        debug_assert!(self.by_key.len() == self.by_index_two.len());

        for (key, value) in &self.by_key {
            debug_assert!(self.by_index_one.get(&value.index_one()) == Some(key));
            debug_assert!(self.by_index_two.get(&value.index_two()) == Some(key));
        }

        for (index_one, key) in &self.by_index_one {
            let Some(value) = self.by_key.get(key) else {
                debug_assert!(false, "index_one points to missing key");
                continue;
            };
            debug_assert!(&value.index_one() == index_one);
        }

        for (index_two, key) in &self.by_index_two {
            let Some(value) = self.by_key.get(key) else {
                debug_assert!(false, "index_two points to missing key");
                continue;
            };
            debug_assert!(&value.index_two() == index_two);
        }
    }
}

fn merge_conflict_reason<K: Copy + Eq>(
    conflicts: &mut Vec<(K, ConflictReasonFlags)>,
    key: K,
    reason: ConflictReasonKind,
) {
    if let Some((_, flags)) = conflicts.iter_mut().find(|(existing, _)| *existing == key) {
        flags.add(reason);
        return;
    }
    let mut flags = ConflictReasonFlags::default();
    flags.add(reason);
    conflicts.push((key, flags));
}

#[cfg(test)]
mod tests {
    use super::{EvictionReason, IndexedRegistry, IndexedValue};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Entry {
        owner: i64,
        token: i64,
        payload: i64,
    }

    impl IndexedValue<i64, i64> for Entry {
        fn index_one(&self) -> i64 {
            self.owner
        }

        fn index_two(&self) -> i64 {
            self.token
        }
    }

    fn assert_registry_invariants(registry: &IndexedRegistry<i64, Entry, i64, i64>) {
        assert_eq!(registry.len(), registry.iter_index_one().count());
        assert_eq!(registry.len(), registry.iter_index_two().count());

        for (key, value) in registry.iter() {
            assert_eq!(registry.key_by_index_one(value.owner), Some(*key));
            assert_eq!(registry.key_by_index_two(value.token), Some(*key));
            assert_eq!(
                registry.get_by_index_one(value.owner).map(|(k, _)| k),
                Some(*key)
            );
            assert_eq!(
                registry.get_by_index_two(value.token).map(|(k, _)| k),
                Some(*key)
            );
        }

        for (owner, key) in registry.iter_index_one() {
            let Some(entry) = registry.get(*key) else {
                panic!("index_one entry must map to existing key");
            };
            assert_eq!(entry.owner, *owner);
        }

        for (token, key) in registry.iter_index_two() {
            let Some(entry) = registry.get(*key) else {
                panic!("index_two entry must map to existing key");
            };
            assert_eq!(entry.token, *token);
        }
    }

    #[test]
    fn insert_and_lookup_by_all_indexes() {
        let mut registry = IndexedRegistry::<i64, Entry, i64, i64>::default();
        assert!(registry.is_empty());

        let evicted = registry.insert_replacing(
            1,
            Entry {
                owner: 10,
                token: 20,
                payload: 30,
            },
        );

        assert!(evicted.is_clean());
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.key_by_index_one(10), Some(1));
        assert_eq!(registry.key_by_index_two(20), Some(1));
        assert!(registry.contains_index_two(20));
        assert_registry_invariants(&registry);
    }

    #[test]
    fn insert_replacing_evicts_conflicts_once() {
        let mut registry = IndexedRegistry::<i64, Entry, i64, i64>::default();
        let _ = registry.insert_replacing(
            1,
            Entry {
                owner: 10,
                token: 20,
                payload: 30,
            },
        );
        let _ = registry.insert_replacing(
            2,
            Entry {
                owner: 11,
                token: 21,
                payload: 31,
            },
        );

        let evicted = registry.insert_replacing(
            3,
            Entry {
                owner: 10,
                token: 21,
                payload: 32,
            },
        );

        assert_eq!(evicted.evicted().len(), 2);
        assert!(
            evicted
                .evicted()
                .iter()
                .any(|entry| entry.key == 1 && entry.reason == EvictionReason::IndexOne)
        );
        assert!(
            evicted
                .evicted()
                .iter()
                .any(|entry| entry.key == 2 && entry.reason == EvictionReason::IndexTwo)
        );
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.key_by_index_one(10), Some(3));
        assert_eq!(registry.key_by_index_two(21), Some(3));
        assert_registry_invariants(&registry);
    }

    #[test]
    fn insert_replacing_reports_combined_reason_for_same_entry() {
        let mut registry = IndexedRegistry::<i64, Entry, i64, i64>::default();
        let _ = registry.insert_replacing(
            7,
            Entry {
                owner: 70,
                token: 80,
                payload: 90,
            },
        );

        let evicted = registry.insert_replacing(
            7,
            Entry {
                owner: 70,
                token: 80,
                payload: 91,
            },
        );

        assert_eq!(evicted.evicted().len(), 1);
        let removed = &evicted.evicted()[0];
        assert_eq!(removed.key, 7);
        assert_eq!(removed.reason, EvictionReason::KeyAndIndexOneAndIndexTwo);
        assert_eq!(removed.value.payload, 90);
        assert_registry_invariants(&registry);
    }

    #[test]
    fn take_by_index_removes_all_index_entries() {
        let mut registry = IndexedRegistry::<i64, Entry, i64, i64>::default();
        let _ = registry.insert_replacing(
            7,
            Entry {
                owner: 70,
                token: 80,
                payload: 90,
            },
        );

        let removed = registry.take_by_index_two(80);
        assert_eq!(
            removed,
            Some((
                7,
                Entry {
                    owner: 70,
                    token: 80,
                    payload: 90
                }
            ))
        );
        assert!(registry.get(7).is_none());
        assert!(registry.key_by_index_one(70).is_none());
        assert!(registry.key_by_index_two(80).is_none());
        assert!(registry.is_empty());
        assert_registry_invariants(&registry);
    }

    #[derive(Clone, Copy)]
    enum Step {
        InsertA,
        InsertB,
        InsertOwnerConflict,
        InsertTokenConflict,
        InsertKeyConflict,
        TakeKeyA,
        TakeOwnerB,
        TakeTokenOwnerConflict,
        Clear,
    }

    impl Step {
        const ALL: [Self; 9] = [
            Self::InsertA,
            Self::InsertB,
            Self::InsertOwnerConflict,
            Self::InsertTokenConflict,
            Self::InsertKeyConflict,
            Self::TakeKeyA,
            Self::TakeOwnerB,
            Self::TakeTokenOwnerConflict,
            Self::Clear,
        ];
    }

    fn apply_step(registry: &mut IndexedRegistry<i64, Entry, i64, i64>, step: Step) {
        match step {
            Step::InsertA => {
                let _ = registry.insert_replacing(
                    1,
                    Entry {
                        owner: 10,
                        token: 20,
                        payload: 100,
                    },
                );
            }
            Step::InsertB => {
                let _ = registry.insert_replacing(
                    2,
                    Entry {
                        owner: 30,
                        token: 40,
                        payload: 101,
                    },
                );
            }
            Step::InsertOwnerConflict => {
                let _ = registry.insert_replacing(
                    3,
                    Entry {
                        owner: 10,
                        token: 50,
                        payload: 102,
                    },
                );
            }
            Step::InsertTokenConflict => {
                let _ = registry.insert_replacing(
                    4,
                    Entry {
                        owner: 60,
                        token: 40,
                        payload: 103,
                    },
                );
            }
            Step::InsertKeyConflict => {
                let _ = registry.insert_replacing(
                    1,
                    Entry {
                        owner: 70,
                        token: 80,
                        payload: 104,
                    },
                );
            }
            Step::TakeKeyA => {
                let _ = registry.take_by_key(1);
            }
            Step::TakeOwnerB => {
                let _ = registry.take_by_index_one(30);
            }
            Step::TakeTokenOwnerConflict => {
                let _ = registry.take_by_index_two(50);
            }
            Step::Clear => registry.clear(),
        }
        assert_registry_invariants(registry);
    }

    fn run_sequences(sequence: &mut Vec<Step>, remaining: usize) {
        if remaining == 0 {
            let mut registry = IndexedRegistry::<i64, Entry, i64, i64>::default();
            assert_registry_invariants(&registry);
            for step in sequence {
                apply_step(&mut registry, *step);
            }
            return;
        }
        for step in Step::ALL {
            sequence.push(step);
            run_sequences(sequence, remaining - 1);
            let _ = sequence.pop();
        }
    }

    #[test]
    fn bounded_sequences_preserve_index_invariants() {
        run_sequences(&mut Vec::new(), 4);
    }
}
