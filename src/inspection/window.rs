// Les captures observent les blocs SWAP résidents sans charger les blocs absents.
use crate::{
    algo::window::queue::WindowQueue,
    inspection::{Bench, InstantMetric},
};
#[cfg(feature = "inspect")]
use crate::{
    algo::window::{queue::Label, states::NodeStates},
    event::{CollectionKind, GpsEvent},
    graph::GNode,
    inspection::{
        GpsInspectors, InspectGraph, Inspector, SCCount, cache_residency,
        emit_inspection_with_entry_size,
    },
};
#[cfg(feature = "inspect")]
use core::mem::size_of;
use swap_cache::cache::CacheSingleton;

#[cfg(feature = "inspect")]
#[derive(Default)]
pub(crate) struct Snapshots {
    global: Inspector<Label>,
    local: Inspector<Label>,
    states: Inspector<GNode>,
    gps: GpsInspectors,
}

#[cfg(feature = "inspect")]
impl Snapshots {
    pub async fn emit<G: InspectGraph, C: CacheSingleton, F: FnMut(GpsEvent<GNode>)>(
        &mut self,
        open_set: &WindowQueue<C>,
        states: &NodeStates<C>,
        graph: &G,
        capacity: usize,
        emit: &mut F,
    ) {
        let before = SCCount::get();
        open_set
            .spill()
            .inspect_cache(async |label, _, rank| {
                if let Some(rank) = rank {
                    self.global.current.insert(*label);
                    self.global.observe_rank(rank);
                }
            })
            .await;
        self.global.save(open_set.spill().len());
        open_set
            .labels()
            .inspect_cache(async |label, rank| {
                if let Some(rank) = rank {
                    self.local.current.insert(*label);
                    self.local.observe_rank(rank);
                }
            })
            .await;
        self.local.save(open_set.local_len());

        states
            .0
            .inspect_cache(async |node, _, rank| {
                if let Some(rank) = rank {
                    self.states.current.insert(*node);
                    self.states.observe_rank(rank);
                }
            })
            .await;
        self.states.save(states.len());
        let entry_size = size_of::<(GNode, crate::algo::bucket::queue::NodeState)>();
        let gps = graph.inspect_gps(&mut self.gps).await;
        let ram = cache_residency(
            capacity,
            [
                (CollectionKind::Global, self.global.resident_blocks()),
                (CollectionKind::Local, self.local.resident_blocks()),
                (CollectionKind::States, self.states.resident_blocks()),
            ]
            .into_iter()
            .chain(gps.resident_collections()),
        );
        emit_inspection_with_entry_size(
            emit,
            CollectionKind::Global,
            &self.global,
            size_of::<(Label, ())>(),
            ram,
        );
        emit_inspection_with_entry_size(
            emit,
            CollectionKind::Local,
            &self.local,
            size_of::<Label>(),
            ram,
        );
        emit_inspection_with_entry_size(
            emit,
            CollectionKind::States,
            &self.states,
            entry_size,
            ram,
        );
        gps.emit(emit, ram);
        before.restore();
    }
}

pub(crate) fn maxima<C: CacheSingleton, B: Bench>(
    open_set: &WindowQueue<C>,
    states: usize,
    bench: &mut B,
) {
    bench.instant(InstantMetric::MaxHeap, open_set.len() as u64);
    bench.instant(InstantMetric::MaxGlobal, open_set.external_len() as u64);
    bench.instant(InstantMetric::MaxLocal, open_set.local_len() as u64);
    bench.instant(InstantMetric::MaxStates, states as u64);
    bench.instant(InstantMetric::MaxPrevious, states as u64);
}
