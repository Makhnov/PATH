#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum CollectionKind {
    Heap,
    Done,
    Previous,
    Global,
    Local,
    States,
    GpsMaps,
    MapStoredTiles,
    MapStoredEnds,
    MapSegments,
    PathPoints,
    PathWaypoints,
    InterestStoredTiles,
    InterestStoredEnds,
    InterestData,
    Heights,
    MapTiles,
    Path,
    Interests,
    ResultPath,
    MapTileIndex,
}

impl CollectionKind {
    pub const COUNT: usize = 21;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Heap,
        Self::Done,
        Self::Previous,
        Self::Global,
        Self::Local,
        Self::States,
        Self::GpsMaps,
        Self::MapStoredTiles,
        Self::MapStoredEnds,
        Self::MapSegments,
        Self::PathPoints,
        Self::PathWaypoints,
        Self::InterestStoredTiles,
        Self::InterestStoredEnds,
        Self::InterestData,
        Self::Heights,
        Self::MapTiles,
        Self::Path,
        Self::Interests,
        Self::ResultPath,
        Self::MapTileIndex,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Heap => "heap",
            Self::Done => "done",
            Self::Previous => "previous",
            Self::Global => "global",
            Self::Local => "local",
            Self::States => "states",
            Self::GpsMaps => "gps_maps",
            Self::MapStoredTiles => "map_stored_tiles",
            Self::MapStoredEnds => "map_stored_ends",
            Self::MapSegments => "map_segments",
            Self::PathPoints => "path_points",
            Self::PathWaypoints => "path_waypoints",
            Self::InterestStoredTiles => "interest_stored_tiles",
            Self::InterestStoredEnds => "interest_stored_ends",
            Self::InterestData => "interest_data",
            Self::Heights => "heights",
            Self::MapTiles => "map_tiles",
            Self::Path => "path",
            Self::Interests => "interests",
            Self::ResultPath => "result_path",
            Self::MapTileIndex => "map_tile_index",
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionAction {
    Push,
    Pop,
    Insert,
    Get,
    Contains,
    Update,
    Remove,
}

impl CollectionAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Push => "push",
            Self::Pop => "pop",
            Self::Insert => "insert",
            Self::Get => "get",
            Self::Contains => "contains",
            Self::Update => "update",
            Self::Remove => "remove",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheSource {
    Heap,
    Done,
    Previous,
    Global,
    Local,
    States,
    Gps,
    Rebuild,
}

impl CacheSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Heap => "heap",
            Self::Done => "done",
            Self::Previous => "previous",
            Self::Global => "global",
            Self::Local => "local",
            Self::States => "states",
            Self::Gps => "gps",
            Self::Rebuild => "rebuild",
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CacheDelta {
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub flash_reads: u64,
    pub flash_writes: u64,
    pub flash_allocs: u64,
    pub flash_frees: u64,
    pub flash_erases: u64,
    pub errors_io: u64,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CollectionCacheState {
    pub entries_total: usize,
    pub entries_cached: usize,
    pub entries_uncached: usize,
    pub entry_size: usize,
    // Estimation logique historique fondée sur les entrées utilisateur résidentes (débranché !!!)
    pub estimated_cached_bytes: usize,
    // Résidence réelle dans swap-cache pour cette collection.
    pub resident_blocks: usize,
    pub resident_bytes: usize,
    pub kept_entries: usize,
    pub entered_entries: usize,
    pub exited_entries: usize,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RamCacheState {
    // Taille du cache RAM en blocs et en octets
    pub cache_capacity_blocks: usize,
    pub cache_capacity_bytes: usize,
    // Total des blocs en RAM repérés par l'inspection (sans doublon)
    pub observed_resident_blocks: usize,
    pub observed_resident_bytes: usize,
    // Nombre de blocs en RAM par collection (idem)
    pub observed_resident_blocks_by_collection: [usize; CollectionKind::COUNT],
    // Capacité du cache moins les octets repérés. TODO quid du reste dans les runs contraints ?
    pub unobserved_capacity_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
// Utiliser un Box comme le suggère clippy ajouterait des allocations due à l'instrumentation (et non à l'algo)
#[allow(clippy::large_enum_variant)]
pub enum GpsEvent<N> {
    TileIndexSnapshot,
    TileIndexSpan {
        tile: crate::graph::Tile,
        len: u8,
        rank: Option<usize>,
    },
    Started {
        start: N,
        end: N,
    },
    Pushed {
        node: N,
        cost: u32,
        estimation: u32,
    },
    Popped {
        node: N,
        cost: u32,
        estimation: u32,
    },
    Settled {
        node: N,
        cost: u32,
    },
    Relaxed {
        from: N,
        to: N,
        edge_cost: u32,
        new_cost: u32,
        estimation: u32,
    },
    PathNode {
        node: N,
    },
    CacheNode {
        node: N,
        source: CacheSource,
        tick: u64,
        delta: CacheDelta,
    },
    Collection {
        collection: CollectionKind,
        action: CollectionAction,
        changed: bool,
        len_before: usize,
        len_after: usize,
        cache_delta: CacheDelta,
        cache_state: Option<CollectionCacheState>,
        ram_state: Option<RamCacheState>,
    },
    Finished {
        found: bool,
        path_len: usize,
    },
}
