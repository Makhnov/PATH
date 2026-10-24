// Nouvelle collection out-of-core qui utilise un index de 4o par tuile pour retrouver directement les segments qu'elle contient
// Chaque entrée contient la position du premier segment sur 24 bits et le nombre de segments sur 8 bits
use core::mem::size_of;
use swap_cache::{
    cache::CacheSingleton,
    collections::RRBTree,
    iterator::{AIterator, IntoAIterator},
};

use super::{Edge, GNode, Graph, RawSegment, Tile, UGraph, adapter::edges};
use crate::{Error, Result, format::Node};

// Méthode d’accès aux tuiles (indépendante de l’organisation physique des données GPS)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphStorage {
    Dichotomic,
    Indexed,
}

impl GraphStorage {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "tile_dicho" => Some(Self::Dichotomic),
            "tile_index" => Some(Self::Indexed),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dichotomic => "tile_dicho",
            Self::Indexed => "tile_index",
        }
    }
}

fn owner_tiles(node: GNode) -> ([Tile; 4], usize) {
    let (xt, yt) = node.tile;
    let (xi, yi) = node.index;
    let mut owners = [(0, 0); 4];
    owners[0] = (xt, yt);
    let mut len = 1;
    if yi == 0 && yt != 0 {
        owners[len] = (xt, yt - 1);
        len += 1;
    }
    if xi == 0 && xt != 0 {
        owners[len] = (xt - 1, yt);
        len += 1;
    }
    if xi == 0 && yi == 0 && xt != 0 && yt != 0 {
        owners[len] = (xt - 1, yt - 1);
        len += 1;
    }
    (owners, len)
}

pub(super) const FALLBACK: u32 = u32::MAX;
const MAX_BEGIN: usize = 0x00ff_ffff;
const MAX_LEN: usize = u8::MAX as usize;

pub type TileIndexBuildError = Error;

pub struct TileIndexedGraph<'g, 'gps, C: CacheSingleton> {
    pub(super) source: &'g UGraph<'gps, C>,
    pub(super) entries: RRBTree<u32, C>,
    fallback_tiles: usize,
}

impl<'g, 'gps, C: CacheSingleton> TileIndexedGraph<'g, 'gps, C> {
    pub async fn new(source: &'g UGraph<'gps, C>) -> Result<Self> {
        let (width, height) = source.map.dimensions.grid_size;
        if width == 0 || height == 0 || width > u8::MAX as usize || height > u8::MAX as usize {
            return Err("tile index requires a 1..=255 tile grid");
        }
        let tiles = width * height;
        let mut entries = RRBTree::<u32, C>::new().await;
        for _ in 0..tiles {
            entries.push(0).await;
        }
        let stored = &source.map.offsets.stored_tiles;
        let ends = &source.map.offsets.stored_tiles_ends;
        let mut begin = 0;
        let mut fallback_tiles = 0;

        // Remplit l'index en parcourant les tuiles dans leur ordre de stockage
        let mut tiles_iter = stored.iter().copied();
        let mut ends_iter = ends.iter().copied();
        for _ in 0..stored.len() {
            let tile = tiles_iter
                .next()
                .await
                .ok_or("tile index source: missing tile offset")?;
            if tile >= tiles {
                return Err("stored tile lies outside tile directory");
            }
            let end = ends_iter
                .next()
                .await
                .ok_or("tile index source: missing tile offset")?;
            let len = end
                .checked_sub(begin)
                .ok_or("tile index source: invalid tile segment bounds")?;

            // Si le u32 ne suffit pas on revient sur la recherche dichotomique
            let packed = if begin <= MAX_BEGIN && len <= MAX_LEN {
                ((begin as u32) << 8) | len as u32
            } else {
                FALLBACK
            };

            fallback_tiles += usize::from(packed == FALLBACK);
            let mut entry = entries
                .get_mut(tile)
                .await
                .expect("entries initialized for every geographic tile");
            if *entry != 0 {
                return Err("duplicate stored tile");
            }
            *entry = packed;
            begin = end;
        }
        Ok(Self {
            source,
            entries,
            fallback_tiles,
        })
    }

    pub fn tile_count(&self) -> usize {
        self.entries.len()
    }

    pub fn entry_size(&self) -> usize {
        size_of::<u32>()
    }

    pub fn fallback_tile_count(&self) -> usize {
        self.fallback_tiles
    }

    // Recherche dichotomique pour les bornes de segments dépassant les limites de l’index compact
    pub(super) async fn dichotomic_segment_bounds(&self, tile: Tile) -> Result<(usize, usize)> {
        let target_rank = self.source.tile_rank(tile);
        let stored = &self.source.map.offsets.stored_tiles;
        let mut left = 0;
        let mut right = stored.len();
        while left < right {
            let middle = left + (right - left) / 2;
            let candidate = *stored.get(middle).await.ok_or("missing tile offset")?;
            let rank = self.source.tile_rank(self.source.cartesian_tile(candidate));
            match rank.cmp(&target_rank) {
                core::cmp::Ordering::Less => left = middle + 1,
                core::cmp::Ordering::Greater => right = middle,
                core::cmp::Ordering::Equal => {
                    let ends = &self.source.map.offsets.stored_tiles_ends;
                    let begin = match middle.checked_sub(1) {
                        Some(previous) => *ends.get(previous).await.ok_or("missing tile offset")?,
                        None => 0,
                    };
                    let end = *ends.get(middle).await.ok_or("missing tile offset")?;
                    if end < begin {
                        return Err("invalid tile segment bounds");
                    }
                    return Ok((begin, end));
                }
            }
        }
        Ok((0, 0))
    }
}

impl<C: CacheSingleton> Graph for TileIndexedGraph<'_, '_, C> {
    type ULoc = GNode;
    type Loc = Node;

    fn heuristic(&self, from: GNode, to: GNode) -> u32 {
        self.source.heuristic(from, to)
    }

    fn heuristile(&self, from: Tile, to: Tile) -> u32 {
        self.source.heuristile(from, to)
    }

    fn tile_rank(&self, tile: Tile) -> usize {
        self.source.tile_rank(tile)
    }

    fn position(&self, node: GNode) -> Option<Node> {
        self.source.position(node)
    }

    async fn segments(&self, tile: Tile) -> Result<impl AIterator<Lend = RawSegment> + '_> {
        self.source.segments(tile).await
    }

    async fn edges(
        &self,
        tile: Tile,
        gnode: GNode,
    ) -> Result<impl AIterator<Lend = Edge<GNode>> + '_> {
        let segments = self.indexed_segments(tile).await?;
        Ok(edges(self.source, tile, gnode, segments))
    }

    async fn neighbors(&self, node: GNode) -> Result<impl AIterator<Lend = Edge<GNode>> + '_> {
        let (owners, len) = owner_tiles(node);
        let mut edge_iters = [None, None, None, None];
        for (slot, owner) in edge_iters.iter_mut().zip(owners).take(len) {
            *slot = Some(self.edges(owner, node).await?);
        }
        Ok(edge_iters
            .into_iter()
            .flatten()
            .to_async()
            .flat_map(async move |edges| {
                edges.flat_map(async move |mut edge| {
                    // Oriente chaque arête depuis le noeud dont on cherche les voisins
                    if edge.neighbor == node {
                        core::mem::swap(&mut edge.node, &mut edge.neighbor);
                    }
                    (edge.node == node).then_some(edge).into_iter().to_async()
                })
            }))
    }

    async fn nearest_gnode(&self, node: Node) -> Result<Option<GNode>> {
        self.source.nearest_gnode(node).await
    }
}

#[cfg(feature = "inspect")]
impl<C: CacheSingleton> crate::inspection::InspectGraph for TileIndexedGraph<'_, '_, C> {
    async fn inspect_gps(
        &self,
        inspectors: &mut crate::inspection::GpsInspectors,
    ) -> crate::inspection::GpsInspection {
        use crate::inspection::TileIndexSpan;
        let mut snapshot = self.source.inspect_gps(inspectors).await;
        let cache = &mut inspectors.map_tile_index;
        let mut spans = std::vec::Vec::<TileIndexSpan>::new();
        let mut entry = 0;
        self.entries
            .inspect_cache(async |_, rank| {
                if let Some(rank) = rank {
                    cache.current.insert(entry);
                    cache.observe_rank(rank);
                }
                let tile = self.source.cartesian_tile(entry);
                if let Some(last) = spans.last_mut()
                    && last.rank == rank
                    && last.tile.1 == tile.1
                    && last.tile.0 as usize + last.len as usize == tile.0 as usize
                {
                    last.len += 1;
                } else {
                    spans.push(TileIndexSpan { tile, len: 1, rank });
                }
                entry += 1;
            })
            .await;
        cache.save(self.entries.len());
        snapshot.add_tile_index(cache, spans);
        snapshot
    }
}
