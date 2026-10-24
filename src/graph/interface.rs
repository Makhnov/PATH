use super::Edge;
use crate::Result;
use crate::graph::Tile;

use core::future::Future;
use swap_cache::iterator::AIterator;

pub type RawTile = usize;
pub type RawSegment = [u8; 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileOrder {
    RowMajor,
    Morton,
    Hilbert,
}

// #[allow(async_fn_in_trait)]
pub trait Graph {
    type ULoc: Copy + Eq;
    type Loc: Copy + PartialOrd;

    fn heuristic(&self, from: Self::ULoc, to: Self::ULoc) -> u32;

    fn heuristile(&self, from: Tile, to: Tile) -> u32;

    fn tile_rank(&self, tile: Tile) -> usize;

    fn position(&self, node: Self::ULoc) -> Option<Self::Loc>;

    fn edges(
        &self,
        tile: Tile,
        gnode: Self::ULoc,
    ) -> impl Future<Output = Result<impl AIterator<Lend = Edge<Self::ULoc>> + '_>> + '_;

    fn segments(
        &self,
        tile: Tile,
    ) -> impl Future<Output = Result<impl AIterator<Lend = RawSegment> + '_>> + '_;

    fn neighbors(
        &self,
        node: Self::ULoc,
    ) -> impl Future<Output = Result<impl AIterator<Lend = Edge<Self::ULoc>> + '_>> + '_;

    fn nearest_gnode(
        &self,
        node: Self::Loc,
    ) -> impl Future<Output = Result<Option<Self::ULoc>>> + '_;

    // async fn edges(&self, tile: Tile) -> Result<impl AIterator<Lend = Edge<Self::ULoc>> + '_>;
    // async fn segments(&self, tile: Tile) -> Result<impl AIterator<Lend = RawSegment> + '_>;
    // async fn neighbors(&self, node: Self::ULoc) -> Result<impl AIterator<Lend = Edge<Self::ULoc>> + '_>;
    // async fn nearest_gnode(&self, node: Self::Loc) -> Result<Option<Self::ULoc>>;
}
