pub type NodeId = u32;
pub type Tile = (u8, u8);
pub type Index = (u8, u8);

// POrd et Ord pour les CTree
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GNode {
    pub tile: Tile,
    pub index: Index,
}

impl GNode {
    pub fn new(tile: Tile, index: Index) -> GNode {
        GNode { tile, index }
    }

    pub fn tiles(self) -> impl Iterator<Item = usize> {
        None.into_iter()
    }

    // Méthode pour récupérer rapidement l'ID (u32 unique) d'un GNode
    pub fn id(&self) -> NodeId {
        ((self.tile.0 as u32) << 24)
            + ((self.tile.1 as u32) << 16)
            + ((self.index.0 as u32) << 8)
            + (self.index.1 as u32)
    }

    // Accesseurs
    pub fn tile(&self) -> Tile {
        self.tile
    }
    pub fn index(&self) -> Index {
        self.index
    }
}
