// Variante Window : On utilise une fenêtre limitée de candidats en essayant de favoriser le traitement dans une même tuile (modifie l'ordre d'exploration)
mod options;
pub(crate) mod queue;
pub(crate) mod states;

#[cfg(feature = "inspect")]
use crate::inspection::{
    LocalityTrackers, record_edge_origin, record_heap_push_locality,
    record_neighbors_next_locality, record_neighbors_open_locality, window::Snapshots,
};
use crate::{
    event::{CacheSource, GpsEvent},
    format::Node,
    graph::{GNode, Graph},
    inspection::{
        Bench, BenchResult, InspectGraph, InstantMetric, PROFILE_ASTAR, StatsMetric, emit_node,
        finish_failed, finish_found, window::maxima,
    },
};
use embassy_time::Instant;
pub use options::{WindowOptions, WindowPolicy};
use queue::{Label, WindowQueue, queue_push};
use states::NodeStates;
use swap_cache::{cache::CacheSingleton, iterator::AIterator};

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowStats {
    pub batches: u64,
    pub switches: u64,
    pub max_batch: usize,
    pub relaxed_pops: u64,
    pub stale_pops: u64,
    pub peak_active_tiles: usize,
    pub max_f_slack: u64,
}

pub async fn astar_window<G, C, F, B>(
    graph: &G,
    start: Node,
    end: Node,
    options: WindowOptions,
    mut emit: F,
    bench: &mut B,
) -> (BenchResult<C>, WindowStats)
where
    G: Graph<ULoc = GNode, Loc = Node> + InspectGraph,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    assert!(options.max_batch > 0, "invalid zero tile batch");
    let now_search = Instant::now();
    let run_before = bench.before();
    let result = BenchResult::<C>::new();
    bench.instant(InstantMetric::Profile, PROFILE_ASTAR);
    let Ok(Some(start)) = graph.nearest_gnode(start).await else {
        return (
            finish_failed(result, bench, &mut emit, now_search, run_before),
            WindowStats::default(),
        );
    };
    let Ok(Some(end)) = graph.nearest_gnode(end).await else {
        return (
            finish_failed(result, bench, &mut emit, now_search, run_before),
            WindowStats::default(),
        );
    };
    bench.event(&mut emit, GpsEvent::Started { start, end });

    let before = bench.before();
    let mut open_set = WindowQueue::<C>::new().await;
    let delta = bench.after(StatsMetric::HeapPush, before);
    emit_node(bench, &mut emit, start, CacheSource::Heap, delta);
    let mut states = NodeStates::<C>::new();
    states.insert(start, 0, start, bench, &mut emit).await;
    bench.instant(InstantMetric::HeuristicCalls, 1);
    let start_f = graph.heuristic(start, end);
    queue_push(
        &mut open_set,
        Label {
            estimation: start_f,
            cost: 0,
            node: start,
        },
        bench,
        &mut emit,
    )
    .await;
    bench.event(
        &mut emit,
        GpsEvent::Pushed {
            node: start,
            cost: 0,
            estimation: start_f,
        },
    );
    maxima(&open_set, states.len(), bench);

    let mut best_end = (start == end).then_some(0);
    #[cfg(feature = "inspect")]
    let mut locality = LocalityTrackers::default();
    #[cfg(feature = "inspect")]
    let mut snapshots = Snapshots::default();

    loop {
        let before = bench.before();
        let minimum = open_set.lower_bound().await;
        let minimum_delta = bench.after(StatsMetric::HeapPop, before);
        let Some(minimum) = minimum else {
            break;
        };
        #[cfg(feature = "inspect")]
        if bench.tick() {
            snapshots
                .emit(&open_set, &states, graph, bench.capacity(), &mut emit)
                .await;
        }
        if best_end.is_some_and(|best| best <= minimum) {
            break;
        }
        let previous = open_set.active_tile();
        let before = bench.before();
        if !open_set
            .prepare(options, |tile| graph.tile_rank(tile))
            .await
        {
            let _ = bench.after(StatsMetric::HeapPop, before);
            break;
        }
        let head = open_set.active_head().await.expect("prepared active tile");
        let delta = bench.after(StatsMetric::HeapPop, before);
        emit_node(
            bench,
            &mut emit,
            head.node,
            CacheSource::Heap,
            minimum_delta,
        );
        emit_node(bench, &mut emit, head.node, CacheSource::Heap, delta);
        if previous != open_set.active_tile() {
            bench.instant(InstantMetric::GlobalPops, 1);
        }

        let before = bench.before();
        let label = open_set.pop_active().await.expect("prepared active head");
        let delta = bench.after(StatsMetric::HeapPop, before);
        emit_node(bench, &mut emit, label.node, CacheSource::Heap, delta);
        bench.instant(InstantMetric::LocalPops, 1);
        bench.instant(InstantMetric::HeapPops, 1);
        #[cfg(feature = "inspect")]
        locality.popped(bench, label.node, graph.tile_rank(label.node.tile()));

        // Chaque candidat extrait quitte l’ensemble ouvert du replay, y compris
        // les entrées obsolètes et celles rejetées par la borne du meilleur chemin.
        // Les estimations et les bornes inférieures sont en u32, avec saturation de g + h.
        bench.event(
            &mut emit,
            GpsEvent::Popped {
                node: label.node,
                cost: label.cost,
                estimation: label.estimation,
            },
        );

        if states
            .cost(label.node, bench, &mut emit)
            .await
            .is_none_or(|cost| cost != label.cost)
        {
            open_set.stats.stale_pops += 1;
            bench.instant(InstantMetric::NodesSkipped, 1);
            #[cfg(feature = "inspect")]
            locality.skipped(bench, label.node, graph.tile_rank(label.node.tile()));
            continue;
        }
        if best_end.is_some_and(|best| label.estimation >= best) {
            bench.instant(InstantMetric::NodesSkipped, 1);
            #[cfg(feature = "inspect")]
            locality.skipped(bench, label.node, graph.tile_rank(label.node.tile()));
            continue;
        }

        bench.event(
            &mut emit,
            GpsEvent::Settled {
                node: label.node,
                cost: label.cost,
            },
        );
        bench.instant(InstantMetric::NodesVisited, 1);
        #[cfg(feature = "inspect")]
        locality.visited(bench, label.node, graph.tile_rank(label.node.tile()));

        #[cfg(feature = "inspect")]
        record_neighbors_open_locality(bench, label.node);
        let before = bench.before();
        let neighbors = graph.neighbors(label.node).await;
        let delta = bench.after(StatsMetric::NeighborsOpen, before);
        emit_node(bench, &mut emit, label.node, CacheSource::Gps, delta);
        let Ok(mut neighbors) = neighbors else {
            return (
                finish_failed(result, bench, &mut emit, now_search, run_before),
                open_set.stats,
            );
        };
        loop {
            let before = bench.before();
            let next_edge = neighbors.next().await;
            let delta = bench.after(StatsMetric::NeighborsNext, before);
            emit_node(bench, &mut emit, label.node, CacheSource::Gps, delta);
            let Some(edge) = next_edge else {
                break;
            };
            #[cfg(feature = "inspect")]
            {
                record_neighbors_next_locality(bench, label.node, edge.neighbor);
                record_edge_origin(bench, label.node);
            }
            bench.instant(InstantMetric::EdgesVisited, 1);
            let Some(cost) = label.cost.checked_add(edge.cost) else {
                bench.instant(InstantMetric::NeighborsSkipped, 1);
                continue;
            };
            if !states
                .improve(label.node, edge.neighbor, cost, bench, &mut emit)
                .await
            {
                bench.instant(InstantMetric::NeighborsSkipped, 1);
                continue;
            }
            bench.instant(InstantMetric::HeuristicCalls, 1);
            let estimation = cost.saturating_add(graph.heuristic(edge.neighbor, end));
            if edge.neighbor == end {
                best_end = Some(best_end.map_or(cost, |best| best.min(cost)));
            }
            bench.event(
                &mut emit,
                GpsEvent::Relaxed {
                    from: label.node,
                    to: edge.neighbor,
                    edge_cost: edge.cost,
                    new_cost: cost,
                    estimation,
                },
            );
            if best_end.is_some_and(|best| estimation >= best) {
                bench.instant(InstantMetric::NeighborsSkipped, 1);
                continue;
            }
            #[cfg(feature = "inspect")]
            record_heap_push_locality(bench, label.node, edge.neighbor);
            queue_push(
                &mut open_set,
                Label {
                    estimation,
                    cost,
                    node: edge.neighbor,
                },
                bench,
                &mut emit,
            )
            .await;
            bench.instant(InstantMetric::NeighborsVisited, 1);
            bench.event(
                &mut emit,
                GpsEvent::Pushed {
                    node: edge.neighbor,
                    cost,
                    estimation,
                },
            );
            maxima(&open_set, states.len(), bench);
        }
    }

    #[cfg(feature = "inspect")]
    snapshots
        .emit(&open_set, &states, graph, bench.capacity(), &mut emit)
        .await;
    bench.instant(
        InstantMetric::SearchMicros,
        now_search.elapsed().as_micros(),
    );
    let stats = open_set.stats;
    let Some(cost) = best_end else {
        return (
            finish_failed(result, bench, &mut emit, now_search, run_before),
            stats,
        );
    };
    let Some(path) = states.rebuild_path(start, end, bench, &mut emit).await else {
        return (
            finish_failed(result, bench, &mut emit, now_search, run_before),
            stats,
        );
    };
    (
        finish_found(result, bench, &mut emit, cost, path, run_before),
        stats,
    )
}
