// Définit le couplage la politique d'explo, le découpage, et le choix de la file locale
use super::queue::GlobalKey;
use crate::graph::{GNode, Tile};

pub const BUCKET_WIDTH: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketStateMode {
    Coupled,
    Decoupled,
}

impl BucketStateMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Coupled => "coupled",
            Self::Decoupled => "decoupled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketPolicy {
    Strict,
    TileAware,
    SpatialTieBreak,
}

impl BucketPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::TileAware => "tile_aware",
            Self::SpatialTieBreak => "spatial_tiebreak",
        }
    }

    pub(super) const fn uses_spatial_queue(self) -> bool {
        matches!(self, Self::SpatialTieBreak)
    }
}

#[inline]
pub(super) fn ring(estimation: u32) -> u32 {
    estimation / BUCKET_WIDTH * BUCKET_WIDTH
}

#[inline]
pub(super) fn global_key(estimation: u32, cost: u32) -> GlobalKey {
    (ring(estimation), estimation, cost)
}

#[inline]
pub(super) fn same_ring(first: GlobalKey, candidate: GlobalKey) -> bool {
    first.0 == candidate.0
}

#[inline]
pub(super) fn stays_local(
    policy: BucketPolicy,
    anchor_tile: Tile,
    active_ring: u32,
    node: GNode,
    estimation: u32,
) -> bool {
    ring(estimation) == active_ring
        || (matches!(policy, BucketPolicy::TileAware) && node.tile() == anchor_tile)
}
