// Gère la map ordonnée done : insertion des noeuds, màj du compteur et suppression des entrées obsolètes
use crate::{
    event::{CacheSource, GpsEvent},
    graph::GNode,
    inspection::{Bench, InstantMetric, StatsMetric, emit_node},
};
use swap_cache::{cache::CacheSingleton, collections::CTreeMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DoneConsume {
    Absent,
    Decremented,
    Exhausted,
}

pub(super) async fn done_consume<C, B, F>(
    done: &mut CTreeMap<GNode, u8, C>,
    node: GNode,
    bench: &mut B,
    emit: &mut F,
) -> DoneConsume
where
    C: CacheSingleton,
    B: Bench,
    F: FnMut(GpsEvent<GNode>),
{
    bench.instant(InstantMetric::CountdownDoneChecks, 1);
    let before = bench.before();
    let mut entry = done.get_mut(&node).await;
    let result = match entry.as_deref_mut() {
        None => DoneConsume::Absent,
        Some(count) if *count <= 1 => DoneConsume::Exhausted,
        Some(count) => {
            *count -= 1;
            DoneConsume::Decremented
        }
    };
    drop(entry);
    let delta = bench.after(StatsMetric::DoneGet, before);
    emit_node(bench, emit, node, CacheSource::Done, delta);

    if result == DoneConsume::Decremented {
        bench.instant(InstantMetric::CountdownDoneDecs, 1);
    }
    result
}

pub(super) async fn done_insert<C, B, F>(
    done: &mut CTreeMap<GNode, u8, C>,
    node: GNode,
    count: u8,
    bench: &mut B,
    emit: &mut F,
) where
    C: CacheSingleton,
    B: Bench,
    F: FnMut(GpsEvent<GNode>),
{
    bench.instant(InstantMetric::CountdownDoneInserts, 1);
    let before = bench.before();
    done.insert(node, count).await;
    let delta = bench.after(StatsMetric::DoneInsert, before);
    emit_node(bench, emit, node, CacheSource::Done, delta);
}

pub(super) async fn done_remove<C, B, F>(
    done: &mut CTreeMap<GNode, u8, C>,
    node: GNode,
    bench: &mut B,
    emit: &mut F,
) where
    C: CacheSingleton,
    B: Bench,
    F: FnMut(GpsEvent<GNode>),
{
    bench.instant(InstantMetric::CountdownDoneRemoves, 1);
    let before = bench.before();
    done.remove(&node).await;
    let delta = bench.after(StatsMetric::DoneRemove, before);
    emit_node(bench, emit, node, CacheSource::Done, delta);
}
