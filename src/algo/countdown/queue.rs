// Countdown utilise une file principale pour gérer sa frontière qui prend deux formes différentes selon l'option de zonage (stricte ou départage)
// Et selon l'option de couplage, deux (couplé) ou trois (découplé) maps ordonnées pour les coûts, les prédécesseurs et les noeuds traités (done)

// Clefs :
// File stricte : f, g et compteur d'insertion = 2 * u32 + u16
// File départage couplée  : f, g, noeud candidat (GNode) et compteur d'insertion = 2 * u32 + 4 * u8 + u16
// File départage découplée : f, g et noeud candidat (GNode) = 2 * u32 + 4 * u8

// CTree costs (découplé) : Le noeud candidat dont on stocke le coût et le compteur incoming (GNode) = 4 * u8
// CTree done : Le noeud traité dont on stocke le compteur restant (GNode) = 4 * u8
// CTree previous : Le noeud dont on stocke le prédécesseur (GNode) = 4 * u8

// Valeurs :
// File couplée : Le noeud candidat et son prédécesseur (Step) = 2 * (4 * u8)
// File découplée stricte : Le noeud (GNode) candidat = 4 * u8
// File découplée départage : Aucune, le noeud est dans la clef = 0
// CTree découplé costs : Coût g et nombre d'arcs déjà parcourus vers ce noeud (incoming) = u32 + u8
// CTree done : Compteur restant avant retrait de l'entrée done = u8
// CTree previous : Prédécesseur (GNode) = 4 * u8

// Total clef + valeur par entrée
// File couplée stricte:        8o + 2o + 8o =  18o
// File couplée départage:      12o + 2o + 8o = 22o
// File découplée stricte:      8o + 2o + 4o =  14o
// File découplée départage:    12o + 0o =      12o
// Map done (couplé et découplé):       4o + 1o = 5o
// Map costs (découplé):               4o + 5o = 9o
// Map previous (couplé et découplé):   4o + 4o = 8o

use super::CountdownPolicy;
use crate::{
    algo::utils::node_queue::NodeQueue,
    event::{CacheSource, GpsEvent},
    graph::GNode,
    inspection::{Bench, InstantMetric, StatsMetric, emit_node},
};
#[cfg(feature = "inspect")]
use core::mem::size_of;
use swap_cache::{cache::CacheSingleton, collections::PriorityQueue};

// Entrée de la file couplée: Le prédécesseur accompagne le noeud
#[derive(Debug, Clone, Copy, Hash)]
pub(crate) struct Step {
    pub(crate) node: GNode,
    pub(super) prev: GNode,
}

// Frontière couplée
// done: CTreeMap<GNode, u8>
// previous: CTreeMap<GNode, GNode>.
pub(crate) enum CoupledQueue<C: CacheSingleton> {
    Strict(PriorityQueue<(u32, u32), Step, C>),
    Spatial(PriorityQueue<(u32, u32, GNode), Step, C>),
}

// État découplé
// costs: CTreeMap<GNode, LiveCost>.
#[derive(Clone, Copy, Debug, Hash)]
pub(crate) struct LiveCost {
    pub(super) cost: u32,
    pub(super) incoming: u8,
}

// Frontière découplée
// costs: CTreeMap<GNode, LiveCost>
// done: CTreeMap<GNode, u8>
// previous: CTreeMap<GNode, GNode>.
pub(crate) enum DecoupledQueue<C: CacheSingleton> {
    Strict(PriorityQueue<(u32, u32), GNode, C>),
    Spatial(NodeQueue<(u32, u32), C>),
}

// Insérer une entrée couplée et mesurer l’opération sur le tas.
pub(super) async fn queue_push_coupled<C, F, B>(
    queue: &mut CoupledQueue<C>,
    key: (u32, u32),
    step: Step,
    bench: &mut B,
    emit: &mut F,
) where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    queue.push(key, step).await;
    let delta = bench.after(StatsMetric::HeapPush, before);
    emit_node(bench, emit, step.node, CacheSource::Heap, delta);
    bench.instant(InstantMetric::HeapPushes, 1);
}

// Mesurer chaque tentative d’extraction ; émettre et compter uniquement les entrées retournées.
pub(super) async fn queue_pop_coupled<C, F, B>(
    queue: &mut CoupledQueue<C>,
    bench: &mut B,
    emit: &mut F,
) -> Option<((u32, u32), Step)>
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    let popped = queue.pop_first().await;
    let delta = bench.after(StatsMetric::HeapPop, before);
    if let Some((_, step)) = &popped {
        emit_node(bench, emit, step.node, CacheSource::Heap, delta);
        bench.instant(InstantMetric::HeapPops, 1);
    }
    popped
}

// Insérer une entrée découplée et mesurer l’opération sur le tas
pub(super) async fn queue_push_decoupled<C, F, B>(
    queue: &mut DecoupledQueue<C>,
    key: (u32, u32),
    node: GNode,
    bench: &mut B,
    emit: &mut F,
) where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    queue.push(key, node).await;
    let delta = bench.after(StatsMetric::HeapPush, before);
    emit_node(bench, emit, node, CacheSource::Heap, delta);
    bench.instant(InstantMetric::HeapPushes, 1);
}

// Mesurer chaque tentative d’extraction ; émettre et compter uniquement les entrées retournées.
pub(super) async fn queue_pop_decoupled<C, F, B>(
    queue: &mut DecoupledQueue<C>,
    bench: &mut B,
    emit: &mut F,
) -> Option<((u32, u32), GNode)>
where
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    let popped = queue.pop_first().await;
    let delta = bench.after(StatsMetric::HeapPop, before);
    if let Some((_, node)) = &popped {
        emit_node(bench, emit, *node, CacheSource::Heap, delta);
        bench.instant(InstantMetric::HeapPops, 1);
    }
    popped
}

impl<C: CacheSingleton> CoupledQueue<C> {
    pub(crate) fn new(policy: CountdownPolicy) -> Self {
        match policy {
            CountdownPolicy::Strict => Self::Strict(PriorityQueue::new()),
            CountdownPolicy::SpatialTieBreak => Self::Spatial(PriorityQueue::new()),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Strict(queue) => queue.len(),
            Self::Spatial(queue) => queue.len(),
        }
    }

    pub(crate) async fn push(&mut self, key: (u32, u32), step: Step) {
        match self {
            Self::Strict(queue) => queue.push(key, step).await,
            Self::Spatial(queue) => queue.push((key.0, key.1, step.node), step).await,
        }
    }

    pub(crate) async fn pop_first(&mut self) -> Option<((u32, u32), Step)> {
        match self {
            Self::Strict(queue) => queue.pop_first().await,
            Self::Spatial(queue) => queue
                .pop_first()
                .await
                .map(|((f, g, _), step)| ((f, g), step)),
        }
    }

    #[cfg(feature = "inspect")]
    pub(crate) fn entry_size(&self) -> usize {
        match self {
            Self::Strict(_) => size_of::<(((u32, u32), u16), Step)>(),
            Self::Spatial(_) => size_of::<(((u32, u32, GNode), u16), Step)>(),
        }
    }

    #[cfg(feature = "inspect")]
    pub(crate) async fn inspect_cache<F: AsyncFnMut(&Step, Option<usize>)>(&self, mut emit: F) {
        match self {
            Self::Strict(queue) => {
                queue
                    .inspect_cache(async |_, step, rank| emit(step, rank).await)
                    .await
            }
            Self::Spatial(queue) => {
                queue
                    .inspect_cache(async |_, step, rank| emit(step, rank).await)
                    .await
            }
        }
    }
}

impl<C: CacheSingleton> DecoupledQueue<C> {
    pub(crate) fn new(policy: CountdownPolicy) -> Self {
        match policy {
            CountdownPolicy::Strict => Self::Strict(PriorityQueue::new()),
            CountdownPolicy::SpatialTieBreak => Self::Spatial(NodeQueue::new()),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Strict(queue) => queue.len(),
            Self::Spatial(queue) => queue.len(),
        }
    }

    pub(crate) async fn push(&mut self, key: (u32, u32), node: GNode) {
        match self {
            Self::Strict(queue) => queue.push(key, node).await,
            Self::Spatial(queue) => queue.push(key, node).await,
        }
    }

    pub(crate) async fn pop_first(&mut self) -> Option<((u32, u32), GNode)> {
        match self {
            Self::Strict(queue) => queue.pop_first().await,
            Self::Spatial(queue) => queue.pop_first().await,
        }
    }

    #[cfg(feature = "inspect")]
    pub(crate) fn entry_size(&self) -> usize {
        match self {
            Self::Strict(_) => size_of::<(((u32, u32), u16), GNode)>(),
            Self::Spatial(_) => size_of::<(((u32, u32), GNode), ())>(),
        }
    }

    #[cfg(feature = "inspect")]
    pub(crate) async fn inspect_cache<F: AsyncFnMut(&GNode, Option<usize>)>(&self, mut emit: F) {
        match self {
            Self::Strict(queue) => {
                queue
                    .inspect_cache(async |_, node, rank| emit(node, rank).await)
                    .await
            }
            Self::Spatial(queue) => {
                queue
                    .inspect_cache(async |node, rank| emit(node, rank).await)
                    .await
            }
        }
    }
}
