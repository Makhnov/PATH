// Lance Bucket avec l’état couplé ou séparé et la politique stricte, tile-aware ou le départage spatial, pas de métriques supplémentaires
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
    use crate::algo::{BucketPolicy, BucketStateMode};
    let state = match options.state {
        "decoupled" => BucketStateMode::Decoupled,
        _ => BucketStateMode::Coupled,
    };
    let policy = match options.policy {
        "tile_aware" => BucketPolicy::TileAware,
        "spatial_tiebreak" => BucketPolicy::SpatialTieBreak,
        _ => BucketPolicy::Strict,
    };
    AlgorithmRun::new(
        crate::algo::astar_bucket::<G, C, F, B>(graph, start, end, state, policy, emit, bench)
            .await,
    )
}
