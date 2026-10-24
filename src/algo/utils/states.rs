use crate::{
    event::{CacheSource, GpsEvent},
    graph::GNode,
    inspection::{Bench, InstantMetric, StatsMetric, emit_node},
};
use swap_cache::{
    cache::CacheSingleton,
    collections::{CTreeMap, btree::Entry},
};

pub(crate) async fn state_entry<'a, V, C, F, B>(
    states: &'a mut CTreeMap<GNode, V, C>,
    node: GNode,
    bench: &mut B,
    emit: &mut F,
) -> Entry<'a, GNode, V, C>
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    let entry = states.entry(node).await;
    let delta = bench.after(StatsMetric::StatesGet, before);
    emit_node(bench, emit, node, CacheSource::States, delta);
    bench.instant(InstantMetric::StatesReads, 1);
    entry
}

pub(crate) async fn state_write<V, C, F, B>(
    entry: Entry<'_, GNode, V, C>,
    value: V,
    bench: &mut B,
    emit: &mut F,
) where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let node = *entry.key();
    let before = bench.before();
    match entry {
        Entry::Occupied(mut entry) => {
            let _ = entry.insert(value);
        }
        Entry::Vacant(entry) => {
            drop(entry.insert(value).await);
        }
    }
    let delta = bench.after(StatsMetric::StatesInsert, before);
    emit_node(bench, emit, node, CacheSource::States, delta);
    bench.instant(InstantMetric::StatesWrites, 1);
}
