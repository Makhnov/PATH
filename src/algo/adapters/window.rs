// Lance Window avec delta, max_batch et la politique spatiale, puis collecte les métriques spécifiques à son analyse
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
    use crate::algo::{
        registry::SpecificMetrics,
        window::{WindowOptions, WindowPolicy, astar_window},
    };
    let descriptor = crate::algo::registry::Algorithm::AstarWindow.descriptor();
    let options = WindowOptions::new(
        options
            .parameter("delta")
            .unwrap_or_else(|| descriptor.parameter("delta").expect("delta enregistré")),
        options
            .parameter("max_batch")
            .unwrap_or_else(|| descriptor.parameter("max_batch").expect("lot enregistré"))
            as usize,
    )
    .expect("preset valide")
    .with_policy(match options.policy {
        "spatial_priority" => WindowPolicy::SpatialPriority,
        "spatial_tiebreak" => WindowPolicy::SpatialTieBreak,
        _ => unreachable!("politique validée par le registre"),
    });
    let (result, stats) =
        astar_window::<G, C, F, B>(graph, start, end, options, emit, bench).await;
    AlgorithmRun {
        result,
        metrics: SpecificMetrics::from_values(&[
            ("window_delta", options.delta as u64),
            ("window_batch", options.max_batch as u64),
            ("tile_batches", stats.batches),
            ("tile_switches", stats.switches),
            ("tile_max_batch", stats.max_batch as u64),
            ("tile_relaxed_pops", stats.relaxed_pops),
            ("tile_stale_pops", stats.stale_pops),
            ("tile_peak_active_tiles", stats.peak_active_tiles as u64),
            ("fslack_max", stats.max_f_slack),
        ]),
    }
}
