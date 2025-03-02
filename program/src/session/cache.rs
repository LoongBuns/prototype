use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub struct ModuleCache {
    entries: BTreeMap<String, CacheEntry>,
    capacity: usize,
    allocated: usize,
}

struct CacheEntry {
    data: Vec<u8>,
    access: usize,
}

impl ModuleCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            capacity,
            allocated: 0,
        }
    }

    pub fn contains_key(&mut self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn get(&mut self, key: &str) -> Option<&[u8]> {
        self.entries.get_mut(key).map(|entry| {
            entry.access += 1;
            &entry.data[..]
        })
    }

    pub fn put(&mut self, key: &str, size: usize) {
        if let Some(removed_entry) = self.entries.remove(key) {
            self.allocated -= removed_entry.data.len();
        }

        while self.capacity - self.allocated < size {
            let victim = self.entries.iter()
                .min_by(|a, b| {
                    let a_score = a.1.access.pow(2) * b.1.data.len();
                    let b_score = b.1.access.pow(2) * a.1.data.len();
                    a_score.cmp(&b_score)
                })
                .map(|(k, _)| k.clone());

            if let Some(victim_key) = victim {
                if let Some(removed_entry) = self.entries.remove(&victim_key) {
                    self.allocated -= removed_entry.data.len();
                }
            } else {
                break;
            }
        }

        if size <= self.capacity - self.allocated {
            self.entries.insert(
                key.to_string(),
                CacheEntry {
                    data: vec![0; size],
                    access: 1,
                },
            );
            self.allocated += size;
        }
    }

    pub fn put_slice(&mut self, key: &str, offset: usize, data: &[u8]) {
        let entry = match self.entries.get_mut(key) {
            Some(e) => e,
            None => return,
        };

        let end = offset + data.len();
        if end > entry.data.len() {
            entry.data.resize(end, 0);
            self.allocated += end - entry.data.len();
            return;
        }

        entry.data[offset..end].copy_from_slice(data);
        entry.access += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_eviction() {
        let mut cache = ModuleCache::new(15);

        cache.put("k1", 5);
        cache.put_slice("k1", 0, &[1;5]);

        cache.put("k2", 10);
        cache.put_slice("k2", 0, &[2;10]);

        cache.put("k3", 2);
        cache.put_slice("k3", 0, &[3;2]);

        assert!(cache.get("k1").is_some());
        assert!(cache.get("k2").is_none());
        assert!(cache.get("k3").is_some());
    }

    #[test]
    fn test_update_existing_key() {
        let mut cache = ModuleCache::new(10);

        cache.put("k1", 1);
        cache.put_slice("k1", 0, &[1]);
        assert_eq!(cache.get("k1"), Some(&[1][..]));

        cache.put("k1", 3);
        cache.put_slice("k1", 0, &[1,2,3]);
        assert_eq!(cache.get("k1"), Some(&[1,2,3][..]));
    }

    #[test]
    fn test_access_count_affects_eviction() {
        let mut cache = ModuleCache::new(15);

        cache.put("k1", 5);
        cache.put_slice("k1", 0, &[1;5]);

        cache.put("k2", 10);
        cache.put_slice("k2", 0, &[2;10]);

        cache.get("k2");
        cache.get("k2");

        cache.put("k3", 2);
        cache.put_slice("k3", 0, &[3;2]);

        assert!(cache.get("k1").is_none());
        assert!(cache.get("k2").is_some());
        assert!(cache.get("k3").is_some());
    }
}
