use std::collections::{HashMap, HashSet, BTreeMap, VecDeque};
use std::hash::Hash;
use std::sync::Mutex;

pub trait Cache<K, V: Clone>: Send + Sync {
    fn get(&self, key: &K) -> Option<V>;
    fn put(&self, key: K, value: V);
    fn invalidate(&self, key: &K);
}

// --- LRU Cache ---

struct LruNode<K, V> {
    key: K,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

struct LruInner<K, V> {
    capacity: usize,
    map: HashMap<K, usize>,
    nodes: Vec<LruNode<K, V>>,
    free_list: Vec<usize>,
    head: Option<usize>,
    tail: Option<usize>,
}

pub struct LruCache<K, V> {
    inner: Mutex<LruInner<K, V>>,
}

impl<K: Clone + Eq + Hash, V: Clone> LruCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "Capacity must be greater than 0");
        Self {
            inner: Mutex::new(LruInner {
                capacity,
                map: HashMap::new(),
                nodes: Vec::with_capacity(capacity),
                free_list: Vec::new(),
                head: None,
                tail: None,
            }),
        }
    }
}

impl<K: Clone + Eq + Hash, V> LruInner<K, V> {
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

impl<K: Clone + Eq + Hash + Send + Sync, V: Clone + Send + Sync> Cache<K, V> for LruCache<K, V> {
    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(&idx) = inner.map.get(key) {
            inner.move_to_front(idx);
            Some(inner.nodes[idx].value.clone())
        } else {
            None
        }
    }

    fn put(&self, key: K, value: V) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(&idx) = inner.map.get(&key) {
            inner.nodes[idx].value = value;
            inner.move_to_front(idx);
            return;
        }

        if inner.map.len() >= inner.capacity {
            if let Some(tail_idx) = inner.tail {
                inner.remove_node(tail_idx);
                let old_key = inner.nodes[tail_idx].key.clone();
                inner.map.remove(&old_key);
                inner.free_list.push(tail_idx);
            }
        }

        let new_idx = if let Some(idx) = inner.free_list.pop() {
            inner.nodes[idx] = LruNode { key: key.clone(), value, prev: None, next: None };
            idx
        } else {
            let idx = inner.nodes.len();
            inner.nodes.push(LruNode { key: key.clone(), value, prev: None, next: None });
            idx
        };

        inner.map.insert(key, new_idx);
        
        let old_head = inner.head;
        inner.nodes[new_idx].next = old_head;
        inner.nodes[new_idx].prev = None;
        if let Some(h) = old_head {
            inner.nodes[h].prev = Some(new_idx);
        }
        inner.head = Some(new_idx);
        if inner.tail.is_none() {
            inner.tail = Some(new_idx);
        }
    }

    fn invalidate(&self, key: &K) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(idx) = inner.map.remove(key) {
            inner.remove_node(idx);
            inner.free_list.push(idx);
        }
    }
}

// --- LFU Cache ---

struct LfuInner<K, V> {
    capacity: usize,
    map: HashMap<K, (V, usize)>,
    freqs: BTreeMap<usize, HashSet<K>>,
}

pub struct LfuCache<K, V> {
    inner: Mutex<LfuInner<K, V>>,
}

impl<K: Clone + Eq + Hash, V: Clone> LfuCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "Capacity must be greater than 0");
        Self {
            inner: Mutex::new(LfuInner {
                capacity,
                map: HashMap::new(),
                freqs: BTreeMap::new(),
            }),
        }
    }
}

impl<K: Clone + Eq + Hash, V> LfuInner<K, V> {
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

impl<K: Clone + Eq + Hash + Send + Sync, V: Clone + Send + Sync> Cache<K, V> for LfuCache<K, V> {
    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock().unwrap();
        if let Some((_, freq)) = inner.map.get(key) {
            let current_freq = *freq;
            inner.increment_freq(key, current_freq);
            inner.map.get(key).map(|(v, _)| v.clone())
        } else {
            None
        }
    }

    fn put(&self, key: K, value: V) {
        let mut inner = self.inner.lock().unwrap();
        if let Some((_, freq)) = inner.map.get(&key) {
            let current_freq = *freq;
            inner.map.insert(key.clone(), (value, current_freq));
            inner.increment_freq(&key, current_freq);
            return;
        }

        if inner.map.len() >= inner.capacity {
            if let Some((&_min_freq, keys)) = inner.freqs.iter().next() {
                let evict_key = keys.iter().next().unwrap().clone();
                if let Some((_, freq)) = inner.map.remove(&evict_key) {
                    if let Some(set) = inner.freqs.get_mut(&freq) {
                        set.remove(&evict_key);
                        if set.is_empty() {
                            inner.freqs.remove(&freq);
                        }
                    }
                }
            }
        }

        inner.map.insert(key.clone(), (value, 1));
        inner.freqs.entry(1).or_insert_with(HashSet::new).insert(key);
    }

    fn invalidate(&self, key: &K) {
        let mut inner = self.inner.lock().unwrap();
        if let Some((_, freq)) = inner.map.remove(key) {
            if let Some(set) = inner.freqs.get_mut(&freq) {
                set.remove(key);
                if set.is_empty() {
                    inner.freqs.remove(&freq);
                }
            }
        }
    }
}

// --- FIFO Cache ---

struct FifoInner<K, V> {
    capacity: usize,
    map: HashMap<K, V>,
    queue: VecDeque<K>,
}

pub struct FifoCache<K, V> {
    inner: Mutex<FifoInner<K, V>>,
}

impl<K: Clone + Eq + Hash, V: Clone> FifoCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "Capacity must be greater than 0");
        Self {
            inner: Mutex::new(FifoInner {
                capacity,
                map: HashMap::new(),
                queue: VecDeque::new(),
            }),
        }
    }
}

impl<K: Clone + Eq + Hash + Send + Sync, V: Clone + Send + Sync> Cache<K, V> for FifoCache<K, V> {
    fn get(&self, key: &K) -> Option<V> {
        let inner = self.inner.lock().unwrap();
        inner.map.get(key).cloned()
    }

    fn put(&self, key: K, value: V) {
        let mut inner = self.inner.lock().unwrap();
        if inner.map.contains_key(&key) {
            inner.map.insert(key, value);
            return;
        }

        while inner.map.len() >= inner.capacity {
            if let Some(evict_key) = inner.queue.pop_front() {
                inner.map.remove(&evict_key);
            }
        }

        inner.queue.push_back(key.clone());
        inner.map.insert(key, value);
    }

    fn invalidate(&self, key: &K) {
        let mut inner = self.inner.lock().unwrap();
        if inner.map.remove(key).is_some() {
            inner.queue.retain(|k| k != key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache() {
        let cache = LruCache::new(2);
        cache.put(1, "A");
        cache.put(2, "B");
        assert_eq!(cache.get(&1), Some("A")); // 1 is now most recently used
        cache.put(3, "C"); // Should evict 2
        assert_eq!(cache.get(&2), None);
        assert_eq!(cache.get(&3), Some("C"));
        assert_eq!(cache.get(&1), Some("A"));
    }

    #[test]
    fn test_lru_capacity_1() {
        let cache = LruCache::new(1);
        cache.put(1, "A");
        assert_eq!(cache.get(&1), Some("A"));
        cache.put(2, "B");
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some("B"));
    }

    #[test]
    fn test_lfu_cache() {
        let cache = LfuCache::new(2);
        cache.put(1, "A"); // freq: 1
        cache.put(2, "B"); // freq: 1
        assert_eq!(cache.get(&1), Some("A")); // 1 freq: 2
        cache.put(3, "C"); // Should evict 2 (freq 1)
        assert_eq!(cache.get(&2), None);
        assert_eq!(cache.get(&3), Some("C"));
        assert_eq!(cache.get(&1), Some("A"));
    }

    #[test]
    fn test_lfu_capacity_1() {
        let cache = LfuCache::new(1);
        cache.put(1, "A");
        assert_eq!(cache.get(&1), Some("A"));
        cache.put(2, "B");
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some("B"));
    }

    #[test]
    fn test_fifo_cache() {
        let cache = FifoCache::new(2);
        cache.put(1, "A");
        cache.put(2, "B");
        assert_eq!(cache.get(&1), Some("A")); // Doesn't change order
        cache.put(3, "C"); // Should evict 1
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some("B"));
        assert_eq!(cache.get(&3), Some("C"));
    }

    #[test]
    fn test_fifo_capacity_1() {
        let cache = FifoCache::new(1);
        cache.put(1, "A");
        assert_eq!(cache.get(&1), Some("A"));
        cache.put(2, "B");
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some("B"));
    }
}
