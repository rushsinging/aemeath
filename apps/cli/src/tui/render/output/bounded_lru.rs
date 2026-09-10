//! 简单的有界 LRU map：HashMap 保存值，VecDeque 保存从旧到新的访问顺序。

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

pub(crate) struct BoundedLruMap<K, V> {
    entries: HashMap<K, V>,
    recency: VecDeque<K>,
    capacity: usize,
}

impl<K, V> BoundedLruMap<K, V>
where
    K: Clone + Eq + Hash,
{
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "LRU capacity must be greater than zero");
        Self {
            entries: HashMap::with_capacity(capacity),
            recency: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        if !self.entries.contains_key(key) {
            return None;
        }
        self.touch(key);
        self.entries.get(key)
    }

    #[cfg(test)]
    pub(crate) fn peek(&self, key: &K) -> Option<&V> {
        self.entries.get(key)
    }

    pub(crate) fn insert(&mut self, key: K, value: V) -> Option<(K, V)> {
        self.entries.insert(key.clone(), value);
        self.touch(&key);
        if self.entries.len() <= self.capacity {
            return None;
        }
        let oldest = self
            .recency
            .pop_front()
            .expect("non-empty LRU order when over capacity");
        self.entries.remove_entry(&oldest)
    }

    #[cfg(test)]
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) -> usize {
        let before = self.entries.len();
        self.entries.retain(|key, value| keep(key, value));
        self.recency.retain(|key| self.entries.contains_key(key));
        before.saturating_sub(self.entries.len())
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    fn touch(&mut self, key: &K) {
        if let Some(position) = self.recency.iter().position(|candidate| candidate == key) {
            self.recency.remove(position);
        }
        self.recency.push_back(key.clone());
    }
}
