// File sur le cache, avec départage explicite par noeud, sans horodatage susceptible de déborder.
use crate::graph::GNode;
use core::hash::Hash;
use swap_cache::{cache::CacheSingleton, collections::CTreeMap};

pub(crate) struct NodeQueue<K: Ord, C: CacheSingleton> {
    tree: CTreeMap<(K, GNode), (), C>,
}

impl<K: Ord + Hash, C: CacheSingleton> NodeQueue<K, C> {
    pub fn new() -> Self {
        Self {
            tree: CTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.tree.len()
    }

    pub async fn push(&mut self, key: K, node: GNode) {
        // Des entrées (priority,node) identiques représentent la même relaxation.
        self.tree.insert((key, node), ()).await;
    }

    pub async fn pop_first(&mut self) -> Option<(K, GNode)> {
        self.tree
            .pop_first()
            .await
            .map(|((key, node), ())| (key, node))
    }

    #[cfg(feature = "inspect")]
    pub async fn inspect_cache<F: AsyncFnMut(&GNode, Option<usize>)>(&self, mut emit: F) {
        self.tree
            .inspect_cache(async |(_, node), (), rank| emit(node, rank).await)
            .await;
    }
}
