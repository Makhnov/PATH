// Gère les coûts et prédécesseurs dans la map dédiée, vérifie les candidats et reconstruit le chemin final
use super::queue::{BucketQueue, LocalKey, NodeState, queue_push};
use crate::{
    algo::utils::states::{state_entry, state_write},
    event::{CacheSource, CollectionKind, GpsEvent},
    graph::GNode,
    inspection::{Bench, InstantMetric, StatsMetric, emit_node},
};
use embassy_time::Instant as TimeInstant;
use swap_cache::{
    cache::CacheSingleton,
    collections::{CTreeMap, RRBTree, Stack, btree::Entry},
};

pub(crate) async fn state_get<C, F, B>(
    states: &CTreeMap<GNode, NodeState, C>,
    node: &GNode,
    bench: &mut B,
    emit: &mut F,
) -> Option<NodeState>
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    let state = states.get(node).await.map(|state| *state);
    let delta = bench.after(StatsMetric::StatesGet, before);
    emit_node(bench, emit, *node, CacheSource::States, delta);
    bench.instant(InstantMetric::StatesReads, 1);
    state
}

pub(crate) async fn state_insert<C, F, B>(
    states: &mut CTreeMap<GNode, NodeState, C>,
    node: GNode,
    state: NodeState,
    bench: &mut B,
    emit: &mut F,
) where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    let _ = states.insert(node, state).await;
    let delta = bench.after(StatsMetric::StatesInsert, before);
    emit_node(bench, emit, node, CacheSource::States, delta);
    bench.instant(InstantMetric::StatesWrites, 1);
}

pub(crate) async fn improve<C, F, B>(
    states: &mut CTreeMap<GNode, NodeState, C>,
    from: GNode,
    to: GNode,
    new_cost: u32,
    bench: &mut B,
    emit: &mut F,
) -> bool
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let entry = state_entry(states, to, bench, emit).await;
    if let Entry::Occupied(ref entry) = entry
        && entry.get().cost <= new_cost
    {
        return false;
    }

    state_write(
        entry,
        NodeState {
            cost: new_cost,
            previous: from,
        },
        bench,
        emit,
    )
    .await;
    true
}

// Rejeter les entrées obsolètes avant l’insertion dans la file locale.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn push_if_current<C, F, B>(
    states: &CTreeMap<GNode, NodeState, C>,
    local: &mut BucketQueue<LocalKey, C>,
    estimation: u32,
    cost: u32,
    node: GNode,
    bench: &mut B,
    emit: &mut F,
) -> bool
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    if state_get(states, &node, bench, emit)
        .await
        .is_none_or(|state| state.cost != cost)
    {
        return false;
    }

    queue_push(
        local,
        (estimation, cost),
        node,
        CollectionKind::Local,
        StatsMetric::LocalPush,
        bench,
        emit,
    )
    .await;
    true
}

pub(crate) async fn rebuild_path<C, F, B>(
    start: GNode,
    end: GNode,
    states: &CTreeMap<GNode, NodeState, C>,
    bench: &mut B,
    emit: &mut F,
) -> Option<RRBTree<GNode, C>>
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let now = TimeInstant::now();
    let mut reversed = Stack::<GNode, C>::new();
    let mut current = end;

    let before = bench.before();
    reversed.push(current).await;
    let delta = bench.after(StatsMetric::Rebuild, before);
    emit_node(bench, emit, current, CacheSource::Rebuild, delta);

    while current != start {
        current = state_get(states, &current, bench, emit).await?.previous;
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
