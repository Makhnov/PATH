use embassy_time::Instant;
use swap_cache::{
    cache::CacheSingleton,
    collections::{CTreeMap, RRBTree, Stack},
};

use crate::{
    event::{CacheSource, GpsEvent},
    inspection::{Bench, InstantMetric, StatsMetric, emit_node},
    graph::GNode,
};

pub async fn rebuild<C, F, B>(
    start: GNode,
    end: GNode,
    previous: &CTreeMap<GNode, GNode, C>,
    bench: &mut B,
    emit: &mut F,
) -> Option<RRBTree<GNode, C>>
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let now = Instant::now();
    let mut reversed = Stack::<GNode, C>::new();
    let mut current = end;

    let before = bench.before();
    reversed.push(current).await;
    let delta = bench.after(StatsMetric::Rebuild, before);
    emit_node(bench, emit, current, CacheSource::Rebuild, delta);

    while current != start {
        let before = bench.before();
        let previous_node = previous.get(&current).await.map(|value| *value);
        let delta = bench.after(StatsMetric::PreviousGet, before);
        emit_node(bench, emit, current, CacheSource::Previous, delta);

        current = previous_node?;

        let before = bench.before();
        reversed.push(current).await;
        let delta = bench.after(StatsMetric::Rebuild, before);
        emit_node(bench, emit, current, CacheSource::Rebuild, delta);
    }

    let mut path = RRBTree::<GNode, C>::new().await;
    while let Some(node) = {
        let before = bench.before();
        let node = reversed.pop().await;
        let delta = bench.after(StatsMetric::Rebuild, before);
        if let Some(node) = node {
            emit_node(bench, emit, node, CacheSource::Rebuild, delta);
        }
        node
    } {
        let before = bench.before();
        path.push(node).await;
        let delta = bench.after(StatsMetric::Rebuild, before);
        emit_node(bench, emit, node, CacheSource::Rebuild, delta);
        bench.event(emit, GpsEvent::PathNode { node });
    }

    bench.instant(InstantMetric::RebuildMicros, now.elapsed().as_micros());
    Some(path)
}
