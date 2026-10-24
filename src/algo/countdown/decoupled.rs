use crate::{
    event::{CacheSource, GpsEvent},
    graph::GNode,
    inspection::{Bench, StatsMetric, emit_node},
};
use swap_cache::{cache::CacheSingleton, collections::CTreeMap};

pub(super) async fn previous_insert<C, B, F>(
    previous: &mut CTreeMap<GNode, GNode, C>,
    node: GNode,
    from: GNode,
    bench: &mut B,
    emit: &mut F,
) where
    C: CacheSingleton,
    B: Bench,
    F: FnMut(GpsEvent<GNode>),
{
    let before = bench.before();
    previous.insert(node, from).await;
    let delta = bench.after(StatsMetric::PreviousInsert, before);
    emit_node(bench, emit, node, CacheSource::Previous, delta);
}
