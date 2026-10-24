// Lance A* sans options spécifiques ni métriques supplémentaires
use crate::{
    algo::registry::{AlgorithmRun, RunOptions},
    event::GpsEvent,
    format::Node,
    graph::{GNode, Graph},
    inspection::{Bench, InspectGraph},
};
use swap_cache::cache::CacheSingleton;

pub async fn run<G, C, F, B>(
    graph: &G,
    start: Node,
    end: Node,
    options: RunOptions<'_>,
    emit: F,
    bench: &mut B,
) -> AlgorithmRun<C>
where
    G: Graph<ULoc = GNode, Loc = Node> + InspectGraph,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let _ = options;
    AlgorithmRun::new(crate::algo::astar::<G, C, F, B>(graph, start, end, emit, bench).await)
}
