// Meilleurs coûts et prédécesseurs persistants (chaque politique permet la réouverture)
use crate::{
    algo::bucket::{coupled, queue::NodeState},
    event::GpsEvent,
    graph::GNode,
    inspection::Bench,
};
use swap_cache::{
    cache::CacheSingleton,
    collections::{CTreeMap, RRBTree},
};

pub(crate) struct NodeStates<C: CacheSingleton>(pub CTreeMap<GNode, NodeState, C>);

impl<C: CacheSingleton> NodeStates<C> {
    pub fn new() -> Self {
        Self(CTreeMap::new())
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub async fn cost<F: FnMut(GpsEvent<GNode>), B: Bench>(
        &self,
        node: GNode,
        bench: &mut B,
        emit: &mut F,
    ) -> Option<u32> {
        coupled::state_get(&self.0, &node, bench, emit)
            .await
            .map(|state| state.cost)
    }

    pub async fn insert<F: FnMut(GpsEvent<GNode>), B: Bench>(
        &mut self,
        node: GNode,
        cost: u32,
        from: GNode,
        bench: &mut B,
        emit: &mut F,
    ) {
        coupled::state_insert(
            &mut self.0,
            node,
            NodeState {
                cost,
                previous: from,
            },
            bench,
            emit,
        )
        .await;
    }

    pub async fn improve<F: FnMut(GpsEvent<GNode>), B: Bench>(
        &mut self,
        from: GNode,
        to: GNode,
        cost: u32,
        bench: &mut B,
        emit: &mut F,
    ) -> bool {
        coupled::improve(&mut self.0, from, to, cost, bench, emit).await
    }

    pub async fn rebuild_path<F: FnMut(GpsEvent<GNode>), B: Bench>(
        &self,
        start: GNode,
        end: GNode,
        bench: &mut B,
        emit: &mut F,
    ) -> Option<RRBTree<GNode, C>> {
        coupled::rebuild_path(start, end, &self.0, bench, emit).await
    }
}
