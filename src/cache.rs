use std::collections::{HashMap, HashSet, BTreeMap, VecDeque};
use std::hash::Hash;

pub trait Cache<K, V> {
    fn get(&mut self, key: &K) -> Option<&V>;
    fn put(&mut self, key: K, value: V);
    fn invalidate(&mut self, key: &K);
}

// --- LRU Cache ---

struct LruNode<K, V> {
    key: K,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

pub struct LruCache<K, V> {
    capacity: usize,
    map: HashMap<K, usize>,
    nodes: Vec<LruNode<K, V>>,
    free_list: Vec<usize>,
    head: Option<usize>,
    tail: Option<usize>,
}

impl<K: Clone + Eq + Hash, V> LruCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "Capacity must be greater than 0");
        Self {
            capacity,
            map: HashMap::new(),
            nodes: Vec::with_capacity(capacity),
            free_list: Vec::new(),
            head: None,
            tail: None,
        }
    }

    fn remove_node(&mut self, idx: usize) {
        let prev = self.nodes[idx].prev;
        let next = self.nodes[idx].next;

        if let Some(p) = prev {
            self.nodes[p].next = next;
        } else {
            self.head = next;
        }

        if let Some(n) = next {
            self.nodes[n].prev = prev;
        } else {
            self.tail = prev;
        }

        self.nodes[idx].prev = None;
        self.nodes[idx].next = None;
    }

    fn move_to_front(&mut self, idx: usize) {
        if self.head == Some(idx) {
            return;
        }
        self.remove_node(idx);

        let old_head = self.head;
        self.nodes[idx].next = old_head;
        self.nodes[idx].prev = None;
        
        if let Some(h) = old_head {
            self.nodes[h].prev = Some(idx);
        }
        self.head = Some(idx);
        if self.tail.is_none() {
            self.tail = Some(idx);
        }
    }
}

impl<K: Clone + Eq + Hash, V> Cache<K, V> for LruCache<K, V> {
    fn get(&mut self, key: &K) -> Option<&V> {
        if let Some(&idx) = self.map.get(key) {
            self.move_to_front(idx);
            Some(&self.nodes[idx].value)
        } else {
            None
        }
    }

    fn put(&mut self, key: K, value: V) {
        if let Some(&idx) = self.map.get(&key) {
            self.nodes[idx].value = value;
            self.move_to_front(idx);
            return;
        }

        if self.map.len() >= self.capacity {
            if let Some(tail_idx) = self.tail {
                self.remove_node(tail_idx);
                let old_key = self.nodes[tail_idx].key.clone();
                self.map.remove(&old_key);
                self.free_list.push(tail_idx);
            }
        }

        let new_idx = if let Some(idx) = self.free_list.pop() {
            self.nodes[idx] = LruNode { key: key.clone(), value, prev: None, next: None };
            idx
        } else {
            let idx = self.nodes.len();
            self.nodes.push(LruNode { key: key.clone(), value, prev: None, next: None });
            idx
        };

        self.map.insert(key, new_idx);
        
        let old_head = self.head;
        self.nodes[new_idx].next = old_head;
        self.nodes[new_idx].prev = None;
        if let Some(h) = old_head {
            self.nodes[h].prev = Some(new_idx);
        }
        self.head = Some(new_idx);
        if self.tail.is_none() {
            self.tail = Some(new_idx);
        }
    }

    fn invalidate(&mut self, key: &K) {
        if let Some(idx) = self.map.remove(key) {
            self.remove_node(idx);
            self.free_list.push(idx);
        }
    }
}

// --- LFU Cache ---

pub struct LfuCache<K, V> {
    capacity: usize,
    map: HashMap<K, (V, usize)>,
    freqs: BTreeMap<usize, HashSet<K>>,
}

impl<K: Clone + Eq + Hash, V> LfuCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "Capacity must be greater than 0");
        Self {
            capacity,
            map: HashMap::new(),
            freqs: BTreeMap::new(),
        }
    }

    fn increment_freq(&mut self, key: &K, current_freq: usize) {
        if let Some(set) = self.freqs.get_mut(&current_freq) {
            set.remove(key);
            if set.is_empty() {
                self.freqs.remove(&current_freq);
            }
        }
        
        let new_freq = current_freq + 1;
        self.freqs.entry(new_freq).or_insert_with(HashSet::new).insert(key.clone());
        if let Some(entry) = self.map.get_mut(key) {
            entry.1 = new_freq;
        }
    }
}

impl<K: Clone + Eq + Hash, V> Cache<K, V> for LfuCache<K, V> {
    fn get(&mut self, key: &K) -> Option<&V> {
        if let Some((_, freq)) = self.map.get(key) {
            let current_freq = *freq;
            self.increment_freq(key, current_freq);
            self.map.get(key).map(|(v, _)| v)
        } else {
            None
        }
    }

    fn put(&mut self, key: K, value: V) {
        if let Some((_, freq)) = self.map.get(&key) {
            let current_freq = *freq;
            self.map.insert(key.clone(), (value, current_freq));
            self.increment_freq(&key, current_freq);
            return;
        }

        if self.map.len() >= self.capacity {
            if let Some((&_min_freq, keys)) = self.freqs.iter().next() {
                let evict_key = keys.iter().next().unwrap().clone();
                self.invalidate(&evict_key);
            }
        }

        self.map.insert(key.clone(), (value, 1));
        self.freqs.entry(1).or_insert_with(HashSet::new).insert(key);
    }

    fn invalidate(&mut self, key: &K) {
        if let Some((_, freq)) = self.map.remove(key) {
            if let Some(set) = self.freqs.get_mut(&freq) {
                set.remove(key);
                if set.is_empty() {
                    self.freqs.remove(&freq);
                }
            }
        }
    }
}

// --- FIFO Cache ---

pub struct FifoCache<K, V> {
    capacity: usize,
    map: HashMap<K, V>,
    queue: VecDeque<K>,
}

impl<K: Clone + Eq + Hash, V> FifoCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "Capacity must be greater than 0");
        Self {
            capacity,
            map: HashMap::new(),
            queue: VecDeque::new(),
        }
    }
}

impl<K: Clone + Eq + Hash, V> Cache<K, V> for FifoCache<K, V> {
    fn get(&mut self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    fn put(&mut self, key: K, value: V) {
        if self.map.contains_key(&key) {
            self.map.insert(key, value);
            return;
        }

        while self.map.len() >= self.capacity {
            if let Some(evict_key) = self.queue.pop_front() {
                self.map.remove(&evict_key);
            }
        }

        self.queue.push_back(key.clone());
        self.map.insert(key, value);
    }

    fn invalidate(&mut self, key: &K) {
        if self.map.remove(key).is_some() {
            self.queue.retain(|k| k != key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache() {
        let mut cache = LruCache::new(2);
        cache.put(1, "A");
        cache.put(2, "B");
        assert_eq!(cache.get(&1), Some(&"A")); // 1 is now most recently used
        cache.put(3, "C"); // Should evict 2
        assert_eq!(cache.get(&2), None);
        assert_eq!(cache.get(&3), Some(&"C"));
        assert_eq!(cache.get(&1), Some(&"A"));
    }

    #[test]
    fn test_lru_capacity_1() {
        let mut cache = LruCache::new(1);
        cache.put(1, "A");
        assert_eq!(cache.get(&1), Some(&"A"));
        cache.put(2, "B");
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some(&"B"));
    }

    #[test]
    fn test_lfu_cache() {
        let mut cache = LfuCache::new(2);
        cache.put(1, "A"); // freq: 1
        cache.put(2, "B"); // freq: 1
        assert_eq!(cache.get(&1), Some(&"A")); // 1 freq: 2
        cache.put(3, "C"); // Should evict 2 (freq 1)
        assert_eq!(cache.get(&2), None);
        assert_eq!(cache.get(&3), Some(&"C"));
        assert_eq!(cache.get(&1), Some(&"A"));
    }

    #[test]
    fn test_lfu_capacity_1() {
        let mut cache = LfuCache::new(1);
        cache.put(1, "A");
        assert_eq!(cache.get(&1), Some(&"A"));
        cache.put(2, "B");
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some(&"B"));
    }

    #[test]
    fn test_fifo_cache() {
        let mut cache = FifoCache::new(2);
        cache.put(1, "A");
        cache.put(2, "B");
        assert_eq!(cache.get(&1), Some(&"A")); // Doesn't change order
        cache.put(3, "C"); // Should evict 1
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some(&"B"));
        assert_eq!(cache.get(&3), Some(&"C"));
    }

    #[test]
    fn test_fifo_capacity_1() {
        let mut cache = FifoCache::new(1);
        cache.put(1, "A");
        assert_eq!(cache.get(&1), Some(&"A"));
        cache.put(2, "B");
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some(&"B"));
    }
}
