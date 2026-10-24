// Gère les coûts et prédécesseurs dans deux maps distinctes (découplage), vérifie les candidats et reconstruit le chemin final
use super::{
    BucketPolicy,
    queue::{BucketQueue, LocalKey, queue_push},
};
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

pub(super) async fn cost_get<C, F, B>(
    costs: &CTreeMap<GNode, u32, C>,
    node: &GNode,
    bench: &mut B,
    emit: &mut F,
) -> Option<u32>
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    let cost = costs.get(node).await.map(|cost| *cost);
    let delta = bench.after(StatsMetric::StatesGet, before);
    emit_node(bench, emit, *node, CacheSource::States, delta);
    bench.instant(InstantMetric::StatesReads, 1);
    cost
}

pub(super) async fn cost_insert<C, F, B>(
    costs: &mut CTreeMap<GNode, u32, C>,
    node: GNode,
    cost: u32,
    bench: &mut B,
    emit: &mut F,
) where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    let _ = costs.insert(node, cost).await;
    let delta = bench.after(StatsMetric::StatesInsert, before);
    emit_node(bench, emit, node, CacheSource::States, delta);
    bench.instant(InstantMetric::StatesWrites, 1);
}

pub(super) async fn previous_get<C, F, B>(
    previous: &CTreeMap<GNode, GNode, C>,
    node: &GNode,
    bench: &mut B,
    emit: &mut F,
) -> Option<GNode>
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    let value = previous.get(node).await.map(|value| *value);
    let delta = bench.after(StatsMetric::PreviousGet, before);
    emit_node(bench, emit, *node, CacheSource::Previous, delta);
    value
}

pub(super) async fn previous_insert<C, F, B>(
    previous: &mut CTreeMap<GNode, GNode, C>,
    node: GNode,
    from: GNode,
    bench: &mut B,
    emit: &mut F,
) where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    let _ = previous.insert(node, from).await;
    let delta = bench.after(StatsMetric::PreviousInsert, before);
    emit_node(bench, emit, node, CacheSource::Previous, delta);
}

// Les coûts des politiques stricte et spatiale peuvent être retirés à l’extraction
// Tile-aware les garde actifs pour permettre d’améliorer et de rouvrir les noeuds visités trop tôt.
pub(super) async fn current_for_policy<C, F, B>(
    policy: BucketPolicy,
    costs: &mut CTreeMap<GNode, u32, C>,
    node: GNode,
    queued_cost: u32,
    bench: &mut B,
    emit: &mut F,
) -> bool
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    if matches!(policy, BucketPolicy::TileAware) {
        return cost_get(costs, &node, bench, emit).await == Some(queued_cost);
    }

    let before = bench.before();
    let removed = match costs.entry(node).await {
        Entry::Occupied(entry) => {
            if *entry.get() != queued_cost {
                false
            } else {
                let _ = entry.remove().await;
                true
            }
        }
        Entry::Vacant(_) => false,
    };
    let delta = bench.after(StatsMetric::StatesRemove, before);
    emit_node(bench, emit, node, CacheSource::States, delta);
    bench.instant(InstantMetric::StatesReads, 1);
    if removed {
        bench.instant(InstantMetric::StatesWrites, 1);
    }
    removed
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn improve<C, F, B>(
    policy: BucketPolicy,
    costs: &mut CTreeMap<GNode, u32, C>,
    previous: &mut CTreeMap<GNode, GNode, C>,
    from: GNode,
    to: GNode,
    new_cost: u32,
    new_estimation: u32,
    best_end: Option<u32>,
    bench: &mut B,
    emit: &mut F,
) -> bool
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    if best_end.is_some_and(|best| new_estimation >= best) {
        return false;
    }

    let entry = state_entry(costs, to, bench, emit).await;
    if let Entry::Occupied(ref entry) = entry {
        if *entry.get() <= new_cost {
            return false;
        }
    } else {
        // Dans les modes strict et spatial, un coût absent avec un prédécesseur signifie que le noeud
        // est déjà finalisé. Tile-aware permet volontairement sa réouverture.
        if !matches!(policy, BucketPolicy::TileAware)
            && previous_get(previous, &to, bench, emit).await.is_some()
        {
            return false;
        }
    }

    state_write(entry, new_cost, bench, emit).await;
    previous_insert(previous, to, from, bench, emit).await;
    true
}

pub(super) async fn push_if_current<C, F, B>(
    costs: &CTreeMap<GNode, u32, C>,
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
    if cost_get(costs, &node, bench, emit).await != Some(cost) {
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

pub(super) async fn rebuild_path<C, F, B>(
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
    let now = TimeInstant::now();
    let mut reversed = Stack::<GNode, C>::new();
    let mut current = end;

    let before = bench.before();
    reversed.push(current).await;
    let delta = bench.after(StatsMetric::Rebuild, before);
    emit_node(bench, emit, current, CacheSource::Rebuild, delta);

    while current != start {
        current = previous_get(previous, &current, bench, emit).await?;
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
