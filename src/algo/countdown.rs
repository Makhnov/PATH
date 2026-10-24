// Variante Countdown : On utilise un compteur de connexions par noeud (en fonction du nb de voisins )pour supprimer les entrées done devenues inutiles
pub(crate) mod coupled;
pub(crate) mod decoupled;
mod options;
pub(crate) mod queue;

#[cfg(feature = "inspect")]
use crate::inspection::{
    GpsInspectors, Inspector, LocalityTrackers,
    countdown::{coupled as inspection, decoupled as inspect_decoupled},
    record_edge_origin, record_heap_push_locality, record_neighbors_next_locality,
    record_neighbors_open_locality,
};
use crate::{
    event::{CacheSource, GpsEvent},
    format::Node,
    graph::{GNode, Graph},
    inspection::{
        Bench, BenchResult, InspectGraph, InstantMetric, PROFILE_ASTAR, StatsMetric,
        countdown::{maxima, update_collection_maxima},
        emit_node, finish_failed, finish_found,
    },
    path::rebuild,
};
use coupled::{DoneConsume, done_consume, done_insert, done_remove};
use decoupled::previous_insert;
use embassy_time::Instant as TimeInstant;
pub use options::{CountdownPolicy, CountdownStateMode};
use queue::{
    CoupledQueue, DecoupledQueue, LiveCost, Step, queue_pop_coupled, queue_pop_decoupled,
    queue_push_coupled, queue_push_decoupled,
};
use swap_cache::{
    cache::CacheSingleton,
    collections::{CTreeMap, btree::Entry},
    iterator::AIterator,
};

// Routage vers option couplée/découplée NB: Pour les màj de incoming on suppose un graph non-orienté
pub async fn astar_countdown<G, C, F, B>(
    graph: &G,
    start: Node,
    end: Node,
    state_mode: CountdownStateMode,
    policy: CountdownPolicy,
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
        CountdownStateMode::Coupled => {
            astar_countdown_coupled::<G, C, F, B>(graph, start, end, policy, emit, bench).await
        }
        CountdownStateMode::Decoupled => {
            astar_countdown_decoupled::<G, C, F, B>(graph, start, end, policy, emit, bench).await
        }
    }
}

// Version legacy
pub async fn astar_countdown_coupled<G, C, F, B>(
    graph: &G,
    start: Node,
    end: Node,
    policy: CountdownPolicy,
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

    let mut heap: CoupledQueue<C> = CoupledQueue::new(policy);
    let mut done: CTreeMap<GNode, u8, C> = CTreeMap::new();
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
    queue_push_coupled(
        &mut heap,
        (estimation, 0),
        Step {
            node: start,
            prev: start,
        },
        bench,
        &mut emit,
    )
    .await;
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
        let Some(((estimation, cost), Step { node, prev })) =
            queue_pop_coupled(&mut heap, bench, &mut emit).await
        else {
            break;
        };
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

        let done_state = done_consume(&mut done, node, bench, &mut emit).await;
        if done_state != DoneConsume::Absent {
            if done_state == DoneConsume::Exhausted {
                done_remove(&mut done, node, bench, &mut emit).await;
            }
            bench.instant(InstantMetric::NodesSkipped, 1);
            #[cfg(feature = "inspect")]
            locality.skipped(bench, node, graph.tile_rank(node.tile()));
            continue;
        }

        bench.instant(InstantMetric::CountdownScans, 1);
        let before = bench.before();
        let degree_neighbors = graph.neighbors(node).await;
        let delta = bench.after(StatsMetric::NeighborsOpen, before);
        emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
        let Ok(mut degree_neighbors) = degree_neighbors else {
            return finish_failed(result, bench, &mut emit, now_search, run_before);
        };

        let mut degree = 0usize;
        loop {
            let before = bench.before();
            let next = degree_neighbors.next().await;
            let delta = bench.after(StatsMetric::NeighborsNext, before);
            emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
            let Some(_) = next else {
                break;
            };
            degree += 1;
            bench.instant(InstantMetric::CountdownEdges, 1);
        }

        let incoming_edge_taken = usize::from(node != start);
        let neighbors_number = (degree - incoming_edge_taken).max(1).min(u8::MAX as usize) as u8;
        done_insert(&mut done, node, neighbors_number, bench, &mut emit).await;

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
            let done_state = done_consume(&mut done, edge.neighbor, bench, &mut emit).await;
            if done_state != DoneConsume::Absent {
                if done_state == DoneConsume::Exhausted {
                    done_remove(&mut done, edge.neighbor, bench, &mut emit).await;
                }
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
            queue_push_coupled(
                &mut heap,
                (next_estimation, next_cost),
                step,
                bench,
                &mut emit,
            )
            .await;
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

// Version découplée : costs stocke à la fois le coût et incoming
pub async fn astar_countdown_decoupled<G, C, F, B>(
    graph: &G,
    start: Node,
    end: Node,
    policy: CountdownPolicy,
    mut emit: F,
    bench: &mut B,
) -> BenchResult<C>
where
    G: Graph<ULoc = GNode, Loc = Node> + InspectGraph,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let now = TimeInstant::now();
    let run_before = bench.before();
    let result = BenchResult::new();
    bench.instant(InstantMetric::Profile, PROFILE_ASTAR);
    let Ok(Some(start)) = graph.nearest_gnode(start).await else {
        return finish_failed(result, bench, &mut emit, now, run_before);
    };
    let Ok(Some(end)) = graph.nearest_gnode(end).await else {
        return finish_failed(result, bench, &mut emit, now, run_before);
    };
    bench.event(&mut emit, GpsEvent::Started { start, end });
    let mut heap = DecoupledQueue::<C>::new(policy);
    let mut costs = CTreeMap::<GNode, LiveCost, C>::new();
    let mut done = CTreeMap::<GNode, u8, C>::new();
    let mut previous = CTreeMap::<GNode, GNode, C>::new();
    #[cfg(feature = "inspect")]
    let mut heap_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut cost_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut done_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut previous_cache = Inspector::<GNode>::default();
    #[cfg(feature = "inspect")]
    let mut gps_cache = GpsInspectors::default();
    #[cfg(feature = "inspect")]
    let mut locality = LocalityTrackers::default();
    let before = bench.before();
    costs
        .insert(
            start,
            LiveCost {
                cost: 0,
                incoming: 0,
            },
        )
        .await;
    let delta = bench.after(StatsMetric::StatesInsert, before);
    emit_node(bench, &mut emit, start, CacheSource::States, delta);
    bench.instant(InstantMetric::StatesWrites, 1);
    previous_insert(&mut previous, start, start, bench, &mut emit).await;
    bench.instant(InstantMetric::HeuristicCalls, 1);
    let estimation = graph.heuristic(start, end);
    queue_push_decoupled(&mut heap, (estimation, 0), start, bench, &mut emit).await;
    maxima(bench, heap.len(), costs.len(), done.len(), previous.len());
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
            inspect_decoupled(
                &heap,
                &mut heap_cache,
                &costs,
                &mut cost_cache,
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
        let Some(((estimation, cost), node)) =
            queue_pop_decoupled(&mut heap, bench, &mut emit).await
        else {
            break;
        };
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
        let current = match costs.entry(node).await {
            Entry::Occupied(entry) if entry.get().cost == cost => Some(entry.remove().await),
            _ => None,
        };
        let delta = bench.after(StatsMetric::StatesRemove, before);
        emit_node(bench, &mut emit, node, CacheSource::States, delta);
        bench.instant(InstantMetric::StatesReads, 1);
        let Some(current) = current else {
            bench.instant(InstantMetric::NodesSkipped, 1);
            #[cfg(feature = "inspect")]
            locality.skipped(bench, node, graph.tile_rank(node.tile()));
            continue;
        };
        bench.instant(InstantMetric::StatesWrites, 1);
        bench.instant(InstantMetric::CountdownScans, 1);
        let before = bench.before();
        let degree_neighbors = graph.neighbors(node).await;
        let delta = bench.after(StatsMetric::NeighborsOpen, before);
        emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
        let Ok(mut degree_neighbors) = degree_neighbors else {
            return finish_failed(result, bench, &mut emit, now, run_before);
        };
        let mut degree = 0u8;
        loop {
            let before = bench.before();
            let next = degree_neighbors.next().await;
            let delta = bench.after(StatsMetric::NeighborsNext, before);
            emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
            if next.is_none() {
                break;
            }
            let Some(next_degree) = degree.checked_add(1) else {
                return finish_failed(result, bench, &mut emit, now, run_before);
            };
            degree = next_degree;
            bench.instant(InstantMetric::CountdownEdges, 1);
        }
        drop(degree_neighbors);
        let Some(remaining) = degree.checked_sub(current.incoming) else {
            return finish_failed(result, bench, &mut emit, now, run_before);
        };
        if remaining > 0 {
            let before = bench.before();
            done.insert(node, remaining).await;
            let delta = bench.after(StatsMetric::DoneInsert, before);
            emit_node(bench, &mut emit, node, CacheSource::Done, delta);
            bench.instant(InstantMetric::CountdownDoneInserts, 1);
        }
        bench.instant(InstantMetric::NodesVisited, 1);
        #[cfg(feature = "inspect")]
        locality.visited(bench, node, graph.tile_rank(node.tile()));
        maxima(bench, heap.len(), costs.len(), done.len(), previous.len());
        bench.event(&mut emit, GpsEvent::Settled { node, cost });
        if node == end {
            bench.instant(InstantMetric::SearchMicros, now.elapsed().as_micros());
            let Some(path) = rebuild(start, end, &previous, bench, &mut emit).await else {
                return finish_failed(result, bench, &mut emit, now, run_before);
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
            return finish_failed(result, bench, &mut emit, now, run_before);
        };
        loop {
            let before = bench.before();
            let next = neighbors.next().await;
            let delta = bench.after(StatsMetric::NeighborsNext, before);
            emit_node(bench, &mut emit, node, CacheSource::Gps, delta);
            let Some(edge) = next else { break };
            #[cfg(feature = "inspect")]
            {
                record_neighbors_next_locality(bench, node, edge.neighbor);
                record_edge_origin(bench, node);
            }
            bench.instant(InstantMetric::EdgesVisited, 1);
            bench.instant(InstantMetric::CountdownDoneChecks, 1);
            let before = bench.before();
            let closed = if let Some(mut entry) = done.get_mut(&edge.neighbor).await {
                *entry -= 1;
                Some(*entry == 0)
            } else {
                None
            };
            let delta = bench.after(StatsMetric::DoneGet, before);
            emit_node(bench, &mut emit, edge.neighbor, CacheSource::Done, delta);
            if let Some(exhausted) = closed {
                if exhausted {
                    let before = bench.before();
                    done.remove(&edge.neighbor).await;
                    let delta = bench.after(StatsMetric::DoneRemove, before);
                    emit_node(bench, &mut emit, edge.neighbor, CacheSource::Done, delta);
                    bench.instant(InstantMetric::CountdownDoneRemoves, 1);
                } else {
                    bench.instant(InstantMetric::CountdownDoneDecs, 1);
                }
                bench.instant(InstantMetric::NeighborsSkipped, 1);
                continue;
            }
            if edge.neighbor == node {
                bench.instant(InstantMetric::NeighborsSkipped, 1);
                continue;
            }
            let Some(next_cost) = cost.checked_add(edge.cost) else {
                return finish_failed(result, bench, &mut emit, now, run_before);
            };
            let before = bench.before();
            let improved = match costs.entry(edge.neighbor).await {
                Entry::Occupied(mut entry) => {
                    let pending = entry.get_mut();
                    let Some(incoming) = pending.incoming.checked_add(1) else {
                        return finish_failed(result, bench, &mut emit, now, run_before);
                    };
                    pending.incoming = incoming;
                    if pending.cost > next_cost {
                        pending.cost = next_cost;
                        true
                    } else {
                        false
                    }
                }
                Entry::Vacant(entry) => {
                    entry
                        .insert(LiveCost {
                            cost: next_cost,
                            incoming: 1,
                        })
                        .await;
                    true
                }
            };
            let delta = bench.after(StatsMetric::StatesInsert, before);
            emit_node(bench, &mut emit, edge.neighbor, CacheSource::States, delta);
            bench.instant(InstantMetric::StatesReads, 1);
            bench.instant(InstantMetric::StatesWrites, 1);
            if !improved {
                bench.instant(InstantMetric::NeighborsSkipped, 1);
                continue;
            }
            previous_insert(&mut previous, edge.neighbor, node, bench, &mut emit).await;
            bench.instant(InstantMetric::NeighborsVisited, 1);
            bench.instant(InstantMetric::HeuristicCalls, 1);
            let estimation = next_cost.saturating_add(graph.heuristic(edge.neighbor, end));
            bench.event(
                &mut emit,
                GpsEvent::Relaxed {
                    from: node,
                    to: edge.neighbor,
                    edge_cost: edge.cost,
                    new_cost: next_cost,
                    estimation,
                },
            );
            #[cfg(feature = "inspect")]
            record_heap_push_locality(bench, node, edge.neighbor);
            queue_push_decoupled(
                &mut heap,
                (estimation, next_cost),
                edge.neighbor,
                bench,
                &mut emit,
            )
            .await;
            maxima(bench, heap.len(), costs.len(), done.len(), previous.len());
            bench.event(
                &mut emit,
                GpsEvent::Pushed {
                    node: edge.neighbor,
                    cost: next_cost,
                    estimation,
                },
            );
        }
    }
    finish_failed(result, bench, &mut emit, now, run_before)
}
