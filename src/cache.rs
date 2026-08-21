use crate::model::Source;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

pub struct SourceCache {
    capacity: usize,
    entries: HashMap<String, Arc<Vec<Source>>>,
    order: VecDeque<String>,
}

impl SourceCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&mut self, key: &str) -> Option<Arc<Vec<Source>>> {
        let value = self.entries.get(key)?.clone();
        self.touch(key);
        Some(value)
    }

    pub fn insert(&mut self, key: String, value: Arc<Vec<Source>>) {
        if self.entries.contains_key(&key) {
            self.order.retain(|existing| existing != &key);
        }
        self.entries.insert(key.clone(), value);
        self.order.push_back(key);
        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    fn touch(&mut self, key: &str) {
        self.order.retain(|existing| existing != key);
        self.order.push_back(key.to_string());
    }
}
