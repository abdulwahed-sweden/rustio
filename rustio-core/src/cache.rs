//! In-process LRU cache for read queries.
//!
//! Keys are namespaced by table: `"posts:list:limit=20,offset=0"`.
//! Invalidation happens on any write to a given table — we simply
//! blow away every key whose prefix matches `"tablename:"`.
//!
//! The LRU is behind a `Mutex` because `lru::LruCache::get` needs
//! `&mut self` (it updates recency). We keep the cache small and
//! the critical section tiny — a few µs per lookup in the worst case.

use std::num::NonZeroUsize;
use std::sync::Mutex;

use bytes::Bytes;

pub struct QueryCache {
    inner: Mutex<lru::LruCache<String, Bytes>>,
}

impl QueryCache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).expect("capacity > 0");
        Self {
            inner: Mutex::new(lru::LruCache::new(cap)),
        }
    }

    pub fn get(&self, key: &str) -> Option<Bytes> {
        self.inner.lock().ok()?.get(key).cloned()
    }

    pub fn put(&self, key: String, value: Bytes) {
        if let Ok(mut lru) = self.inner.lock() {
            lru.put(key, value);
        }
    }

    /// Drop every entry whose key starts with `prefix:`. Called on
    /// writes to keep stale reads out.
    pub fn invalidate_prefix(&self, prefix: &str) {
        let prefix = format!("{prefix}:");
        let mut lru = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let stale: Vec<String> = lru
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(k, _)| k.clone())
            .collect();
        for k in stale {
            lru.pop(&k);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut lru) = self.inner.lock() {
            lru.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|l| l.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_then_get() {
        let c = QueryCache::new(4);
        c.put("posts:list".into(), Bytes::from_static(b"hello"));
        assert_eq!(c.get("posts:list"), Some(Bytes::from_static(b"hello")));
    }

    #[test]
    fn invalidate_prefix_only_hits_table() {
        let c = QueryCache::new(16);
        c.put("posts:list".into(), Bytes::from_static(b"a"));
        c.put("posts:count".into(), Bytes::from_static(b"b"));
        c.put("users:list".into(), Bytes::from_static(b"c"));
        c.invalidate_prefix("posts");
        assert!(c.get("posts:list").is_none());
        assert!(c.get("posts:count").is_none());
        assert_eq!(c.get("users:list"), Some(Bytes::from_static(b"c")));
    }
}
