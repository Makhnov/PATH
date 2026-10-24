#[cfg(feature = "inspect")]
use core::sync::atomic::Ordering;

use crate::{
    event::{CacheDelta, CacheSource, GpsEvent},
    graph::GNode,
};
#[cfg(feature = "inspect")]
use crate::{
    event::{CollectionAction, CollectionCacheState, CollectionKind, RamCacheState},
    format::{Node, interests::RawInterest},
    graph::{RawSegment, Tile},
};

use embassy_time::Instant as TimeInstant;
use swap_cache::{cache::CacheSingleton, collections::RRBTree};

#[cfg(feature = "inspect")]
use core::mem::size_of;
#[cfg(feature = "inspect")]
use std::collections::BTreeSet;

pub const PROFILE_DIJKSTRA: u64 = 1;
pub const PROFILE_ASTAR: u64 = 2;

pub const FIELDS: [&str; 18] = [
    "cache_hits",
    "cache_misses",
    "flash_reads",
    "flash_writes",
    "flash_allocs",
    "flash_frees",
    "flash_erases",
    "cache_total_micros",
    "cache_alloc_micros",
    "cache_free_micros",
    "cache_used_micros",
    "cache_unused_micros",
    "flash_reads_micros",
    "flash_writes_micros",
    "btree_micros",
    "heap_micros",
    "rrb_micros",
    "errors_io",
];

pub const INSTANT_METRICS: &[InstantMetric] = &[
    InstantMetric::Profile,
    InstantMetric::Found,
    InstantMetric::Cost,
    InstantMetric::PathLen,
    InstantMetric::DataMicros,
    InstantMetric::GraphMicros,
    InstantMetric::SearchMicros,
    InstantMetric::RebuildMicros,
    InstantMetric::HeapPushes,
    InstantMetric::HeapPops,
    InstantMetric::NodesSkipped,
    InstantMetric::NodesVisited,
    InstantMetric::NeighborsSkipped,
    InstantMetric::NeighborsVisited,
    InstantMetric::EdgesVisited,
    InstantMetric::HeuristicCalls,
    InstantMetric::MaxHeap,
    InstantMetric::MaxDone,
    InstantMetric::MaxPrevious,
    InstantMetric::GlobalPushes,
    InstantMetric::GlobalPops,
    InstantMetric::LocalPushes,
    InstantMetric::LocalPops,
    InstantMetric::StatesReads,
    InstantMetric::StatesWrites,
    InstantMetric::MaxGlobal,
    InstantMetric::MaxLocal,
    InstantMetric::MaxStates,
    InstantMetric::CountdownDoneChecks,
    InstantMetric::CountdownDoneInserts,
    InstantMetric::CountdownDoneDecs,
    InstantMetric::CountdownDoneRemoves,
    InstantMetric::CountdownScans,
    InstantMetric::CountdownEdges,
    InstantMetric::PoppedSameTile,
    InstantMetric::PoppedCrossTile,
    InstantMetric::PoppedTileJumpSum,
    InstantMetric::PoppedTileJumpMax,
    InstantMetric::SkippedSameTile,
    InstantMetric::SkippedCrossTile,
    InstantMetric::SkippedTileJumpSum,
    InstantMetric::SkippedTileJumpMax,
    InstantMetric::VisitedSameTile,
    InstantMetric::VisitedCrossTile,
    InstantMetric::VisitedTileJumpSum,
    InstantMetric::VisitedTileJumpMax,
    InstantMetric::HeapPushSameTileCalls,
    InstantMetric::HeapPushCrossTileCalls,
    InstantMetric::NeighborsNextSameTileCalls,
    InstantMetric::NeighborsNextCrossTileCalls,
    InstantMetric::NeighborsSameTileCalls,
    InstantMetric::NeighborsCrossLeftCalls,
    InstantMetric::NeighborsCrossBottomCalls,
    InstantMetric::NeighborsCrossBottomLeftCalls,
    InstantMetric::EdgesFromSameNode,
    InstantMetric::EdgesFromLeftNode,
    InstantMetric::EdgesFromBottomNode,
    InstantMetric::EdgesFromBottomLeftNode,
];

pub const STATS_METRICS: &[StatsMetric] = &[
    StatsMetric::HeapPush,
    StatsMetric::HeapPop,
    StatsMetric::DoneGet,
    StatsMetric::DoneInsert,
    StatsMetric::DoneRemove,
    StatsMetric::PreviousGet,
    StatsMetric::PreviousInsert,
    StatsMetric::GlobalPush,
    StatsMetric::GlobalPop,
    StatsMetric::LocalPush,
    StatsMetric::LocalPop,
    StatsMetric::StatesGet,
    StatsMetric::StatesInsert,
    StatsMetric::StatesRemove,
    StatsMetric::NeighborsOpen,
    StatsMetric::NeighborsNext,
    StatsMetric::Rebuild,
];

#[derive(Default, Debug, Clone, Copy)]
pub struct SCCount {
    pub cache_hits: u64,
    pub cache_misses: u64,

    pub flash_reads: u64,
    pub flash_writes: u64,
    pub flash_allocs: u64,
    pub flash_frees: u64,
    pub flash_erases: u64,

    pub cache_total_micros: u64,
    pub cache_alloc_micros: u64,
    pub cache_free_micros: u64,
    pub cache_used_micros: u64,
    pub cache_unused_micros: u64,

    pub flash_reads_micros: u64,
    pub flash_writes_micros: u64,

    pub btree_micros: u64,
    pub heap_micros: u64,
    pub rrb_micros: u64,

    pub errors_io: u64,
}

impl SCCount {
    pub const ZERO: Self = Self {
        cache_hits: 0,
        cache_misses: 0,

        flash_reads: 0,
        flash_writes: 0,
        flash_allocs: 0,
        flash_frees: 0,
        flash_erases: 0,

        cache_total_micros: 0,
        cache_alloc_micros: 0,
        cache_free_micros: 0,
        cache_used_micros: 0,
        cache_unused_micros: 0,

        flash_reads_micros: 0,
        flash_writes_micros: 0,

        btree_micros: 0,
        heap_micros: 0,
        rrb_micros: 0,

        errors_io: 0,
    };

    pub fn values(&self) -> [u64; 18] {
        [
            self.cache_hits,
            self.cache_misses,
            self.flash_reads,
            self.flash_writes,
            self.flash_allocs,
            self.flash_frees,
            self.flash_erases,
            self.cache_total_micros,
            self.cache_alloc_micros,
            self.cache_free_micros,
            self.cache_used_micros,
            self.cache_unused_micros,
            self.flash_reads_micros,
            self.flash_writes_micros,
            self.btree_micros,
            self.heap_micros,
            self.rrb_micros,
            self.errors_io,
        ]
    }

    pub fn visit_names<F: FnMut(&'static str)>(mut visit: F) {
        for field in FIELDS {
            visit(field);
        }
    }

    pub fn visit_values<F: FnMut(u64)>(&self, mut visit: F) {
        for field in self.values() {
            visit(field);
        }
    }

    // Capture les compteurs actuels de swap-cache
    pub fn get() -> Self {
        #[cfg(feature = "inspect")]
        {
            Self {
                cache_hits: swap_cache::STATS.cache_hits.load(Ordering::Relaxed) as u64,
                cache_misses: swap_cache::STATS.cache_misses.load(Ordering::Relaxed) as u64,
                flash_reads: swap_cache::STATS.flash_reads.load(Ordering::Relaxed) as u64,
                flash_writes: swap_cache::STATS.flash_writes.load(Ordering::Relaxed) as u64,
                flash_allocs: swap_cache::STATS.flash_allocs.load(Ordering::Relaxed) as u64,
                flash_frees: swap_cache::STATS.flash_frees.load(Ordering::Relaxed) as u64,
                flash_erases: swap_cache::STATS.flash_erases.load(Ordering::Relaxed) as u64,
                cache_total_micros: swap_cache::STATS.cache_total_micros.load(Ordering::Relaxed),
                cache_alloc_micros: swap_cache::STATS.cache_alloc_micros.load(Ordering::Relaxed),
                cache_free_micros: swap_cache::STATS.cache_free_micros.load(Ordering::Relaxed),
                cache_used_micros: swap_cache::STATS.cache_used_micros.load(Ordering::Relaxed),
                cache_unused_micros: swap_cache::STATS
                    .cache_unused_micros
                    .load(Ordering::Relaxed),
                flash_reads_micros: swap_cache::STATS.flash_reads_micros.load(Ordering::Relaxed),
                flash_writes_micros: swap_cache::STATS
                    .flash_writes_micros
                    .load(Ordering::Relaxed),
                btree_micros: swap_cache::STATS.btree_micros.load(Ordering::Relaxed),
                heap_micros: swap_cache::STATS.heap_micros.load(Ordering::Relaxed),
                rrb_micros: swap_cache::STATS.rrb_micros.load(Ordering::Relaxed),
                errors_io: swap_cache::STATS.errors_io.load(Ordering::Relaxed) as u64,
            }
        }

        #[cfg(not(feature = "inspect"))]
        {
            Self::default()
        }
    }

    // Restaure exactement une capture des compteurs swap-cache.
    //
    // L'inspection de résidence appelle `inspect_cache`, qui peut lire les blocs
    // absents directement depuis le stockage. Ces lectures ne modifient pas le cache,
    // mais elles incrémentent les compteurs globaux de swap-cache. L'instrumentation
    // restaure donc la capture prise avant l'inspection afin que Stats et Inspect
    // puissent cohabiter dans un même run sans polluer les métriques algorithmiques.
    #[cfg(feature = "inspect")]
    pub fn restore(self) {
        swap_cache::STATS
            .cache_hits
            .store(self.cache_hits as u32, Ordering::Relaxed);
        swap_cache::STATS
            .cache_misses
            .store(self.cache_misses as u32, Ordering::Relaxed);
        swap_cache::STATS
            .flash_reads
            .store(self.flash_reads as u32, Ordering::Relaxed);
        swap_cache::STATS
            .flash_writes
            .store(self.flash_writes as u32, Ordering::Relaxed);
        swap_cache::STATS
            .flash_allocs
            .store(self.flash_allocs as u32, Ordering::Relaxed);
        swap_cache::STATS
            .flash_frees
            .store(self.flash_frees as u32, Ordering::Relaxed);
        swap_cache::STATS
            .flash_erases
            .store(self.flash_erases as u32, Ordering::Relaxed);

        swap_cache::STATS
            .cache_total_micros
            .store(self.cache_total_micros, Ordering::Relaxed);
        swap_cache::STATS
            .cache_alloc_micros
            .store(self.cache_alloc_micros, Ordering::Relaxed);
        swap_cache::STATS
            .cache_free_micros
            .store(self.cache_free_micros, Ordering::Relaxed);
        swap_cache::STATS
            .cache_used_micros
            .store(self.cache_used_micros, Ordering::Relaxed);
        swap_cache::STATS
            .cache_unused_micros
            .store(self.cache_unused_micros, Ordering::Relaxed);
        swap_cache::STATS
            .flash_reads_micros
            .store(self.flash_reads_micros, Ordering::Relaxed);
        swap_cache::STATS
            .flash_writes_micros
            .store(self.flash_writes_micros, Ordering::Relaxed);
        swap_cache::STATS
            .btree_micros
            .store(self.btree_micros, Ordering::Relaxed);
        swap_cache::STATS
            .heap_micros
            .store(self.heap_micros, Ordering::Relaxed);
        swap_cache::STATS
            .rrb_micros
            .store(self.rrb_micros, Ordering::Relaxed);
        swap_cache::STATS
            .errors_io
            .store(self.errors_io as u32, Ordering::Relaxed);
    }

    // Calcule ce qui s'est passé depuis une capture précédente
    pub fn since(before: Self) -> Self {
        let after = Self::get();

        Self {
            cache_hits: after.cache_hits - before.cache_hits,
            cache_misses: after.cache_misses - before.cache_misses,
            flash_reads: after.flash_reads - before.flash_reads,
            flash_writes: after.flash_writes - before.flash_writes,
            flash_allocs: after.flash_allocs - before.flash_allocs,
            flash_frees: after.flash_frees - before.flash_frees,
            flash_erases: after.flash_erases - before.flash_erases,
            cache_total_micros: after.cache_total_micros - before.cache_total_micros,
            cache_alloc_micros: after.cache_alloc_micros - before.cache_alloc_micros,
            cache_free_micros: after.cache_free_micros - before.cache_free_micros,
            cache_used_micros: after.cache_used_micros - before.cache_used_micros,
            cache_unused_micros: after.cache_unused_micros - before.cache_unused_micros,
            flash_reads_micros: after.flash_reads_micros - before.flash_reads_micros,
            flash_writes_micros: after.flash_writes_micros - before.flash_writes_micros,
            btree_micros: after.btree_micros - before.btree_micros,
            heap_micros: after.heap_micros - before.heap_micros,
            rrb_micros: after.rrb_micros - before.rrb_micros,
            errors_io: after.errors_io - before.errors_io,
        }
    }

    // Ajoute les compteurs d'une autre opération
    pub fn add(&mut self, other: Self) {
        self.cache_hits += other.cache_hits;
        self.cache_misses += other.cache_misses;

        self.flash_reads += other.flash_reads;
        self.flash_writes += other.flash_writes;
        self.flash_allocs += other.flash_allocs;
        self.flash_frees += other.flash_frees;
        self.flash_erases += other.flash_erases;

        self.cache_total_micros += other.cache_total_micros;
        self.cache_alloc_micros += other.cache_alloc_micros;
        self.cache_free_micros += other.cache_free_micros;
        self.cache_used_micros += other.cache_used_micros;
        self.cache_unused_micros += other.cache_unused_micros;

        self.flash_reads_micros += other.flash_reads_micros;
        self.flash_writes_micros += other.flash_writes_micros;

        self.btree_micros += other.btree_micros;
        self.heap_micros += other.heap_micros;
        self.rrb_micros += other.rrb_micros;

        self.errors_io += other.errors_io;
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct SCDetail {
    pub calls: u64,
    pub cache_counter: SCCount,
}

impl SCDetail {
    const EMPTY: Self = Self {
        calls: 0,
        cache_counter: SCCount::ZERO,
    };
}

#[derive(Debug, Clone, Copy)]
enum InstantMode {
    Set,
    Add,
    Max,
}

#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstantMetric {
    Profile,
    Found,
    Cost,
    PathLen,
    DataMicros,
    GraphMicros,
    SearchMicros,
    RebuildMicros,
    HeapPushes,
    HeapPops,
    NodesSkipped,
    NodesVisited,
    NeighborsSkipped,
    NeighborsVisited,
    EdgesVisited,
    HeuristicCalls,
    MaxHeap,
    MaxDone,
    MaxPrevious,
    GlobalPushes,
    GlobalPops,
    LocalPushes,
    LocalPops,
    StatesReads,
    StatesWrites,
    MaxGlobal,
    MaxLocal,
    MaxStates,
    CountdownDoneChecks,
    CountdownDoneInserts,
    CountdownDoneDecs,
    CountdownDoneRemoves,
    CountdownScans,
    CountdownEdges,
    PoppedSameTile,
    PoppedCrossTile,
    PoppedTileJumpSum,
    PoppedTileJumpMax,
    SkippedSameTile,
    SkippedCrossTile,
    SkippedTileJumpSum,
    SkippedTileJumpMax,
    VisitedSameTile,
    VisitedCrossTile,
    VisitedTileJumpSum,
    VisitedTileJumpMax,
    HeapPushSameTileCalls,
    HeapPushCrossTileCalls,
    NeighborsNextSameTileCalls,
    NeighborsNextCrossTileCalls,
    NeighborsSameTileCalls,
    NeighborsCrossLeftCalls,
    NeighborsCrossBottomCalls,
    NeighborsCrossBottomLeftCalls,
    EdgesFromSameNode,
    EdgesFromLeftNode,
    EdgesFromBottomNode,
    EdgesFromBottomLeftNode,
}

impl InstantMetric {
    pub const COUNT: usize = 58;

    pub const fn name(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Found => "found",
            Self::Cost => "cost",
            Self::PathLen => "path_len",
            Self::DataMicros => "data_micros",
            Self::GraphMicros => "graph_micros",
            Self::SearchMicros => "search_micros",
            Self::RebuildMicros => "rebuild_micros",
            Self::HeapPushes => "heap_pushes",
            Self::HeapPops => "heap_pops",
            Self::NodesSkipped => "nodes_skipped",
            Self::NodesVisited => "nodes_visited",
            Self::NeighborsSkipped => "neighbors_skipped",
            Self::NeighborsVisited => "neighbors_visited",
            Self::EdgesVisited => "edges_visited",
            Self::HeuristicCalls => "heuristic_calls",
            Self::MaxHeap => "max_heap",
            Self::MaxDone => "max_done",
            Self::MaxPrevious => "max_previous",
            Self::GlobalPushes => "global_pushes",
            Self::GlobalPops => "global_pops",
            Self::LocalPushes => "local_pushes",
            Self::LocalPops => "local_pops",
            Self::StatesReads => "states_reads",
            Self::StatesWrites => "states_writes",
            Self::MaxGlobal => "max_global",
            Self::MaxLocal => "max_local",
            Self::MaxStates => "max_states",
            Self::CountdownDoneChecks => "countdown_done_checks",
            Self::CountdownDoneInserts => "countdown_done_inserts",
            Self::CountdownDoneDecs => "countdown_done_decs",
            Self::CountdownDoneRemoves => "countdown_done_removes",
            Self::CountdownScans => "countdown_scans",
            Self::CountdownEdges => "countdown_edges",
            Self::PoppedSameTile => "popped_same_tile",
            Self::PoppedCrossTile => "popped_cross_tile",
            Self::PoppedTileJumpSum => "popped_tile_jump_sum",
            Self::PoppedTileJumpMax => "popped_tile_jump_max",
            Self::SkippedSameTile => "skipped_same_tile",
            Self::SkippedCrossTile => "skipped_cross_tile",
            Self::SkippedTileJumpSum => "skipped_tile_jump_sum",
            Self::SkippedTileJumpMax => "skipped_tile_jump_max",
            Self::VisitedSameTile => "visited_same_tile",
            Self::VisitedCrossTile => "visited_cross_tile",
            Self::VisitedTileJumpSum => "visited_tile_jump_sum",
            Self::VisitedTileJumpMax => "visited_tile_jump_max",
            Self::HeapPushSameTileCalls => "heap_push_same_tile_calls",
            Self::HeapPushCrossTileCalls => "heap_push_cross_tile_calls",
            Self::NeighborsNextSameTileCalls => "neighbors_next_same_tile_calls",
            Self::NeighborsNextCrossTileCalls => "neighbors_next_cross_tile_calls",
            Self::NeighborsSameTileCalls => "neighbors_same_tile_calls",
            Self::NeighborsCrossLeftCalls => "neighbors_cross_left_calls",
            Self::NeighborsCrossBottomCalls => "neighbors_cross_bottom_calls",
            Self::NeighborsCrossBottomLeftCalls => "neighbors_cross_bottom_left_calls",
            Self::EdgesFromSameNode => "edges_from_same_node",
            Self::EdgesFromLeftNode => "edges_from_left_node",
            Self::EdgesFromBottomNode => "edges_from_bottom_node",
            Self::EdgesFromBottomLeftNode => "edges_from_bottom_left_node",
        }
    }

    const fn mode(self) -> InstantMode {
        match self {
            Self::Profile
            | Self::Found
            | Self::Cost
            | Self::PathLen
            | Self::DataMicros
            | Self::GraphMicros
            | Self::SearchMicros
            | Self::RebuildMicros => InstantMode::Set,

            Self::MaxHeap
            | Self::MaxDone
            | Self::MaxPrevious
            | Self::MaxGlobal
            | Self::MaxLocal
            | Self::MaxStates
            | Self::PoppedTileJumpMax
            | Self::SkippedTileJumpMax
            | Self::VisitedTileJumpMax => InstantMode::Max,

            _ => InstantMode::Add,
        }
    }

    pub const fn exported(self) -> bool {
        !matches!(
            self,
            Self::Profile
                | Self::Found
                | Self::Cost
                | Self::PathLen
                | Self::DataMicros
                | Self::GraphMicros
                | Self::SearchMicros
                | Self::RebuildMicros
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsMetric {
    Total,
    HeapPush,
    HeapPop,
    DoneGet,
    DoneInsert,
    DoneRemove,
    PreviousGet,
    PreviousInsert,
    GlobalPush,
    GlobalPop,
    LocalPush,
    LocalPop,
    StatesGet,
    StatesInsert,
    StatesRemove,
    NeighborsOpen,
    NeighborsNext,
    Rebuild,
}

impl StatsMetric {
    pub const COUNT: usize = 18;

    pub const fn name(self) -> &'static str {
        match self {
            Self::Total => "total",
            Self::HeapPush => "heap_push",
            Self::HeapPop => "heap_pop",
            Self::DoneGet => "done_get",
            Self::DoneInsert => "done_insert",
            Self::DoneRemove => "done_remove",
            Self::PreviousGet => "previous_get",
            Self::PreviousInsert => "previous_insert",
            Self::GlobalPush => "global_push",
            Self::GlobalPop => "global_pop",
            Self::LocalPush => "local_push",
            Self::LocalPop => "local_pop",
            Self::StatesGet => "states_get",
            Self::StatesInsert => "states_insert",
            Self::StatesRemove => "states_remove",
            Self::NeighborsOpen => "neighbors_open",
            Self::NeighborsNext => "neighbors_next",
            Self::Rebuild => "rebuild",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BenchMetrics {
    instant: [u64; InstantMetric::COUNT],
    details: [SCDetail; StatsMetric::COUNT],
    pub total: SCCount,
}

impl BenchMetrics {
    pub const EMPTY: Self = Self {
        instant: [0; InstantMetric::COUNT],
        details: [SCDetail::EMPTY; StatsMetric::COUNT],
        total: SCCount::ZERO,
    };

    pub fn instant_value(&self, metric: InstantMetric) -> u64 {
        self.instant[metric as usize]
    }

    pub fn detail(&self, metric: StatsMetric) -> &SCDetail {
        &self.details[metric as usize]
    }

    fn instant(&mut self, metric: InstantMetric, value: u64) {
        let current = &mut self.instant[metric as usize];
        match metric.mode() {
            InstantMode::Set => *current = value,
            InstantMode::Add => *current += value,
            InstantMode::Max => *current = (*current).max(value),
        }
    }

    #[cfg(feature = "inspect")]
    fn record(&mut self, metric: StatsMetric, delta: SCCount) {
        let detail = &mut self.details[metric as usize];
        detail.calls += 1;
        detail.cache_counter.add(delta);
    }
}

impl Default for BenchMetrics {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(feature = "inspect")]
pub struct Inspector<T> {
    pub known: BTreeSet<T>,
    pub current: BTreeSet<T>,
    pub kept: usize,
    pub exits: usize,
    pub entries: usize,
    pub cached: usize,
    pub total: usize,
    // Rangs de cache renvoyés par swap-cache::inspect_cache pour la capture actuelle.
    // Un rang existe si et seulement si le bloc de stockage correspondant est résident.
    current_blocks: BTreeSet<usize>,
    resident_blocks: BTreeSet<usize>,
}

#[cfg(feature = "inspect")]
impl<T> Default for Inspector<T> {
    fn default() -> Self {
        Self {
            known: BTreeSet::new(),
            current: BTreeSet::new(),
            kept: 0,
            exits: 0,
            entries: 0,
            cached: 0,
            total: 0,
            current_blocks: BTreeSet::new(),
            resident_blocks: BTreeSet::new(),
        }
    }
}

#[cfg(feature = "inspect")]
impl<T: Ord> Inspector<T> {
    // Enregistre le rang de cache exposé par swap-cache::inspect_cache.
    // Les rangs ne servent que dans une même capture : ils identifient des blocs résidents
    // distincts, mais ne constituent pas des identités stables entre les captures.
    pub fn observe_rank(&mut self, rank: usize) {
        self.current_blocks.insert(rank);
    }

    pub fn resident_blocks(&self) -> &BTreeSet<usize> {
        &self.resident_blocks
    }

    pub fn save(&mut self, total: usize) {
        let kept = self.known.intersection(&self.current).count();
        self.kept = kept;
        self.exits = self.known.len() - kept;
        self.entries = self.current.len() - kept;
        self.cached = self.current.len();
        self.total = total;

        core::mem::swap(&mut self.known, &mut self.current);
        self.current.clear();
        core::mem::swap(&mut self.resident_blocks, &mut self.current_blocks);
        self.current_blocks.clear();
    }

    fn state(&self, collection: CollectionKind) -> CollectionCacheState {
        self.state_with_entry_size(collection_entry_size(collection))
    }

    pub(crate) fn state_with_entry_size(&self, entry_size: usize) -> CollectionCacheState {
        CollectionCacheState {
            entries_total: self.total,
            entries_cached: self.cached,
            entries_uncached: self.total.saturating_sub(self.cached),
            entry_size,
            estimated_cached_bytes: estimated_size_with_entry_size(entry_size, self.cached),
            resident_blocks: self.resident_blocks.len(),
            resident_bytes: self.resident_blocks.len() * swap_cache::config::BLOCK_SIZE,
            kept_entries: self.kept,
            entered_entries: self.entries,
            exited_entries: self.exits,
        }
    }
}

#[cfg(feature = "inspect")]
#[derive(Default)]
pub struct GpsInspectors {
    pub gps_maps: Inspector<usize>,
    pub map_stored_tiles: Inspector<(usize, usize)>,
    pub map_stored_ends: Inspector<(usize, usize)>,
    pub map_segments: Inspector<(usize, usize)>,
    pub path_points: Inspector<usize>,
    pub path_waypoints: Inspector<usize>,
    pub interest_stored_tiles: Inspector<usize>,
    pub interest_stored_ends: Inspector<usize>,
    pub interest_data: Inspector<usize>,
    pub heights: Inspector<usize>,
    pub map_tile_index: Inspector<usize>,
}

#[cfg(feature = "inspect")]
#[derive(Clone)]
pub struct TileIndexSpan {
    pub tile: Tile,
    pub len: u8,
    pub rank: Option<usize>,
}

#[cfg(feature = "inspect")]
#[derive(Clone)]
pub struct GpsInspection {
    states: [(CollectionKind, CollectionCacheState, BTreeSet<usize>); 6],
    resident_blocks: BTreeSet<usize>,
    tile_index: Option<(
        CollectionCacheState,
        BTreeSet<usize>,
        std::vec::Vec<TileIndexSpan>,
    )>,
}

#[cfg(feature = "inspect")]
fn union_blocks<'a>(sets: impl IntoIterator<Item = &'a BTreeSet<usize>>) -> BTreeSet<usize> {
    let mut blocks = BTreeSet::new();
    for set in sets {
        blocks.extend(set.iter().copied());
    }
    blocks
}

#[cfg(feature = "inspect")]
fn add(
    states: &[CollectionCacheState],
    entry_size: usize,
    resident_blocks: &BTreeSet<usize>,
) -> CollectionCacheState {
    let mut combined = states.iter().fold(
        CollectionCacheState {
            entry_size,
            ..CollectionCacheState::default()
        },
        |mut combined, state| {
            combined.entries_total += state.entries_total;
            combined.entries_cached += state.entries_cached;
            combined.entries_uncached += state.entries_uncached;
            combined.estimated_cached_bytes += state.estimated_cached_bytes;
            combined.kept_entries += state.kept_entries;
            combined.entered_entries += state.entered_entries;
            combined.exited_entries += state.exited_entries;
            combined
        },
    );
    combined.resident_blocks = resident_blocks.len();
    combined.resident_bytes = resident_blocks.len() * swap_cache::config::BLOCK_SIZE;
    combined
}

#[cfg(feature = "inspect")]
impl GpsInspectors {
    pub fn snapshot(&self) -> GpsInspection {
        let map_tile_blocks = union_blocks([
            self.map_stored_tiles.resident_blocks(),
            self.map_stored_ends.resident_blocks(),
        ]);
        let map_tiles = add(
            &[
                self.map_stored_tiles.state(CollectionKind::MapStoredTiles),
                self.map_stored_ends.state(CollectionKind::MapStoredEnds),
            ],
            size_of::<usize>(),
            &map_tile_blocks,
        );
        let map_segments = self.map_segments.state(CollectionKind::MapSegments);
        let map_segments_blocks = self.map_segments.resident_blocks().clone();
        let path_blocks = union_blocks([
            self.path_points.resident_blocks(),
            self.path_waypoints.resident_blocks(),
        ]);
        let path = add(
            &[
                self.path_points.state(CollectionKind::PathPoints),
                self.path_waypoints.state(CollectionKind::PathWaypoints),
            ],
            0,
            &path_blocks,
        );
        let interest_blocks = union_blocks([
            self.interest_stored_tiles.resident_blocks(),
            self.interest_stored_ends.resident_blocks(),
            self.interest_data.resident_blocks(),
        ]);
        let interests = add(
            &[
                self.interest_stored_tiles
                    .state(CollectionKind::InterestStoredTiles),
                self.interest_stored_ends
                    .state(CollectionKind::InterestStoredEnds),
                self.interest_data.state(CollectionKind::InterestData),
            ],
            0,
            &interest_blocks,
        );
        let heights = self.heights.state(CollectionKind::Heights);
        let heights_blocks = self.heights.resident_blocks().clone();
        let gps_maps = self.gps_maps.state(CollectionKind::GpsMaps);
        let gps_maps_blocks = self.gps_maps.resident_blocks().clone();
        let resident_blocks = union_blocks([
            &gps_maps_blocks,
            &map_tile_blocks,
            &map_segments_blocks,
            &path_blocks,
            &interest_blocks,
            &heights_blocks,
        ]);

        GpsInspection {
            states: [
                (CollectionKind::GpsMaps, gps_maps, gps_maps_blocks),
                (CollectionKind::MapTiles, map_tiles, map_tile_blocks),
                (
                    CollectionKind::MapSegments,
                    map_segments,
                    map_segments_blocks,
                ),
                (CollectionKind::Path, path, path_blocks),
                (CollectionKind::Interests, interests, interest_blocks),
                (CollectionKind::Heights, heights, heights_blocks),
            ],
            resident_blocks,
            tile_index: None,
        }
    }
}

#[cfg(feature = "inspect")]
impl GpsInspection {
    pub fn add_tile_index(
        &mut self,
        index: &Inspector<usize>,
        spans: std::vec::Vec<TileIndexSpan>,
    ) {
        let blocks = index.resident_blocks().clone();
        self.resident_blocks.extend(blocks.iter().copied());
        self.tile_index = Some((index.state(CollectionKind::MapTileIndex), blocks, spans));
    }

    pub fn resident_blocks(&self) -> &BTreeSet<usize> {
        &self.resident_blocks
    }

    pub fn resident_collections(&self) -> impl Iterator<Item = (CollectionKind, &BTreeSet<usize>)> {
        self.states
            .iter()
            .map(|(collection, _, blocks)| (*collection, blocks))
            .chain(
                self.tile_index
                    .iter()
                    .map(|(_, blocks, _)| (CollectionKind::MapTileIndex, blocks)),
            )
    }

    pub fn emit<F>(&self, emit: &mut F, ram: RamCacheState)
    where
        F: FnMut(GpsEvent<GNode>),
    {
        for (collection, state, _) in &self.states {
            emit_state(emit, *collection, *state, ram);
        }
        if let Some((state, _, spans)) = &self.tile_index {
            emit_state(emit, CollectionKind::MapTileIndex, *state, ram);
            emit(GpsEvent::TileIndexSnapshot);
            for span in spans {
                emit(GpsEvent::TileIndexSpan {
                    tile: span.tile,
                    len: span.len,
                    rank: span.rank,
                });
            }
        }
    }
}

#[cfg(feature = "inspect")]
#[allow(async_fn_in_trait)]
pub trait InspectGraph {
    async fn inspect_gps(&self, inspectors: &mut GpsInspectors) -> GpsInspection;
}

#[cfg(not(feature = "inspect"))]
pub trait InspectGraph {}

#[cfg(not(feature = "inspect"))]
impl<T> InspectGraph for T {}

#[allow(async_fn_in_trait)]
pub trait Bench {
    fn report(&self) -> (&'static str, &BenchMetrics);

    #[inline(always)]
    fn instant(&mut self, _metric: InstantMetric, _value: u64) {}

    #[inline(always)]
    fn before(&self) -> SCCount {
        SCCount::ZERO
    }

    #[inline(always)]
    fn after(&mut self, _metric: StatsMetric, _before: SCCount) -> SCCount {
        SCCount::ZERO
    }

    #[inline(always)]
    fn event<F>(&mut self, _emit: &mut F, _event: GpsEvent<GNode>)
    where
        F: FnMut(GpsEvent<GNode>),
    {
    }

    #[cfg(feature = "inspect")]
    fn tick(&mut self) -> bool {
        false
    }

    #[cfg(feature = "inspect")]
    fn current_tick(&self) -> u64 {
        0
    }

    #[cfg(feature = "inspect")]
    fn capacity(&self) -> usize {
        0
    }
}

#[cfg(feature = "inspect")]
#[derive(Clone, Copy)]
struct TileFlowMetrics {
    same: InstantMetric,
    cross: InstantMetric,
    jump_sum: InstantMetric,
    jump_max: InstantMetric,
}

#[cfg(feature = "inspect")]
#[derive(Default)]
struct TileFlowTracker {
    previous: Option<(Tile, usize)>,
}

#[cfg(feature = "inspect")]
impl TileFlowTracker {
    fn observe<B: Bench>(
        &mut self,
        bench: &mut B,
        node: GNode,
        rank: usize,
        metrics: TileFlowMetrics,
    ) {
        if let Some((previous_tile, previous_rank)) = self.previous {
            if previous_tile == node.tile() {
                bench.instant(metrics.same, 1);
            } else {
                let jump = previous_rank.abs_diff(rank) as u64;
                bench.instant(metrics.cross, 1);
                bench.instant(metrics.jump_sum, jump);
                bench.instant(metrics.jump_max, jump);
            }
        }
        self.previous = Some((node.tile(), rank));
    }
}

// Suit séparément la localité des trois flux d’exécution. Le premier noeud de
// chaque flux établit le point de référence et ne compte donc pas comme une
// transition dans la même tuile ou entre tuiles.
#[cfg(feature = "inspect")]
#[derive(Default)]
pub struct LocalityTrackers {
    popped: TileFlowTracker,
    skipped: TileFlowTracker,
    visited: TileFlowTracker,
}

#[cfg(feature = "inspect")]
impl LocalityTrackers {
    pub fn popped<B: Bench>(&mut self, bench: &mut B, node: GNode, rank: usize) {
        self.popped.observe(
            bench,
            node,
            rank,
            TileFlowMetrics {
                same: InstantMetric::PoppedSameTile,
                cross: InstantMetric::PoppedCrossTile,
                jump_sum: InstantMetric::PoppedTileJumpSum,
                jump_max: InstantMetric::PoppedTileJumpMax,
            },
        );
    }

    pub fn skipped<B: Bench>(&mut self, bench: &mut B, node: GNode, rank: usize) {
        self.skipped.observe(
            bench,
            node,
            rank,
            TileFlowMetrics {
                same: InstantMetric::SkippedSameTile,
                cross: InstantMetric::SkippedCrossTile,
                jump_sum: InstantMetric::SkippedTileJumpSum,
                jump_max: InstantMetric::SkippedTileJumpMax,
            },
        );
    }

    pub fn visited<B: Bench>(&mut self, bench: &mut B, node: GNode, rank: usize) {
        self.visited.observe(
            bench,
            node,
            rank,
            TileFlowMetrics {
                same: InstantMetric::VisitedSameTile,
                cross: InstantMetric::VisitedCrossTile,
                jump_sum: InstantMetric::VisitedTileJumpSum,
                jump_max: InstantMetric::VisitedTileJumpMax,
            },
        );
    }
}

#[cfg(feature = "inspect")]
fn record_pair_locality<B: Bench>(
    bench: &mut B,
    from: GNode,
    to: GNode,
    same_metric: InstantMetric,
    cross_metric: InstantMetric,
) {
    if from.tile() == to.tile() {
        bench.instant(same_metric, 1);
    } else {
        bench.instant(cross_metric, 1);
    }
}

#[cfg(feature = "inspect")]
pub fn record_heap_push_locality<B: Bench>(bench: &mut B, from: GNode, to: GNode) {
    record_pair_locality(
        bench,
        from,
        to,
        InstantMetric::HeapPushSameTileCalls,
        InstantMetric::HeapPushCrossTileCalls,
    );
}

#[cfg(feature = "inspect")]
pub fn record_neighbors_next_locality<B: Bench>(bench: &mut B, from: GNode, to: GNode) {
    record_pair_locality(
        bench,
        from,
        to,
        InstantMetric::NeighborsNextSameTileCalls,
        InstantMetric::NeighborsNextCrossTileCalls,
    );
}

#[cfg(feature = "inspect")]
pub fn record_neighbors_open_locality<B: Bench>(bench: &mut B, node: GNode) {
    let (x, y) = node.index();
    let metric = match (x == 0, y == 0) {
        (false, false) => InstantMetric::NeighborsSameTileCalls,
        (true, false) => InstantMetric::NeighborsCrossLeftCalls,
        (false, true) => InstantMetric::NeighborsCrossBottomCalls,
        (true, true) => InstantMetric::NeighborsCrossBottomLeftCalls,
    };
    bench.instant(metric, 1);
}

#[cfg(feature = "inspect")]
pub fn record_edge_origin<B: Bench>(bench: &mut B, node: GNode) {
    let (x, y) = node.index();
    let metric = match (x == 0, y == 0) {
        (false, false) => InstantMetric::EdgesFromSameNode,
        (true, false) => InstantMetric::EdgesFromLeftNode,
        (false, true) => InstantMetric::EdgesFromBottomNode,
        (true, true) => InstantMetric::EdgesFromBottomLeftNode,
    };
    bench.instant(metric, 1);
}

#[inline(always)]
pub fn emit_node<B, F>(
    bench: &mut B,
    emit: &mut F,
    node: GNode,
    source: CacheSource,
    delta: SCCount,
) where
    B: Bench,
    F: FnMut(GpsEvent<GNode>),
{
    #[cfg(feature = "inspect")]
    {
        if delta.cache_hits == 0 && delta.cache_misses == 0 {
            return;
        }

        bench.event(
            emit,
            GpsEvent::CacheNode {
                node,
                source,
                tick: bench.current_tick(),
                delta: delta.into(),
            },
        );
    }

    #[cfg(not(feature = "inspect"))]
    {
        let _ = (bench, emit, node, source, delta);
    }
}

#[derive(Default)]
pub struct PerfBench;

impl Bench for PerfBench {
    fn report(&self) -> (&'static str, &BenchMetrics) {
        ("perf", &BenchMetrics::EMPTY)
    }
}

pub struct StatsBench {
    mode: &'static str,
    metrics: BenchMetrics,
}

impl StatsBench {
    pub fn new(mode: &'static str) -> Self {
        Self {
            mode,
            metrics: BenchMetrics::default(),
        }
    }

    fn record_after(&mut self, metric: StatsMetric, before: SCCount) -> SCCount {
        #[cfg(feature = "inspect")]
        {
            let delta = SCCount::since(before);
            if metric == StatsMetric::Total {
                self.metrics.total = delta;
            } else {
                self.metrics.record(metric, delta);
            }
            delta
        }
        #[cfg(not(feature = "inspect"))]
        {
            let _ = (metric, before);
            SCCount::ZERO
        }
    }
}

impl Bench for StatsBench {
    fn report(&self) -> (&'static str, &BenchMetrics) {
        (self.mode, &self.metrics)
    }

    fn instant(&mut self, metric: InstantMetric, value: u64) {
        self.metrics.instant(metric, value);
    }

    fn before(&self) -> SCCount {
        SCCount::get()
    }

    fn after(&mut self, metric: StatsMetric, before: SCCount) -> SCCount {
        self.record_after(metric, before)
    }
}

#[cfg(feature = "inspect")]
fn collection_entry_size(coll: CollectionKind) -> usize {
    match coll {
        CollectionKind::Heap => size_of::<GNode>() + 3 * size_of::<u32>(),
        // A* et Dijkstra stockent (GNode, ()) dans Done. Countdown transmet explicitement
        // la taille réelle de son entrée (GNode, u8) lors de l’émission des états d’inspection.
        CollectionKind::Done => size_of::<(GNode, ())>(),
        CollectionKind::Previous => 2 * size_of::<GNode>(),
        CollectionKind::Global => size_of::<GNode>() + 3 * size_of::<u32>(),
        CollectionKind::Local => size_of::<GNode>() + size_of::<u32>(),
        CollectionKind::States => 2 * size_of::<GNode>() + size_of::<u32>(),
        CollectionKind::ResultPath => size_of::<GNode>(),
        CollectionKind::MapTileIndex => size_of::<u32>(),
        CollectionKind::MapStoredTiles
        | CollectionKind::MapStoredEnds
        | CollectionKind::MapTiles => size_of::<usize>(),
        CollectionKind::MapSegments => size_of::<RawSegment>(),
        CollectionKind::PathPoints => size_of::<Node>(),
        CollectionKind::PathWaypoints => size_of::<u8>(),
        CollectionKind::InterestStoredTiles | CollectionKind::InterestStoredEnds => {
            size_of::<usize>()
        }
        CollectionKind::InterestData => size_of::<RawInterest>(),
        CollectionKind::Heights => size_of::<i16>(),
        CollectionKind::GpsMaps | CollectionKind::Path | CollectionKind::Interests => 0,
    }
}

// Projection minimale de la RAM occupée par les blocs feuilles réellement résidents.
#[cfg(feature = "inspect")]
pub fn estimated_size(coll: CollectionKind, cached: usize) -> usize {
    estimated_size_with_entry_size(collection_entry_size(coll), cached)
}

#[cfg(feature = "inspect")]
pub fn estimated_size_with_entry_size(entry_size: usize, cached: usize) -> usize {
    if cached == 0 || entry_size == 0 {
        return 0;
    }

    let block_size = swap_cache::config::BLOCK_SIZE;
    let payload_size = block_size - size_of::<u16>();
    let entries_per_block = payload_size / entry_size;

    if entries_per_block == 0 {
        return cached * block_size;
    }

    let full_blocks = cached / entries_per_block;
    let remaining_entries = cached % entries_per_block;

    let full_block_waste = block_size - entries_per_block * entry_size;
    let last_block_waste = if remaining_entries == 0 {
        0
    } else {
        block_size - remaining_entries * entry_size
    };

    (cached * entry_size) + (full_blocks * full_block_waste) + (last_block_waste)
}

#[cfg(feature = "inspect")]
pub fn cache_residency<'a>(
    cache_capacity_bytes: usize,
    resident_sets: impl IntoIterator<Item = (CollectionKind, &'a BTreeSet<usize>)>,
) -> RamCacheState {
    let mut blocks = BTreeSet::new();
    let mut by_collection = [0usize; CollectionKind::COUNT];

    for (collection, resident) in resident_sets {
        for rank in resident {
            if blocks.insert(*rank) {
                by_collection[collection.index()] += 1;
            }
        }
    }

    let resident_blocks = blocks.len();
    let resident_bytes = resident_blocks * swap_cache::config::BLOCK_SIZE;
    RamCacheState {
        cache_capacity_blocks: cache_capacity_bytes / swap_cache::config::BLOCK_SIZE,
        cache_capacity_bytes,
        observed_resident_blocks: resident_blocks,
        observed_resident_bytes: resident_bytes,
        observed_resident_blocks_by_collection: by_collection,
        unobserved_capacity_bytes: cache_capacity_bytes.saturating_sub(resident_bytes),
    }
}

#[cfg(feature = "inspect")]
pub fn emit_inspection<T: Ord, F>(
    emit: &mut F,
    coll: CollectionKind,
    cache: &Inspector<T>,
    ram: RamCacheState,
) where
    F: FnMut(GpsEvent<GNode>),
{
    emit_state(emit, coll, cache.state(coll), ram);
}

#[cfg(feature = "inspect")]
pub fn emit_inspection_with_entry_size<T: Ord, F>(
    emit: &mut F,
    coll: CollectionKind,
    cache: &Inspector<T>,
    entry_size: usize,
    ram: RamCacheState,
) where
    F: FnMut(GpsEvent<GNode>),
{
    emit_state(emit, coll, cache.state_with_entry_size(entry_size), ram);
}

#[cfg(feature = "inspect")]
fn emit_state<F>(
    emit: &mut F,
    coll: CollectionKind,
    state: CollectionCacheState,
    ram: RamCacheState,
) where
    F: FnMut(GpsEvent<GNode>),
{
    emit(GpsEvent::Collection {
        collection: coll,
        action: CollectionAction::Update,
        changed: state.entered_entries != 0 || state.exited_entries != 0,
        len_before: 0,
        len_after: state.entries_total,
        cache_delta: CacheDelta::default(),
        cache_state: Some(state),
        ram_state: Some(ram),
    });
}

#[cfg(feature = "inspect")]
pub struct InspectBench {
    stats: StatsBench,
    stride: u64,
    i: u64,
    capacity: usize,
}

#[cfg(feature = "inspect")]
impl InspectBench {
    pub fn new(stride: u64, capacity: usize) -> Self {
        Self {
            stats: StatsBench::new("inspect"),
            stride,
            i: 0,
            capacity,
        }
    }
}

#[cfg(feature = "inspect")]
impl Bench for InspectBench {
    fn report(&self) -> (&'static str, &BenchMetrics) {
        self.stats.report()
    }

    fn instant(&mut self, metric: InstantMetric, value: u64) {
        self.stats.instant(metric, value);
    }

    fn before(&self) -> SCCount {
        self.stats.before()
    }

    fn after(&mut self, metric: StatsMetric, before: SCCount) -> SCCount {
        self.stats.after(metric, before)
    }

    fn event<F>(&mut self, emit: &mut F, event: GpsEvent<GNode>)
    where
        F: FnMut(GpsEvent<GNode>),
    {
        emit(event);
    }

    fn tick(&mut self) -> bool {
        let inspect = self.i.is_multiple_of(self.stride);
        self.i += 1;
        inspect
    }

    fn current_tick(&self) -> u64 {
        self.i + 1
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}

impl From<SCCount> for CacheDelta {
    fn from(value: SCCount) -> Self {
        Self {
            cache_hits: value.cache_hits,
            cache_misses: value.cache_misses,
            flash_reads: value.flash_reads,
            flash_writes: value.flash_writes,
            flash_allocs: value.flash_allocs,
            flash_frees: value.flash_frees,
            flash_erases: value.flash_erases,
            errors_io: value.errors_io,
        }
    }
}

pub fn finish_found<C, B, F>(
    mut result: BenchResult<C>,
    bench: &mut B,
    emit: &mut F,
    cost: u32,
    path: RRBTree<GNode, C>,
    run_before: SCCount,
) -> BenchResult<C>
where
    C: CacheSingleton,
    B: Bench,
    F: FnMut(GpsEvent<GNode>),
{
    let path_len = path.len();
    bench.after(StatsMetric::Total, run_before);
    bench.event(
        emit,
        GpsEvent::Finished {
            found: true,
            path_len,
        },
    );
    result.found(bench, cost, path_len);
    result.path = Some(path);
    result
}

pub fn finish_failed<C, B, F>(
    result: BenchResult<C>,
    bench: &mut B,
    emit: &mut F,
    now_search: TimeInstant,
    run_before: SCCount,
) -> BenchResult<C>
where
    C: CacheSingleton,
    B: Bench,
    F: FnMut(GpsEvent<GNode>),
{
    bench.instant(
        InstantMetric::SearchMicros,
        now_search.elapsed().as_micros(),
    );
    bench.after(StatsMetric::Total, run_before);
    bench.event(
        emit,
        GpsEvent::Finished {
            found: false,
            path_len: 0,
        },
    );
    result
}

pub struct BenchResult<C: CacheSingleton> {
    pub found: bool,
    pub cost: Option<u32>,
    pub path_len: Option<usize>,
    pub path: Option<RRBTree<GNode, C>>,
}

impl<C: CacheSingleton> BenchResult<C> {
    pub fn new() -> Self {
        Self {
            found: false,
            cost: None,
            path_len: None,
            path: None,
        }
    }

    pub fn found<B: Bench>(&mut self, bench: &mut B, cost: u32, path_len: usize) {
        self.found = true;
        self.cost = Some(cost);
        self.path_len = Some(path_len);
        bench.instant(InstantMetric::Found, 1);
        bench.instant(InstantMetric::Cost, cost as u64);
        bench.instant(InstantMetric::PathLen, path_len as u64);
    }
}

impl<C: CacheSingleton> Default for BenchResult<C> {
    fn default() -> Self {
        Self::new()
    }
}
