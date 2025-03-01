use alloc::collections::{btree_map, BTreeMap};
use alloc::string::String;
use alloc::vec::Vec;

pub struct LruCache {
    map: BTreeMap<String, CacheEntry>,
    capacity: usize,
}

struct CacheEntry {
    data: Vec<u8>,
    access: usize,
    size: usize,
}

impl LruCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            map: BTreeMap::new(),
            capacity,
        }
    }

    pub fn get(&mut self, key: &str) -> Option<&[u8]> {
        self.map.get_mut(key).map(|entry| {
            entry.access += 1;
            &entry.data[..]
        })
    }

    pub fn put(&mut self, key: String, data: Vec<u8>) {
        let size = data.len();

        let mut candidate = None;
        if !self.map.contains_key(&key) && self.map.len() >= self.capacity {
            candidate = self.find_removal_candidate();
        }

        if let Some(k) = candidate {
            self.map.remove(&k);
        }

        match self.map.entry(key) {
            btree_map::Entry::Occupied(mut e) => {
                let entry = e.get_mut();
                entry.data = data;
                entry.size = size;
                entry.access += 1;
            }
            btree_map::Entry::Vacant(e) => {
                e.insert(CacheEntry {
                    data,
                    access: 1,
                    size,
                });
            }
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }

    fn find_removal_candidate(&self) -> Option<String> {
        self.map
            .iter()
            .fold(None, |acc: Option<(u64, u64, &String)>, (k, e)| {
                let current_access = e.access as u64;
                let current_size = e.size as u64;
                match acc {
                    Some((acc_access, acc_size, acc_key)) => {
                        // equivalent to comparing (current_access^2 / current_size) vs (acc_access^2 / acc_size)
                        let current_score = current_access.pow(2) * acc_size;
                        let acc_score = acc_access.pow(2) * current_size;
                        if current_score < acc_score {
                            Some((current_access, current_size, k))
                        } else {
                            Some((acc_access, acc_size, acc_key))
                        }
                    }
                    None => Some((current_access, current_size, k)),
                }
            })
            .map(|(_, _, k)| k.clone())
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn test_basic_eviction() {
        let mut cache = LruCache::new(2);

        cache.put("k1".to_string(), vec![1; 5]);
        cache.put("k2".to_string(), vec![2; 10]);

        cache.put("k3".to_string(), vec![3; 2]);

        assert!(cache.get("k1").is_some());
        assert!(cache.get("k2").is_none());
        assert!(cache.get("k3").is_some());
    }

    #[test]
    fn test_update_existing_key() {
        let mut cache = LruCache::new(2);

        cache.put("k1".to_string(), vec![1]);
        assert_eq!(cache.get("k1"), Some(&[1][..]));

        cache.put("k1".to_string(), vec![1, 2, 3]);
        assert_eq!(cache.get("k1"), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn test_access_count_affects_eviction() {
        let mut cache = LruCache::new(2);

        cache.put("k1".to_string(), vec![1; 5]);
        cache.put("k2".to_string(), vec![2; 10]);

        cache.get("k2");
        cache.get("k2");

        cache.put("k3".to_string(), vec![3; 2]);

        assert!(cache.get("k1").is_none());
        assert!(cache.get("k2").is_some());
        assert!(cache.get("k3").is_some());
    }
}
