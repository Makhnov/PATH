use crate::inspection::{Bench, InstantMetric};

#[cfg(feature = "inspect")]
use crate::{
    algo::bucket::queue::{BucketQueue, GlobalKey, LocalKey, NodeState},
    event::{CollectionKind, GpsEvent},
    graph::GNode,
    inspection::{
        GpsInspectors, InspectGraph, Inspector, SCCount, emit_inspection,
        emit_inspection_with_entry_size, cache_residency,
    },
};
#[cfg(feature = "inspect")]
use core::mem::size_of;
#[cfg(feature = "inspect")]
use swap_cache::{cache::CacheSingleton, collections::CTreeMap};

#[cfg(feature = "inspect")]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn coupled<G, C, F>(
    global: &BucketQueue<GlobalKey, C>,
    global_cache: &mut Inspector<GNode>,
    local: &BucketQueue<LocalKey, C>,
    local_cache: &mut Inspector<GNode>,
    states: &CTreeMap<GNode, NodeState, C>,
    states_cache: &mut Inspector<GNode>,
    graph: &G,
    gps_cache: &mut GpsInspectors,
    capacity: usize,
    emit: &mut F,
) where
    G: InspectGraph,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
{
    let stats_before_inspection = SCCount::get();

    global
        .inspect_cache(async |node, rank| {
            if let Some(rank) = rank {
                global_cache.current.insert(*node);
                global_cache.observe_rank(rank);
            }
        })
        .await;
    global_cache.save(global.len());

    local
        .inspect_cache(async |node, rank| {
            if let Some(rank) = rank {
                local_cache.current.insert(*node);
                local_cache.observe_rank(rank);
            }
        })
        .await;
    local_cache.save(local.len());

    states
        .inspect_cache(async |node, _, rank| {
            if let Some(rank) = rank {
                states_cache.current.insert(*node);
                states_cache.observe_rank(rank);
            }
        })
        .await;
    states_cache.save(states.len());

    let gps = graph.inspect_gps(gps_cache).await;
    let ram = cache_residency(
        capacity,
        [
            (CollectionKind::Global, global_cache.resident_blocks()),
            (CollectionKind::Local, local_cache.resident_blocks()),
            (CollectionKind::States, states_cache.resident_blocks()),
        ]
        .into_iter()
        .chain(gps.resident_collections()),
    );

    emit_inspection_with_entry_size(
        emit,
        CollectionKind::Global,
        global_cache,
        global.entry_size(),
        ram,
    );
    emit_inspection_with_entry_size(
        emit,
        CollectionKind::Local,
        local_cache,
        local.entry_size(),
        ram,
    );
    emit_inspection_with_entry_size(
        emit,
        CollectionKind::States,
        states_cache,
        core::mem::size_of::<(GNode, NodeState)>(),
        ram,
    );
    gps.emit(emit, ram);

    stats_before_inspection.restore();
}

pub(crate) fn maxima_coupled<B: Bench>(bench: &mut B, global: usize, local: usize, states: usize) {
    bench.instant(InstantMetric::MaxGlobal, global as u64);
    bench.instant(InstantMetric::MaxLocal, local as u64);
    bench.instant(InstantMetric::MaxStates, states as u64);
    bench.instant(InstantMetric::MaxHeap, (global + local) as u64);
    bench.instant(InstantMetric::MaxPrevious, states as u64);
}

#[cfg(feature = "inspect")]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn decoupled<G, C, F>(
    global: &BucketQueue<GlobalKey, C>,
    global_cache: &mut Inspector<GNode>,
    local: &BucketQueue<LocalKey, C>,
    local_cache: &mut Inspector<GNode>,
    costs: &CTreeMap<GNode, u32, C>,
    costs_cache: &mut Inspector<GNode>,
    previous: &CTreeMap<GNode, GNode, C>,
    previous_cache: &mut Inspector<GNode>,
    graph: &G,
    gps_cache: &mut GpsInspectors,
    capacity: usize,
    emit: &mut F,
) where
    G: InspectGraph,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
{
    let stats_before_inspection = SCCount::get();

    global
        .inspect_cache(async |node, rank| {
            if let Some(rank) = rank {
                global_cache.current.insert(*node);
                global_cache.observe_rank(rank);
            }
        })
        .await;
    global_cache.save(global.len());

    local
        .inspect_cache(async |node, rank| {
            if let Some(rank) = rank {
                local_cache.current.insert(*node);
                local_cache.observe_rank(rank);
            }
        })
        .await;
    local_cache.save(local.len());

    costs
        .inspect_cache(async |node, _, rank| {
            if let Some(rank) = rank {
                costs_cache.current.insert(*node);
                costs_cache.observe_rank(rank);
            }
        })
        .await;
    costs_cache.save(costs.len());

    previous
        .inspect_cache(async |node, _, rank| {
            if let Some(rank) = rank {
                previous_cache.current.insert(*node);
                previous_cache.observe_rank(rank);
            }
        })
        .await;
    previous_cache.save(previous.len());

    let gps = graph.inspect_gps(gps_cache).await;
    let ram = cache_residency(
        capacity,
        [
            (CollectionKind::Global, global_cache.resident_blocks()),
            (CollectionKind::Local, local_cache.resident_blocks()),
            (CollectionKind::States, costs_cache.resident_blocks()),
            (CollectionKind::Previous, previous_cache.resident_blocks()),
        ]
        .into_iter()
        .chain(gps.resident_collections()),
    );

    emit_inspection_with_entry_size(
        emit,
        CollectionKind::Global,
        global_cache,
        global.entry_size(),
        ram,
    );
    emit_inspection_with_entry_size(
        emit,
        CollectionKind::Local,
        local_cache,
        local.entry_size(),
        ram,
    );
    emit_inspection_with_entry_size(
        emit,
        CollectionKind::States,
        costs_cache,
        size_of::<(GNode, u32)>(),
        ram,
    );
    emit_inspection(emit, CollectionKind::Previous, previous_cache, ram);
    gps.emit(emit, ram);

    stats_before_inspection.restore();
}

pub(crate) fn maxima_decoupled<B: Bench>(
    bench: &mut B,
    global: usize,
    local: usize,
    costs: usize,
    previous: usize,
) {
    bench.instant(InstantMetric::MaxGlobal, global as u64);
    bench.instant(InstantMetric::MaxLocal, local as u64);
    bench.instant(InstantMetric::MaxStates, costs as u64);
    bench.instant(InstantMetric::MaxPrevious, previous as u64);
    bench.instant(InstantMetric::MaxHeap, (global + local) as u64);
}
