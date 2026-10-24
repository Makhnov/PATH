// Variante Bucket : On regroupe les candidats par tranche de f avec une file globale et une file locale (modifie possible l'ordre d'explo)
pub(crate) mod coupled;
mod decoupled;
mod options;
pub(crate) mod queue;

#[cfg(feature = "inspect")]
use crate::inspection::{
    GpsInspectors, Inspector, LocalityTrackers, record_edge_origin, record_heap_push_locality,
    record_neighbors_next_locality, record_neighbors_open_locality,
};
use crate::{
    event::{CacheSource, CollectionKind, GpsEvent},
    format::Node,
    graph::{GNode, Graph},
    inspection::{
        Bench, BenchResult, InspectGraph, InstantMetric, PROFILE_ASTAR, StatsMetric, emit_node,
        finish_failed, finish_found,
    },
};
use embassy_time::Instant as TimeInstant;
use swap_cache::{cache::CacheSingleton, collections::CTreeMap, iterator::AIterator};

pub use options::{BUCKET_WIDTH, BucketPolicy, BucketStateMode};
use options::{global_key, same_ring, stays_local};
use queue::{BucketQueue, GlobalKey, LocalKey, NodeState, queue_pop, queue_push};

// Routage vers option couplée/découplée
pub async fn astar_bucket<G, C, F, B>(
    graph: &G,
    start: Node,
    end: Node,
    state_mode: BucketStateMode,
    policy: BucketPolicy,
    emit: F,
    bench: &mut B,
) -> BenchResult<C>
where
    G: Graph<ULoc = GNode, Loc = Node> + InspectGraph,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    match state_mode {
        BucketStateMode::Coupled => {
            astar_bucket_coupled::<G, C, F, B>(graph, start, end, policy, emit, bench).await
        }
        BucketStateMode::Decoupled => {
            astar_bucket_decoupled::<G, C, F, B>(graph, start, end, policy, emit, bench).await
        }
    }
}

// Version legacy
pub async fn astar_bucket_coupled<G, C, F, B>(
    graph: &G,
    start: Node,
    end: Node,
    policy: BucketPolicy,
    mut emit: F,
    bench: &mut B,
) -> BenchResult<C>
where
    G: Graph<ULoc = GNode, Loc = Node> + InspectGraph,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    #[cfg(feature = "inspect")]
    use crate::inspection::bucket::coupled as inspection;
    use crate::inspection::bucket::maxima_coupled as update_maxima;
    use coupled::{improve, push_if_current, rebuild_path, state_get, state_insert};

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

    let mut global: BucketQueue<GlobalKey, C> = BucketQueue::new(policy);
    let mut states: CTreeMap<GNode, NodeState, C> = CTreeMap::new();

    state_insert(
        &mut states,
        start,
        NodeState {
            cost: 0,
            previous: start,
        },
        bench,
        &mut emit,
    )
    .await;

    bench.instant(InstantMetric::HeuristicCalls, 1);
    let start_estimation = graph.heuristic(start, end);
    queue_push(
        &mut global,
        global_key(start_estimation, 0),
        start,
        CollectionKind::Global,
        StatsMetric::GlobalPush,
        bench,
        &mut emit,
    )
    .await;
    update_maxima(bench, global.len(), 0, states.len());
    bench.event(
        &mut emit,
        GpsEvent::Pushed {
            node: start,
            cost: 0,
            estimation: start_estimation,
        },
    );

    let mut best_end = (start == end).then_some(0);

    #[cfg(feature = "inspect")]
    let mut global_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut local_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut states_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut gps_cache = GpsInspectors::default();
    #[cfg(feature = "inspect")]
    let mut locality = LocalityTrackers::default();

    'global: loop {
        let mut local: BucketQueue<LocalKey, C> = BucketQueue::new(policy);

        #[cfg(feature = "inspect")]
        if bench.tick() {
            inspection(
                &global,
                &mut global_cache,
                &local,
                &mut local_cache,
                &states,
                &mut states_cache,
                graph,
                &mut gps_cache,
                bench.capacity(),
                &mut emit,
            )
            .await;
        }

        let Some((first_key, first_node)) = queue_pop(
            &mut global,
            CollectionKind::Global,
            StatsMetric::GlobalPop,
            bench,
            &mut emit,
        )
        .await
        else {
            break 'global;
        };

        let active_ring = first_key.0;
        let anchor_tile = first_node.tile();

        if best_end.is_some_and(|cost| active_ring >= cost) {
            break 'global;
        }

        let mut has_entry = push_if_current(
            &states,
            &mut local,
            first_key.1,
            first_key.2,
            first_node,
            bench,
            &mut emit,
        )
        .await;
        update_maxima(bench, global.len(), local.len(), states.len());

        'collect: loop {
            let Some((key, node)) = queue_pop(
                &mut global,
                CollectionKind::Global,
                StatsMetric::GlobalPop,
                bench,
                &mut emit,
            )
            .await
            else {
                break 'collect;
            };

            if !same_ring(first_key, key) {
                queue_push(
                    &mut global,
                    key,
                    node,
                    CollectionKind::Global,
                    StatsMetric::GlobalPush,
                    bench,
                    &mut emit,
                )
                .await;
                break 'collect;
            }

            has_entry |=
                push_if_current(&states, &mut local, key.1, key.2, node, bench, &mut emit).await;
            update_maxima(bench, global.len(), local.len(), states.len());
        }

        if !has_entry {
            continue;
        }

        'local: loop {
            #[cfg(feature = "inspect")]
            if bench.tick() {
                inspection(
                    &global,
                    &mut global_cache,
                    &local,
                    &mut local_cache,
                    &states,
                    &mut states_cache,
                    graph,
                    &mut gps_cache,
                    bench.capacity(),
                    &mut emit,
                )
                .await;
            }

            let Some(((estimation, cost), node)) = queue_pop(
                &mut local,
                CollectionKind::Local,
                StatsMetric::LocalPop,
                bench,
                &mut emit,
            )
            .await
            else {
                break 'local;
            };

            bench.instant(InstantMetric::HeapPops, 1);
            #[cfg(feature = "inspect")]
            locality.popped(bench, node, graph.tile_rank(node.tile()));

            if state_get(&states, &node, bench, &mut emit)
                .await
                .is_none_or(|state| state.cost != cost)
            {
                bench.instant(InstantMetric::NodesSkipped, 1);
                #[cfg(feature = "inspect")]
                locality.skipped(bench, node, graph.tile_rank(node.tile()));
                continue;
            }

            if best_end.is_some_and(|best| estimation >= best) {
                bench.instant(InstantMetric::NodesSkipped, 1);
                #[cfg(feature = "inspect")]
                locality.skipped(bench, node, graph.tile_rank(node.tile()));
                continue;
            }

            bench.event(
                &mut emit,
                GpsEvent::Popped {
                    node,
                    cost,
                    estimation,
                },
            );
            bench.event(&mut emit, GpsEvent::Settled { node, cost });
            bench.instant(InstantMetric::NodesVisited, 1);
            #[cfg(feature = "inspect")]
            locality.visited(bench, node, graph.tile_rank(node.tile()));

            #[cfg(feature = "inspect")]
            record_neighbors_open_locality(bench, node);
            let before = bench.before();
            let neighbors = graph.neighbors(node).await;
            let delta = bench.after(StatsMetric::NeighborsOpen, before);
            emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
            let Ok(mut neighbors) = neighbors else {
                return finish_failed(result, bench, &mut emit, now_search, run_before);
            };

            'neighbors: loop {
                let before = bench.before();
                let next_edge = neighbors.next().await;
                let delta = bench.after(StatsMetric::NeighborsNext, before);
                emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
                let Some(edge) = next_edge else {
                    break 'neighbors;
                };

                #[cfg(feature = "inspect")]
                {
                    record_neighbors_next_locality(bench, node, edge.neighbor);
                    record_edge_origin(bench, node);
                }
                bench.instant(InstantMetric::EdgesVisited, 1);
                let next = edge.neighbor;
                let Some(next_cost) = cost.checked_add(edge.cost) else {
                    bench.instant(InstantMetric::NeighborsSkipped, 1);
                    continue;
                };

                if !improve(&mut states, node, next, next_cost, bench, &mut emit).await {
                    bench.instant(InstantMetric::NeighborsSkipped, 1);
                    continue;
                }
                update_maxima(bench, global.len(), local.len(), states.len());

                bench.instant(InstantMetric::HeuristicCalls, 1);
                let next_estimation = next_cost.saturating_add(graph.heuristic(next, end));

                if next == end {
                    best_end = Some(best_end.map_or(next_cost, |best| best.min(next_cost)));
                }

                bench.event(
                    &mut emit,
                    GpsEvent::Relaxed {
                        from: node,
                        to: next,
                        edge_cost: edge.cost,
                        new_cost: next_cost,
                        estimation: next_estimation,
                    },
                );

                if best_end.is_some_and(|best| next_estimation >= best) {
                    bench.instant(InstantMetric::NeighborsSkipped, 1);
                    continue;
                }

                #[cfg(feature = "inspect")]
                record_heap_push_locality(bench, node, next);

                if stays_local(policy, anchor_tile, active_ring, next, next_estimation) {
                    queue_push(
                        &mut local,
                        (next_estimation, next_cost),
                        next,
                        CollectionKind::Local,
                        StatsMetric::LocalPush,
                        bench,
                        &mut emit,
                    )
                    .await;
                } else {
                    queue_push(
                        &mut global,
                        global_key(next_estimation, next_cost),
                        next,
                        CollectionKind::Global,
                        StatsMetric::GlobalPush,
                        bench,
                        &mut emit,
                    )
                    .await;
                }

                bench.instant(InstantMetric::NeighborsVisited, 1);
                bench.event(
                    &mut emit,
                    GpsEvent::Pushed {
                        node: next,
                        cost: next_cost,
                        estimation: next_estimation,
                    },
                );
                update_maxima(bench, global.len(), local.len(), states.len());
            }
        }
    }

    bench.instant(
        InstantMetric::SearchMicros,
        now_search.elapsed().as_micros(),
    );
    let Some(cost) = state_get(&states, &end, bench, &mut emit)
        .await
        .map(|state| state.cost)
    else {
        return finish_failed(result, bench, &mut emit, now_search, run_before);
    };

    let Some(path) = rebuild_path(start, end, &states, bench, &mut emit).await else {
        return finish_failed(result, bench, &mut emit, now_search, run_before);
    };

    finish_found(result, bench, &mut emit, cost, path, run_before)
}

// Version découplée : coûts et prédecesseurs stockés dans deux maps séparées
pub async fn astar_bucket_decoupled<G, C, F, B>(
    graph: &G,
    start: Node,
    end: Node,
    policy: BucketPolicy,
    mut emit: F,
    bench: &mut B,
) -> BenchResult<C>
where
    G: Graph<ULoc = GNode, Loc = Node> + InspectGraph,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    #[cfg(feature = "inspect")]
    use crate::inspection::bucket::decoupled as inspection;
    use crate::inspection::bucket::maxima_decoupled as update_maxima;
    use decoupled::{
        cost_insert, current_for_policy, improve, previous_insert, push_if_current, rebuild_path,
    };

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

    let mut global: BucketQueue<GlobalKey, C> = BucketQueue::new(policy);
    let mut costs: CTreeMap<GNode, u32, C> = CTreeMap::new();
    let mut previous: CTreeMap<GNode, GNode, C> = CTreeMap::new();

    cost_insert(&mut costs, start, 0, bench, &mut emit).await;
    previous_insert(&mut previous, start, start, bench, &mut emit).await;

    bench.instant(InstantMetric::HeuristicCalls, 1);
    let start_estimation = graph.heuristic(start, end);
    queue_push(
        &mut global,
        global_key(start_estimation, 0),
        start,
        CollectionKind::Global,
        StatsMetric::GlobalPush,
        bench,
        &mut emit,
    )
    .await;
    update_maxima(bench, global.len(), 0, costs.len(), previous.len());
    bench.event(
        &mut emit,
        GpsEvent::Pushed {
            node: start,
            cost: 0,
            estimation: start_estimation,
        },
    );

    let mut best_end = (start == end).then_some(0);

    #[cfg(feature = "inspect")]
    let mut global_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut local_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut costs_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut previous_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut gps_cache = GpsInspectors::default();
    #[cfg(feature = "inspect")]
    let mut locality = LocalityTrackers::default();

    'global: loop {
        let mut local: BucketQueue<LocalKey, C> = BucketQueue::new(policy);

        #[cfg(feature = "inspect")]
        if bench.tick() {
            inspection(
                &global,
                &mut global_cache,
                &local,
                &mut local_cache,
                &costs,
                &mut costs_cache,
                &previous,
                &mut previous_cache,
                graph,
                &mut gps_cache,
                bench.capacity(),
                &mut emit,
            )
            .await;
        }

        let Some((first_key, first_node)) = queue_pop(
            &mut global,
            CollectionKind::Global,
            StatsMetric::GlobalPop,
            bench,
            &mut emit,
        )
        .await
        else {
            break 'global;
        };

        let active_ring = first_key.0;
        let anchor_tile = first_node.tile();
        if best_end.is_some_and(|cost| active_ring >= cost) {
            break 'global;
        }

        let mut has_entry = push_if_current(
            &costs,
            &mut local,
            first_key.1,
            first_key.2,
            first_node,
            bench,
            &mut emit,
        )
        .await;
        update_maxima(
            bench,
            global.len(),
            local.len(),
            costs.len(),
            previous.len(),
        );

        'collect: loop {
            let Some((key, node)) = queue_pop(
                &mut global,
                CollectionKind::Global,
                StatsMetric::GlobalPop,
                bench,
                &mut emit,
            )
            .await
            else {
                break 'collect;
            };

            if !same_ring(first_key, key) {
                queue_push(
                    &mut global,
                    key,
                    node,
                    CollectionKind::Global,
                    StatsMetric::GlobalPush,
                    bench,
                    &mut emit,
                )
                .await;
                break 'collect;
            }

            has_entry |=
                push_if_current(&costs, &mut local, key.1, key.2, node, bench, &mut emit).await;
            update_maxima(
                bench,
                global.len(),
                local.len(),
                costs.len(),
                previous.len(),
            );
        }

        if !has_entry {
            continue;
        }

        'local: loop {
            #[cfg(feature = "inspect")]
            if bench.tick() {
                inspection(
                    &global,
                    &mut global_cache,
                    &local,
                    &mut local_cache,
                    &costs,
                    &mut costs_cache,
                    &previous,
                    &mut previous_cache,
                    graph,
                    &mut gps_cache,
                    bench.capacity(),
                    &mut emit,
                )
                .await;
            }

            let Some(((estimation, cost), node)) = queue_pop(
                &mut local,
                CollectionKind::Local,
                StatsMetric::LocalPop,
                bench,
                &mut emit,
            )
            .await
            else {
                break 'local;
            };

            bench.instant(InstantMetric::HeapPops, 1);
            #[cfg(feature = "inspect")]
            locality.popped(bench, node, graph.tile_rank(node.tile()));

            if !current_for_policy(policy, &mut costs, node, cost, bench, &mut emit).await {
                bench.instant(InstantMetric::NodesSkipped, 1);
                #[cfg(feature = "inspect")]
                locality.skipped(bench, node, graph.tile_rank(node.tile()));
                continue;
            }

            if best_end.is_some_and(|best| estimation >= best) {
                bench.instant(InstantMetric::NodesSkipped, 1);
                #[cfg(feature = "inspect")]
                locality.skipped(bench, node, graph.tile_rank(node.tile()));
                continue;
            }

            bench.event(
                &mut emit,
                GpsEvent::Popped {
                    node,
                    cost,
                    estimation,
                },
            );
            bench.event(&mut emit, GpsEvent::Settled { node, cost });
            bench.instant(InstantMetric::NodesVisited, 1);
            #[cfg(feature = "inspect")]
            locality.visited(bench, node, graph.tile_rank(node.tile()));

            #[cfg(feature = "inspect")]
            record_neighbors_open_locality(bench, node);
            let before = bench.before();
            let neighbors = graph.neighbors(node).await;
            let delta = bench.after(StatsMetric::NeighborsOpen, before);
            emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
            let Ok(mut neighbors) = neighbors else {
                return finish_failed(result, bench, &mut emit, now_search, run_before);
            };

            'neighbors: loop {
                let before = bench.before();
                let next_edge = neighbors.next().await;
                let delta = bench.after(StatsMetric::NeighborsNext, before);
                emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
                let Some(edge) = next_edge else {
                    break 'neighbors;
                };

                #[cfg(feature = "inspect")]
                {
                    record_neighbors_next_locality(bench, node, edge.neighbor);
                    record_edge_origin(bench, node);
                }
                bench.instant(InstantMetric::EdgesVisited, 1);
                let next = edge.neighbor;
                let Some(next_cost) = cost.checked_add(edge.cost) else {
                    bench.instant(InstantMetric::NeighborsSkipped, 1);
                    continue;
                };

                bench.instant(InstantMetric::HeuristicCalls, 1);
                let next_estimation = next_cost.saturating_add(graph.heuristic(next, end));

                if !improve(
                    policy,
                    &mut costs,
                    &mut previous,
                    node,
                    next,
                    next_cost,
                    next_estimation,
                    best_end,
                    bench,
                    &mut emit,
                )
                .await
                {
                    bench.instant(InstantMetric::NeighborsSkipped, 1);
                    continue;
                }

                if next == end {
                    best_end = Some(best_end.map_or(next_cost, |best| best.min(next_cost)));
                }

                bench.event(
                    &mut emit,
                    GpsEvent::Relaxed {
                        from: node,
                        to: next,
                        edge_cost: edge.cost,
                        new_cost: next_cost,
                        estimation: next_estimation,
                    },
                );

                #[cfg(feature = "inspect")]
                record_heap_push_locality(bench, node, next);

                if stays_local(policy, anchor_tile, active_ring, next, next_estimation) {
                    queue_push(
                        &mut local,
                        (next_estimation, next_cost),
                        next,
                        CollectionKind::Local,
                        StatsMetric::LocalPush,
                        bench,
                        &mut emit,
                    )
                    .await;
                } else {
                    queue_push(
                        &mut global,
                        global_key(next_estimation, next_cost),
                        next,
                        CollectionKind::Global,
                        StatsMetric::GlobalPush,
                        bench,
                        &mut emit,
                    )
                    .await;
                }

                bench.instant(InstantMetric::NeighborsVisited, 1);
                bench.event(
                    &mut emit,
                    GpsEvent::Pushed {
                        node: next,
                        cost: next_cost,
                        estimation: next_estimation,
                    },
                );
                update_maxima(
                    bench,
                    global.len(),
                    local.len(),
                    costs.len(),
                    previous.len(),
                );
            }
        }
    }

    bench.instant(
        InstantMetric::SearchMicros,
        now_search.elapsed().as_micros(),
    );

    let Some(cost) = best_end else {
        return finish_failed(result, bench, &mut emit, now_search, run_before);
    };

    let Some(path) = rebuild_path(start, end, &previous, bench, &mut emit).await else {
        return finish_failed(result, bench, &mut emit, now_search, run_before);
    };

    finish_found(result, bench, &mut emit, cost, path, run_before)
}
