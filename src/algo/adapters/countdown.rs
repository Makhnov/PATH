// Lance Countdown avec l’état couplé ou séparé et la politique stricte ou le départage spatial, pas de métriques supplémentaires.
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
    use crate::algo::{CountdownPolicy, CountdownStateMode};
    let state = match options.state {
        "decoupled" => CountdownStateMode::Decoupled,
        _ => CountdownStateMode::Coupled,
    };
    let policy = match options.policy {
        "spatial_tiebreak" => CountdownPolicy::SpatialTieBreak,
        _ => CountdownPolicy::Strict,
    };
    AlgorithmRun::new(
        crate::algo::astar_countdown::<G, C, F, B>(graph, start, end, state, policy, emit, bench)
            .await,
    )
}
