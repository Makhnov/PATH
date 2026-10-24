// Version de référence avec heuristique
pub(crate) mod queue;

use crate::{
    event::{CacheSource, GpsEvent},
    format::Node,
    graph::{GNode, Graph},
    inspection::{
        Bench, BenchResult, InspectGraph, InstantMetric, PROFILE_ASTAR, StatsMetric,
        astar::update_collection_maxima, emit_node, finish_failed, finish_found,
    },
    path::rebuild,
};

#[cfg(feature = "inspect")]
use crate::inspection::{
    GpsInspectors, Inspector, LocalityTrackers, astar::inspect as inspection, record_edge_origin,
    record_heap_push_locality, record_neighbors_next_locality, record_neighbors_open_locality,
};

use embassy_time::Instant as TimeInstant;
use queue::Step;
use swap_cache::{
    cache::CacheSingleton,
    collections::{CTreeMap, PriorityQueue},
    iterator::AIterator,
};

pub async fn astar<G, C, F, B>(
    graph: &G,
    start: Node,
    end: Node,
    mut emit: F,
    bench: &mut B,
) -> BenchResult<C>
where
    G: Graph<ULoc = GNode, Loc = Node> + InspectGraph,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let now_search = TimeInstant::now();
    let run_before = bench.before();
    let result = BenchResult::<C>::new();
    bench.instant(InstantMetric::Profile, PROFILE_ASTAR);

    let Ok(Some(start)) = graph.nearest_gnode(start).await else {
        return finish_failed(result, bench, &mut emit, now_search, run_before);
    };
    let Ok(Some(end)) = graph.nearest_gnode(end).await else {
        return finish_failed(result, bench, &mut emit, now_search, run_before);
    };

    bench.event(&mut emit, GpsEvent::Started { start, end });

    let mut heap: PriorityQueue<(u32, u32), Step, C> = PriorityQueue::new();
    let mut done: CTreeMap<GNode, (), C> = CTreeMap::new();
    let mut previous: CTreeMap<GNode, GNode, C> = CTreeMap::new();

    #[cfg(feature = "inspect")]
    let mut heap_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut done_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut previous_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut gps_cache = GpsInspectors::default();
    #[cfg(feature = "inspect")]
    let mut locality = LocalityTrackers::default();

    bench.instant(InstantMetric::HeuristicCalls, 1);
    let estimation = graph.heuristic(start, end);
    let before = bench.before();
    heap.push(
        (estimation, 0),
        Step {
            node: start,
            prev: start,
        },
    )
    .await;
    let delta = bench.after(StatsMetric::HeapPush, before);
    emit_node(bench, &mut emit, start, CacheSource::Heap, delta);
    bench.instant(InstantMetric::HeapPushes, 1);
    update_collection_maxima(bench, heap.len(), done.len(), previous.len());
    bench.event(
        &mut emit,
        GpsEvent::Pushed {
            node: start,
            cost: 0,
            estimation,
        },
    );

    loop {
        #[cfg(feature = "inspect")]
        if bench.tick() {
            inspection(
                &heap,
                &mut heap_cache,
                &done,
                &mut done_cache,
                &previous,
                &mut previous_cache,
                graph,
                &mut gps_cache,
                bench.capacity(),
                &mut emit,
            )
            .await;
        }

        let before = bench.before();
        let popped = heap.pop_first().await;
        let delta = bench.after(StatsMetric::HeapPop, before);

        let Some(((estimation, cost), Step { node, prev })) = popped else {
            break;
        };

        emit_node(bench, &mut emit, node, CacheSource::Heap, delta);

        bench.instant(InstantMetric::HeapPops, 1);
        #[cfg(feature = "inspect")]
        locality.popped(bench, node, graph.tile_rank(node.tile()));
        bench.event(
            &mut emit,
            GpsEvent::Popped {
                node,
                cost,
                estimation,
            },
        );
        let before = bench.before();
        let inserted = done.insert(node, ()).await.is_none();
        let delta = bench.after(StatsMetric::DoneInsert, before);
        emit_node(bench, &mut emit, node, CacheSource::Done, delta);

        if !inserted {
            bench.instant(InstantMetric::NodesSkipped, 1);
            #[cfg(feature = "inspect")]
            locality.skipped(bench, node, graph.tile_rank(node.tile()));
            continue;
        }

        bench.instant(InstantMetric::NodesVisited, 1);
        #[cfg(feature = "inspect")]
        locality.visited(bench, node, graph.tile_rank(node.tile()));
        update_collection_maxima(bench, heap.len(), done.len(), previous.len());
        bench.event(&mut emit, GpsEvent::Settled { node, cost });

        if node != start {
            let before = bench.before();
            previous.insert(node, prev).await;
            let delta = bench.after(StatsMetric::PreviousInsert, before);
            emit_node(bench, &mut emit, node, CacheSource::Previous, delta);
            update_collection_maxima(bench, heap.len(), done.len(), previous.len());
        }

        if node == end {
            bench.instant(
                InstantMetric::SearchMicros,
                now_search.elapsed().as_micros(),
            );
            let Some(path) = rebuild(start, end, &previous, bench, &mut emit).await else {
                return finish_failed(result, bench, &mut emit, now_search, run_before);
            };

            return finish_found(result, bench, &mut emit, cost, path, run_before);
        }

        #[cfg(feature = "inspect")]
        record_neighbors_open_locality(bench, node);
        let before = bench.before();
        let neighbors = graph.neighbors(node).await;
        let delta = bench.after(StatsMetric::NeighborsOpen, before);
        emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
        let Ok(mut neighbors) = neighbors else {
            return finish_failed(result, bench, &mut emit, now_search, run_before);
        };

        loop {
            let before = bench.before();
            let next = neighbors.next().await;
            let delta = bench.after(StatsMetric::NeighborsNext, before);
            emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
            let Some(edge) = next else {
                break;
            };

            #[cfg(feature = "inspect")]
            {
                record_neighbors_next_locality(bench, node, edge.neighbor);
                record_edge_origin(bench, node);
            }
            bench.instant(InstantMetric::EdgesVisited, 1);
            let before = bench.before();
            let already_done = done.contains_key(&edge.neighbor).await;
            let delta = bench.after(StatsMetric::DoneGet, before);
            emit_node(bench, &mut emit, edge.neighbor, CacheSource::Done, delta);

            if already_done {
                bench.instant(InstantMetric::NeighborsSkipped, 1);
                continue;
            }

            bench.instant(InstantMetric::NeighborsVisited, 1);
            let next_cost = cost + edge.cost;
            bench.instant(InstantMetric::HeuristicCalls, 1);
            let next_estimation = next_cost.saturating_add(graph.heuristic(edge.neighbor, end));

            bench.event(
                &mut emit,
                GpsEvent::Relaxed {
                    from: node,
                    to: edge.neighbor,
                    edge_cost: edge.cost,
                    new_cost: next_cost,
                    estimation: next_estimation,
                },
            );

            let step = Step {
                node: edge.neighbor,
                prev: edge.node,
            };
            #[cfg(feature = "inspect")]
            record_heap_push_locality(bench, node, step.node);
            let before = bench.before();
            heap.push((next_estimation, next_cost), step).await;
            let delta = bench.after(StatsMetric::HeapPush, before);
            emit_node(bench, &mut emit, step.node, CacheSource::Heap, delta);

            bench.instant(InstantMetric::HeapPushes, 1);
            update_collection_maxima(bench, heap.len(), done.len(), previous.len());
            bench.event(
                &mut emit,
                GpsEvent::Pushed {
                    node: step.node,
                    cost: next_cost,
                    estimation: next_estimation,
                },
            );
        }
    }

    finish_failed(result, bench, &mut emit, now_search, run_before)
}
