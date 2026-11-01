use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::RwLock;

pub(crate) const NUM_SHARDS: usize = 64;

/// Partitioned lock-striped state storage to minimize lock contention under concurrent load.
pub(crate) struct ShardedState<T> {
    shards: Box<[RwLock<HashMap<String, T>>]>,
}

impl<T> Default for ShardedState<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ShardedState<T> {
    pub(crate) fn new() -> Self {
        let mut shards = Vec::with_capacity(NUM_SHARDS);
        for _ in 0..NUM_SHARDS {
            shards.push(RwLock::new(HashMap::new()));
        }
        Self {
            shards: shards.into_boxed_slice(),
        }
    }

    pub(crate) fn get_shard(&self, key: &str) -> &RwLock<HashMap<String, T>> {
        let mut hasher = std::hash::DefaultHasher::new();
        key.hash(&mut hasher);
        let idx = (hasher.finish() as usize) % NUM_SHARDS;
        &self.shards[idx]
    }
}
