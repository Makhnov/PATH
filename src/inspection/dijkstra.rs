use crate::inspection::{Bench, InstantMetric};

#[cfg(feature = "inspect")]
use crate::{
    algo::dijkstra::queue::Step,
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
use swap_cache::{
    cache::CacheSingleton,
    collections::{CTreeMap, PriorityQueue},
};

#[cfg(feature = "inspect")]
// L’inspection représente explicitement chaque collection du cache ; les regrouper masquerait les métriques.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn inspect<G, C, F>(
    heap: &PriorityQueue<u32, Step, C>,
    heap_cache: &mut Inspector<GNode>,
    done: &CTreeMap<GNode, (), C>,
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

    heap.inspect_cache(async |_, step, rank| {
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
        size_of::<((u32, u16), Step)>(),
        ram,
    );
    emit_inspection(emit, CollectionKind::Done, done_cache, ram);
    emit_inspection(emit, CollectionKind::Previous, previous_cache, ram);
    gps.emit(emit, ram);

    stats_before_inspection.restore();
}

pub(crate) fn update_collection_maxima<B: Bench>(bench: &mut B, heap: usize, done: usize, previous: usize) {
    bench.instant(InstantMetric::MaxHeap, heap as u64);
    bench.instant(InstantMetric::MaxDone, done as u64);
    bench.instant(InstantMetric::MaxPrevious, previous as u64);
}
