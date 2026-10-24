use crate::inspection::{Bench, InstantMetric};
#[cfg(feature = "inspect")]
use crate::{
    algo::countdown::queue::{CoupledQueue, DecoupledQueue, LiveCost},
    event::{CollectionKind, GpsEvent},
    graph::GNode,
    inspection::{
        GpsInspectors, InspectGraph, Inspector, SCCount, cache_residency, emit_inspection,
        emit_inspection_with_entry_size,
    },
};
#[cfg(feature = "inspect")]
use core::mem::size_of;
#[cfg(feature = "inspect")]
use swap_cache::{cache::CacheSingleton, collections::CTreeMap};

#[cfg(feature = "inspect")]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn coupled<G, C, F>(
    heap: &CoupledQueue<C>,
    heap_cache: &mut Inspector<GNode>,
    done: &CTreeMap<GNode, u8, C>,
    done_cache: &mut Inspector<GNode>,
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

    heap.inspect_cache(async |step, rank| {
        if let Some(rank) = rank {
            heap_cache.current.insert(step.node);
            heap_cache.observe_rank(rank);
        }
    })
    .await;
    heap_cache.save(heap.len());

    done.inspect_cache(async |node, _, rank| {
        if let Some(rank) = rank {
            done_cache.current.insert(*node);
            done_cache.observe_rank(rank);
        }
    })
    .await;
    done_cache.save(done.len());

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
    let done_entry_size = size_of::<(GNode, u8)>();

    let ram = cache_residency(
        capacity,
        [
            (CollectionKind::Heap, heap_cache.resident_blocks()),
            (CollectionKind::Done, done_cache.resident_blocks()),
            (CollectionKind::Previous, previous_cache.resident_blocks()),
        ]
        .into_iter()
        .chain(gps.resident_collections()),
    );

    emit_inspection_with_entry_size(
        emit,
        CollectionKind::Heap,
        heap_cache,
        heap.entry_size(),
        ram,
    );
    emit_inspection_with_entry_size(emit, CollectionKind::Done, done_cache, done_entry_size, ram);
    emit_inspection(emit, CollectionKind::Previous, previous_cache, ram);
    gps.emit(emit, ram);

    stats_before_inspection.restore();
}

#[cfg(feature = "inspect")]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn decoupled<G, C, F>(
    heap: &DecoupledQueue<C>,
    heap_cache: &mut Inspector<GNode>,
    costs: &CTreeMap<GNode, LiveCost, C>,
    cost_cache: &mut Inspector<GNode>,
    done: &CTreeMap<GNode, u8, C>,
    done_cache: &mut Inspector<GNode>,
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
    let before = SCCount::get();
    heap.inspect_cache(async |node, rank| {
        if let Some(rank) = rank {
            heap_cache.current.insert(*node);
            heap_cache.observe_rank(rank);
        }
    })
    .await;
    heap_cache.save(heap.len());
    costs
        .inspect_cache(async |node, _, rank| {
            if let Some(rank) = rank {
                cost_cache.current.insert(*node);
                cost_cache.observe_rank(rank);
            }
        })
        .await;
    cost_cache.save(costs.len());
    done.inspect_cache(async |node, _, rank| {
        if let Some(rank) = rank {
            done_cache.current.insert(*node);
            done_cache.observe_rank(rank);
        }
    })
    .await;
    done_cache.save(done.len());
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
            (CollectionKind::Heap, heap_cache.resident_blocks()),
            (CollectionKind::States, cost_cache.resident_blocks()),
            (CollectionKind::Done, done_cache.resident_blocks()),
            (CollectionKind::Previous, previous_cache.resident_blocks()),
        ]
        .into_iter()
        .chain(gps.resident_collections()),
    );
    emit_inspection_with_entry_size(
        emit,
        CollectionKind::Heap,
        heap_cache,
        heap.entry_size(),
        ram,
    );
    emit_inspection_with_entry_size(
        emit,
        CollectionKind::States,
        cost_cache,
        size_of::<(GNode, LiveCost)>(),
        ram,
    );
    emit_inspection_with_entry_size(
        emit,
        CollectionKind::Done,
        done_cache,
        size_of::<(GNode, u8)>(),
        ram,
    );
    emit_inspection(emit, CollectionKind::Previous, previous_cache, ram);
    gps.emit(emit, ram);
    before.restore();
}

pub(crate) fn update_collection_maxima<B: Bench>(
    bench: &mut B,
    heap: usize,
    done: usize,
    previous: usize,
) {
    bench.instant(InstantMetric::MaxHeap, heap as u64);
    bench.instant(InstantMetric::MaxDone, done as u64);
    bench.instant(InstantMetric::MaxPrevious, previous as u64);
}

pub(crate) fn maxima<B: Bench>(
    bench: &mut B,
    heap: usize,
    costs: usize,
    done: usize,
    previous: usize,
) {
    bench.instant(InstantMetric::MaxHeap, heap as u64);
    bench.instant(InstantMetric::MaxStates, costs as u64);
    bench.instant(InstantMetric::MaxDone, done as u64);
    bench.instant(InstantMetric::MaxPrevious, previous as u64);
}
