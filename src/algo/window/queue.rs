// Window utilise une file globale (CTree) et une fenêtre locale (RRBTree) pour gérer sa frontière
// et une map ordonnée pour les coûts et les prédécesseurs (pas de différence entre les options)

// Clefs :
// File globale : f, g et le noeud candidat (Label) = 2 * u32 + 4 * u8
// File locale : Aucune, la fenêtre RRBTree stocke les candidats par position = 0
// CTree couplé (state) : Le noeud pour lequel on stocke coût et prédécesseur (GNode) = 4 * u8

// Valeurs :
// File globale : Aucune, le noeud et les coûts sont dans la clef = 0
// File locale : f, g et le noeud candidat (Label) = 2 * u32 + 4 * u8
// CTree couplé (state) : coût g et prédécesseur (GNode) = u32 + 4 * u8

// Total clef + valeur par entrée
// File globale:        12o + 0o = 12o
// File locale:         0o + 12o = 12o
// Map state (couplé):  4o + 8o = 12o

use crate::{
    event::{CacheSource, GpsEvent},
    graph::{GNode, Tile},
    inspection::{Bench, InstantMetric, StatsMetric, emit_node},
};
use swap_cache::{
    cache::CacheSingleton,
    collections::{CTreeMap, RRBTree},
    iterator::AIterator,
};

use super::{WindowOptions, WindowPolicy, WindowStats};

pub(super) use crate::algo::registry::WINDOW_CAPACITY;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Label {
    pub estimation: u32,
    pub cost: u32,
    pub node: GNode,
}

pub(crate) struct WindowQueue<C: CacheSingleton> {
    window: RRBTree<Label, C>,
    window_len: usize,
    spill: CTreeMap<Label, (), C>,
    active: Option<Tile>,
    chosen: Option<usize>,
    batch_len: usize,
    pub stats: WindowStats,
}

pub(super) async fn queue_push<C: CacheSingleton, F: FnMut(GpsEvent<GNode>), B: Bench>(
    open_set: &mut WindowQueue<C>,
    label: Label,
    bench: &mut B,
    emit: &mut F,
) {
    let local = open_set.active_tile() == Some(label.node.tile());
    let before = bench.before();
    let inserted = open_set.push(label).await;
    let delta = bench.after(StatsMetric::HeapPush, before);
    emit_node(bench, emit, label.node, CacheSource::Heap, delta);
    if inserted {
        bench.instant(InstantMetric::HeapPushes, 1);
        bench.instant(
            if local {
                InstantMetric::LocalPushes
            } else {
                InstantMetric::GlobalPushes
            },
            1,
        );
    }
}

impl<C: CacheSingleton> WindowQueue<C> {
    pub async fn new() -> Self {
        Self {
            window: RRBTree::new().await,
            window_len: 0,
            spill: CTreeMap::new(),
            active: None,
            chosen: None,
            batch_len: 0,
            stats: WindowStats::default(),
        }
    }

    pub(crate) fn labels(&self) -> &RRBTree<Label, C> {
        &self.window
    }

    async fn best_index(&self, tile: Option<Tile>) -> Option<usize> {
        let mut best: Option<(usize, Label)> = None;
        let mut labels = self.labels().iter().enumerate();
        while let Some((index, label)) = labels.next().await {
            if tile.is_none_or(|tile| label.node.tile() == tile)
                && best.is_none_or(|(_, current)| *label < current)
            {
                best = Some((index, *label));
            }
        }
        best.map(|(index, _)| index)
    }

    async fn spill_head(&self) -> Option<Label> {
        let mut labels = self.spill.iter();
        labels.next().await.map(|(label, ())| *label)
    }

    pub fn active_tile(&self) -> Option<Tile> {
        self.active
    }

    pub async fn active_head(&self) -> Option<Label> {
        let index = match self.chosen {
            Some(index) => Some(index),
            None => match self.active {
                Some(tile) => self.best_index(Some(tile)).await,
                None => None,
            },
        }?;
        self.window.get(index).await.map(|label| *label)
    }

    pub fn len(&self) -> usize {
        self.window_len + self.global_len()
    }

    pub fn local_len(&self) -> usize {
        self.window_len
    }

    pub fn global_len(&self) -> usize {
        self.spill.len()
    }

    pub fn external_len(&self) -> usize {
        self.spill.len()
    }

    #[cfg(feature = "inspect")]
    pub(crate) fn spill(&self) -> &CTreeMap<Label, (), C> {
        &self.spill
    }

    // Inclut toutes les entrées de la fenêtre et la première entrée exacte du débordement
    // NB Les entrées obsolètes ne peuvent que sous-estimer le minimum des entrées valides
    pub async fn lower_bound(&self) -> Option<u32> {
        let local = match self.best_index(None).await {
            Some(index) => self.window.get(index).await.map(|label| label.estimation),
            None => None,
        };
        let global = self.spill_head().await.map(|label| label.estimation);
        match (local, global) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    }

    async fn spill_push(&mut self, label: Label) -> bool {
        self.spill.insert(label, ()).await.is_none()
    }

    async fn fill_window(&mut self) {
        while self.window_len < WINDOW_CAPACITY {
            let Some((head, ())) = self.spill.pop_first().await else {
                break;
            };
            self.window.push(head).await;
            self.window_len += 1;
        }
    }

    pub async fn push(&mut self, label: Label) -> bool {
        // Les entrées identiques sont interchangeables. Les améliorations strictes de g empêchent normalement
        // ce cas, mais la sémantique d’ensemble évite les débordements de l’horodatage.
        let mut duplicate = false;
        {
            let mut labels = self.labels().iter();
            while let Some(candidate) = labels.next().await {
                if *candidate == label {
                    duplicate = true;
                    break;
                }
            }
        }
        if duplicate || self.spill_head().await == Some(label) {
            return false;
        }
        let inserted;
        if self.window_len < WINDOW_CAPACITY {
            if self.spill_head().await.is_none_or(|head| label < head) {
                self.window.push(label).await;
                self.window_len += 1;
                inserted = true;
            } else {
                inserted = self.spill_push(label).await;
            }
            self.fill_window().await;
        } else {
            let worst = {
                let mut worst: Option<(usize, Label)> = None;
                let mut labels = self.labels().iter().enumerate();
                while let Some((index, candidate)) = labels.next().await {
                    if worst.is_none_or(|(_, current)| *candidate >= current) {
                        worst = Some((index, *candidate));
                    }
                }
                worst.map(|(index, _)| index).expect("full window")
            };
            let worst_label = *self.window.get(worst).await.expect("window label");
            if label < worst_label {
                let displaced = core::mem::replace(
                    &mut *self.window.get_mut(worst).await.expect("window label"),
                    label,
                );
                assert!(self.spill_push(displaced).await);
                inserted = true;
            } else {
                inserted = self.spill_push(label).await;
            }
        }
        self.chosen = None;
        inserted
    }

    #[cfg(feature = "inspect")]
    async fn observe_resident_tiles(&mut self) {
        let before = crate::inspection::SCCount::get();
        let mut tiles = std::vec::Vec::new();
        self.labels()
            .inspect_cache(async |label, _| {
                let tile = label.node.tile();
                if !tiles.contains(&tile) {
                    tiles.push(tile);
                }
            })
            .await;
        let mut head: Option<Label> = None;
        self.spill
            .inspect_cache(async |label, _, _| {
                if head.is_none_or(|current| *label < current) {
                    head = Some(*label);
                }
            })
            .await;
        if let Some(head) = head {
            let tile = head.node.tile();
            if !tiles.contains(&tile) {
                tiles.push(tile);
            }
        }
        // Compte les tuiles représentées par les candidats de la fenêtre, pas toutes les tuiles du débordement.
        self.stats.peak_active_tiles = self.stats.peak_active_tiles.max(tiles.len());
        before.restore();
    }

    // L’ordre de traitement est réévalué après chaque extraction. À la limite du lot, choisir un
    // candidat de f minimal global, pour permettre de traiter une autre tuile de même f.
    pub async fn prepare(
        &mut self,
        options: WindowOptions,
        tile_rank: impl Fn(Tile) -> usize,
    ) -> bool {
        self.fill_window().await;
        let Some(best) = self.best_index(None).await else {
            return false;
        };
        let minimum = self.lower_bound().await.expect("nonempty window");
        let local = match self.active {
            Some(tile) => self.best_index(Some(tile)).await,
            None => None,
        };
        let expired = self.batch_len >= options.max_batch;
        let eligible = match local {
            Some(index) => (self
                .window
                .get(index)
                .await
                .expect("window label")
                .estimation
                <= minimum.saturating_add(options.delta))
            .then_some(index),
            None => None,
        };
        let chosen = if options.policy == WindowPolicy::SpatialTieBreak {
            // f reste la clé principale. Pour les f égaux dans la fenêtre, suivre le rang physique
            // de la tuile (RowMajor/Hilbert/ZCurve), puis g et le noeud.
            let chosen = {
                let mut chosen: Option<(usize, (usize, u32, GNode))> = None;
                let mut labels = self.labels().iter().enumerate();
                while let Some((index, label)) = labels.next().await {
                    if label.estimation == minimum {
                        let key = (tile_rank(label.node.tile()), label.cost, label.node);
                        if chosen.is_none_or(|(_, current)| key < current) {
                            chosen = Some((index, key));
                        }
                    }
                }
                chosen.map(|(index, _)| index).expect("minimal f in window")
            };
            if expired
                || self.active
                    != Some(
                        self.window
                            .get(chosen)
                            .await
                            .expect("window label")
                            .node
                            .tile(),
                    )
            {
                self.batch_len = 0;
                self.stats.batches += 1;
            }
            chosen
        } else if !expired && let Some(index) = eligible {
            index
        } else {
            // Une limite de lot ramène au f minimal. Le départage ne garantit pas ici
            // un traitement équitable à tour de rôle de toutes les tuiles actives.
            let other = if expired {
                let mut other: Option<(usize, Label)> = None;
                let mut labels = self.labels().iter().enumerate();
                while let Some((index, label)) = labels.next().await {
                    if label.estimation == minimum
                        && Some(label.node.tile()) != self.active
                        && other.is_none_or(|(_, current)| *label < current)
                    {
                        other = Some((index, *label));
                    }
                }
                other.map(|(index, _)| index)
            } else {
                None
            };
            self.batch_len = 0;
            self.stats.batches += 1;
            other.unwrap_or(best)
        };
        let tile = self
            .window
            .get(chosen)
            .await
            .expect("window label")
            .node
            .tile();
        if self.active.is_some_and(|previous| previous != tile) {
            self.stats.switches += 1;
        }
        if self.active != Some(tile) {
            self.batch_len = 0;
        }
        self.active = Some(tile);
        self.chosen = Some(chosen);
        #[cfg(feature = "inspect")]
        self.observe_resident_tiles().await;
        true
    }

    pub async fn pop_active(&mut self) -> Option<Label> {
        let minimum = self.lower_bound().await?;
        let index = self.chosen.take()?;
        let label = *self.window.get(index).await.expect("window label");
        self.window_len -= 1;
        let last = self.window.pop().await.expect("nonempty window");
        if index < self.window_len {
            *self.window.get_mut(index).await.expect("window label") = last;
        }
        self.batch_len += 1;
        self.stats.max_batch = self.stats.max_batch.max(self.batch_len);
        let slack = label.estimation - minimum;
        self.stats.max_f_slack = self.stats.max_f_slack.max(u64::from(slack));
        self.stats.relaxed_pops += u64::from(slack > 0);
        Some(label)
    }
}
