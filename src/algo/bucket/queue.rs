// Bucket utilise deux files principales BucketQueue (globale et locale) pour gérer sa frontière
// et une ou deux maps ordonnées pour les coûts et les prédécesseurs.

// Clefs :
// File globale : bucket courant, f, g = 3 * u32
// File locale : f, g = 2 * u32
// CTree couplé (state) : Le noeud pour lequel on stocke coût et prédécesseur (GNode) = 4 * u8
// CTree découplé cost : Le noeud dont on stocke le coût (GNode) = 4 * u8
// CTree découplé previous : Le noeud dont on stocke le prédécesseur (GNode) = 4 * u8

// Valeurs :
// Files globale et locale : Le noeud (GNode) candidat = 4 * u8
// CTree couplé (state) : coût g et prédécesseur (GNode) = u32 + 4 * u8
// CTree découplé (costs) : coût g = u32
// CTree découplé (previous) : prédécesseur (GNode) = 4 * u8

// Total clef + valeur par entrée
// File globale:    16o
// File locale:     12o
// Map (couplé):    12o
// Maps (découplé): 8o + 8o

use crate::{
    algo::bucket::BucketPolicy,
    algo::utils::node_queue::NodeQueue,
    event::{CacheSource, CollectionKind, GpsEvent},
    graph::GNode,
    inspection::{Bench, InstantMetric, StatsMetric, emit_node},
};
use core::hash::Hash;
#[cfg(feature = "inspect")]
use core::mem::size_of;
use swap_cache::{cache::CacheSingleton, collections::PriorityQueue};

// Priorité de la file globale
// (ring(estimation), estimation, cost).
pub(crate) type GlobalKey = (u32, u32, u32);

// Priorité de la file locale
// (estimation, cost).
pub(crate) type LocalKey = (u32, u32);

// État couplé
// states: CTreeMap<GNode, NodeState>.
#[derive(Debug, Clone, Copy, Hash)]
pub(crate) struct NodeState {
    pub(crate) cost: u32,
    pub(crate) previous: GNode,
}

// Frontière commune
// K: GlobalKey ou LocalKey.
pub(crate) enum BucketQueue<K: Ord, C: CacheSingleton> {
    // Ordre de priorité pour les politiques Strict et TileAware.
    Priority(PriorityQueue<K, GNode, C>),
    // Les priorités égales sont départagées par noeud pour SpatialTieBreak.
    Spatial(NodeQueue<K, C>),
}

// Insérer une entrée et mesurer l’opération sur la file globale ou locale.
pub(super) async fn queue_push<K, C, F, B>(
    queue: &mut BucketQueue<K, C>,
    key: K,
    node: GNode,
    collection: CollectionKind,
    metric: StatsMetric,
    bench: &mut B,
    emit: &mut F,
) where
    K: Ord + core::hash::Hash,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    queue.push(key, node).await;
    let delta = bench.after(metric, before);
    let source = match collection {
        CollectionKind::Global => CacheSource::Global,
        CollectionKind::Local => CacheSource::Local,
        _ => return,
    };
    emit_node(bench, emit, node, source, delta);

    bench.instant(InstantMetric::HeapPushes, 1);
    match collection {
        CollectionKind::Global => bench.instant(InstantMetric::GlobalPushes, 1),
        CollectionKind::Local => bench.instant(InstantMetric::LocalPushes, 1),
        _ => {}
    }
}

// Mesurer chaque tentative d’extraction ; émettre et compter uniquement les entrées retournées.
pub(super) async fn queue_pop<K, C, F, B>(
    queue: &mut BucketQueue<K, C>,
    collection: CollectionKind,
    metric: StatsMetric,
    bench: &mut B,
    emit: &mut F,
) -> Option<(K, GNode)>
where
    K: Ord + core::hash::Hash,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{
    let before = bench.before();
    let popped = queue.pop_first().await;
    let delta = bench.after(metric, before);

    if let Some((_, node)) = &popped {
        let source = match collection {
            CollectionKind::Global => CacheSource::Global,
            CollectionKind::Local => CacheSource::Local,
            _ => return popped,
        };
        emit_node(bench, emit, *node, source, delta);
    }

    if popped.is_some() {
        match collection {
            CollectionKind::Global => bench.instant(InstantMetric::GlobalPops, 1),
            CollectionKind::Local => bench.instant(InstantMetric::LocalPops, 1),
            _ => {}
        }
    }
    popped
}

// Les opérations de la file sont déléguées à la collection choisie par la politique.
impl<K: Ord + Hash, C: CacheSingleton> BucketQueue<K, C> {
    pub(super) fn new(policy: BucketPolicy) -> Self {
        if policy.uses_spatial_queue() {
            Self::Spatial(NodeQueue::new())
        } else {
            Self::Priority(PriorityQueue::new())
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Priority(queue) => queue.len(),
            Self::Spatial(queue) => queue.len(),
        }
    }

    pub(super) async fn push(&mut self, key: K, node: GNode) {
        match self {
            Self::Priority(queue) => queue.push(key, node).await,
            Self::Spatial(queue) => queue.push(key, node).await,
        }
    }

    pub(super) async fn pop_first(&mut self) -> Option<(K, GNode)> {
        match self {
            Self::Priority(queue) => queue.pop_first().await,
            Self::Spatial(queue) => queue.pop_first().await,
        }
    }

    #[cfg(feature = "inspect")]
    pub(crate) fn entry_size(&self) -> usize {
        match self {
            Self::Priority(_) => size_of::<((K, u16), GNode)>(),
            Self::Spatial(_) => size_of::<((K, GNode), ())>(),
        }
    }

    #[cfg(feature = "inspect")]
    pub(crate) async fn inspect_cache<F: AsyncFnMut(&GNode, Option<usize>)>(&self, mut emit: F) {
        match self {
            Self::Priority(queue) => {
                queue
                    .inspect_cache(async |_, node, rank| emit(node, rank).await)
                    .await;
            }
            Self::Spatial(queue) => {
                queue
                    .inspect_cache(async |node, rank| emit(node, rank).await)
                    .await
            }
        }
    }
}
