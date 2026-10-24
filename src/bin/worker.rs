use chrono::Local;
use lib_gps::{
    algo::registry::{
        Algorithm as BenchAlgo, AlgorithmRun, RunOptions, SPECIFIC_METRIC_NAMES, SpecificMetrics,
        WINDOW_CAPACITY, validate_capacity,
    },
    event::GpsEvent,
    format::{Gps, Node, reader::Reader},
    graph::{
        GNode, Graph, TileOrder, UGraph,
        tile_index::{GraphStorage, TileIndexedGraph},
    },
    inspection::{
        Bench, BenchMetrics, INSTANT_METRICS, InspectGraph, InstantMetric, SCCount, SCDetail,
        STATS_METRICS,
    },
};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Instant;
#[cfg(feature = "inspect")]
use lib_gps::inspection::InspectBench;
#[cfg(feature = "perf")]
use lib_gps::inspection::PerfBench;

use serde::Serialize;
use std::env;
use std::fs::File;
use std::io::{BufWriter, Error, ErrorKind, Result, Write};
use std::path::{Path, PathBuf};

use swap_cache::{CacheLRU, cache, storage, storages::MockFlash};

#[cfg(all(feature = "cache-128k", feature = "cache-256k"))]
compile_error!("Les features cache-128k et cache-256k sont mutuellement exclusives");

// 1024 Blocs de 256o (256Ko de RAM)
#[cfg(feature = "cache-256k")]
const PROFILE_CAPACITY: usize = 1024;

// 512 Blocs de 256o (128Ko de RAM)
#[cfg(all(feature = "cache-128k", not(feature = "cache-256k")))]
const PROFILE_CAPACITY: usize = 512;

// Lancement par défaut (256Ko)
#[cfg(not(any(feature = "cache-128k", feature = "cache-256k")))]
const PROFILE_CAPACITY: usize = 1024;

const CAPACITY: usize = PROFILE_CAPACITY;

const FLASH_SIZE: usize = 16_384; // 4Mo

#[cfg(feature = "swap-64")]
const SWAP_BLOCK_BYTES: usize = 64;

#[cfg(feature = "swap-128")]
const SWAP_BLOCK_BYTES: usize = 128;

#[cfg(feature = "swap-256")]
const SWAP_BLOCK_BYTES: usize = 256;

#[cfg(feature = "swap-512")]
const SWAP_BLOCK_BYTES: usize = 512;

const SWAP_CACHE_BLOCKS: usize = CAPACITY;
const SWAP_CACHE_KO: usize = SWAP_CACHE_BLOCKS * SWAP_BLOCK_BYTES / 1024;
const SWAP_FLASH_BLOCKS: usize = FLASH_SIZE;
const SWAP_FLASH_KO: usize = SWAP_FLASH_BLOCKS * SWAP_BLOCK_BYTES / 1024;

storage!(FLASH: MockFlash<FLASH_SIZE>);
cache!(CACHE: CacheLRU::<FLASH, CriticalSectionRawMutex, CAPACITY>);

#[derive(Clone)]
struct BenchConfig {
    graph_storage: String,
    neighbor_access: String,
    exploration_policy: String,
    bucket_state: String,
    heuristic: String,
    window_delta: u32,
    window_batch: u32,
}

impl BenchConfig {
    fn from_env(algo: BenchAlgo) -> Result<BenchConfig> {
        Ok(BenchConfig {
            graph_storage: env_value("BENCH_GRAPH_STORAGE", "tile_dicho"),
            neighbor_access: env_value("BENCH_NEIGHBOR_ACCESS", "tile_segments"),
            exploration_policy: env_value(
                "BENCH_EXPLORATION_POLICY",
                algo.descriptor().policy(None),
            ),
            bucket_state: if !algo.descriptor().states.is_empty() {
                env_value("BENCH_BUCKET_STATE", algo.descriptor().state(None))
            } else {
                String::new()
            },
            heuristic: env_value("BENCH_HEURISTIC", "standard"),
            window_delta: algo.descriptor().parameter("delta").unwrap_or(0),
            window_batch: algo.descriptor().parameter("max_batch").unwrap_or(32),
        })
    }

    fn validate_current_backend(&self, algo: BenchAlgo) -> Result<()> {
        if GraphStorage::parse(&self.graph_storage).is_none()
            || self.neighbor_access != "tile_segments"
            || self.heuristic != "standard"
        {
            return Err(invalid(
                "configuration graphe/voisins/heuristique non implémentée",
            ));
        }
        algo.descriptor()
            .validate(RunOptions {
                policy: &self.exploration_policy,
                state: &self.bucket_state,
                parameters: &[],
            })
            .map_err(invalid)?;
        Ok(())
    }

    fn apply_window_tuning(&mut self, args: &WorkerArgs) -> Result<()> {
        let tuning_requested = args.window.is_some()
            || args.delta.is_some()
            || args.batch.is_some()
            || args.tuning_output;
        if tuning_requested && args.algo != BenchAlgo::AstarWindow {
            return Err(invalid(
                "--window, --delta, --batch et --tuning-output sont réservés à A* Window",
            ));
        }
        if tuning_requested && args.save_dir.is_some() {
            return Err(invalid(
                "les réglages Window ne peuvent pas être sauvegardés",
            ));
        }
        if args.algo == BenchAlgo::AstarWindow
            && args.save_dir.is_some()
            && WINDOW_CAPACITY != args.algo.descriptor().parameter("window").unwrap_or(64) as usize
        {
            return Err(invalid(
                "une fenêtre compilée différente du preset ne peut pas être sauvegardée",
            ));
        }
        if args.tuning_output && args.saved_output {
            return Err(invalid(
                "--tuning-output est incompatible avec --saved-output",
            ));
        }
        if let Some(window) = args.window {
            validate_capacity(window).map_err(invalid)?;
            if window != WINDOW_CAPACITY {
                return Err(invalid(format!(
                    "fenêtre demandée {window}, worker compilé pour {WINDOW_CAPACITY} : lancer via bench-bin",
                )));
            }
        }
        if args.algo == BenchAlgo::AstarWindow {
            if let Some(batch) = args.batch {
                if batch == 0 {
                    return Err(invalid("--batch doit être supérieur à zéro"));
                }
                self.window_batch = batch;
            }
            if self.exploration_policy == "spatial_tiebreak" {
                if args.delta.is_some_and(|delta| delta != 0) {
                    return Err(invalid("Départage spatial nécessite --delta 0"));
                }
                self.window_delta = 0;
            } else if let Some(delta) = args.delta {
                self.window_delta = delta;
            }
        }
        Ok(())
    }

    fn slug(&self) -> String {
        let mut parts = vec![
            slug_part(&self.graph_storage),
            slug_part(&self.neighbor_access),
            slug_part(&self.exploration_policy),
            slug_part(&self.heuristic),
        ];
        if !self.bucket_state.is_empty() {
            parts.push(slug_part(&self.bucket_state));
        }
        parts.join("_")
    }
}

struct WorkerArgs {
    algo: BenchAlgo,
    order: TileOrder,
    file: String,
    case_id: String,
    start_lon: f64,
    start_lat: f64,
    end_lon: f64,
    end_lat: f64,
    save_dir: Option<PathBuf>,
    saved_output: bool,
    inspect_stride: u64,
    window: Option<usize>,
    delta: Option<u32>,
    batch: Option<u32>,
    tuning_output: bool,
}

impl WorkerArgs {
    fn parse() -> Result<Self> {
        let inspect_stride = env::var("BENCH_INSPECT_STRIDE")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(100)
            .max(1);
        Self::parse_from(env::args().skip(1), inspect_stride)
    }

    fn parse_from(mut args: impl Iterator<Item = String>, mut inspect_stride: u64) -> Result<Self> {
        if args.next().as_deref() != Some("--worker") {
            return Err(invalid("le worker doit être lancé par bench-bin"));
        }

        let mut algo = None;
        let mut order = None;
        let mut file = None;
        let mut case_id = None;
        let mut start = None;
        let mut end = None;
        let mut save_dir = None;
        let mut saved_output = false;
        let mut window = None;
        let mut delta = None;
        let mut batch = None;
        let mut tuning_output = false;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--algo" => {
                    algo = Some(
                        BenchAlgo::parse(&next(&mut args, "--algo")?)
                            .ok_or_else(|| invalid("algorithme inconnu ou désactivé"))?,
                    )
                }
                "--layout" => {
                    order = Some(match next(&mut args, "--layout")?.as_str() {
                        "normal" => TileOrder::RowMajor,
                        "hilbert" => TileOrder::Hilbert,
                        "zcurve" => TileOrder::Morton,
                        value => return Err(invalid(format!("layout inconnu: {value}"))),
                    });
                }
                "--file" => file = Some(next(&mut args, "--file")?),
                "--case" => case_id = Some(next(&mut args, "--case")?),
                "--from" => start = Some(read_point(&mut args, "--from")?),
                "--to" => end = Some(read_point(&mut args, "--to")?),
                "--save-dir" => save_dir = Some(PathBuf::from(next(&mut args, "--save-dir")?)),
                "--saved-output" => saved_output = true,
                "--window" => {
                    window = Some(
                        next(&mut args, "--window")?
                            .parse::<usize>()
                            .map_err(|_| invalid("--window doit être un entier positif"))?,
                    );
                }
                "--delta" => {
                    delta = Some(
                        next(&mut args, "--delta")?
                            .parse::<u32>()
                            .map_err(|_| invalid("--delta doit être un entier positif ou nul"))?,
                    );
                }
                "--tuning-output" => tuning_output = true,
                "--batch" | "--max-batch" => {
                    batch =
                        Some(next(&mut args, &arg)?.parse::<u32>().map_err(|_| {
                            invalid("--batch doit être un entier strictement positif")
                        })?);
                }
                "--inspect-stride" => {
                    inspect_stride = next(&mut args, "--inspect-stride")?
                        .parse::<u64>()
                        .map_err(|_| invalid("--inspect-stride doit être un entier positif"))?
                        .max(1)
                }
                _ => return Err(invalid(format!("argument worker inconnu: {arg}"))),
            }
        }

        let (start_lon, start_lat) = start.ok_or_else(|| invalid("--from manquant"))?;
        let (end_lon, end_lat) = end.ok_or_else(|| invalid("--to manquant"))?;

        Ok(Self {
            algo: algo.ok_or_else(|| invalid("--algo manquant"))?,
            order: order.ok_or_else(|| invalid("--layout manquant"))?,
            file: file.ok_or_else(|| invalid("--file manquant"))?,
            case_id: case_id.ok_or_else(|| invalid("--case manquant"))?,
            start_lon,
            start_lat,
            end_lon,
            end_lat,
            save_dir,
            saved_output,
            inspect_stride,
            window,
            delta,
            batch,
            tuning_output,
        })
    }
}

#[derive(Default)]
struct EventCounts {
    started: u64,
    pushed: u64,
    popped: u64,
    settled: u64,
    relaxed: u64,
    path_nodes: u64,
    cache_nodes: u64,
    collections: u64,
    finished: u64,
    tile_index: u64,
}

struct EventLog {
    counts: EventCounts,
    next_seq: u64,
    writer: Option<BufWriter<File>>,
    temp_path: Option<PathBuf>,
    write_error: Option<Error>,
}

#[inline(always)]
fn record_event(event_log: &mut EventLog, event: GpsEvent<GNode>) {
    // En mode perf, cette condition est fausse dès la compilation. La fonction
    // intégrée sans effet permet à LLVM de supprimer la gestion et la création
    // des événements du chemin critique. Le mode inspect garde le replay complet.
    if cfg!(feature = "inspect") {
        event_log.record(event);
    }
}

impl EventLog {
    fn new(save_dir: Option<&Path>) -> Result<Self> {
        let (writer, temp_path) = if cfg!(feature = "inspect") {
            if let Some(save_dir) = save_dir {
                std::fs::create_dir_all(save_dir)?;
                let path = save_dir.join(format!(
                    ".events-{}-{}.jsonl.tmp",
                    std::process::id(),
                    Local::now().timestamp_nanos_opt().unwrap_or_default()
                ));
                (Some(BufWriter::new(File::create(&path)?)), Some(path))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        Ok(Self {
            counts: EventCounts::default(),
            next_seq: 0,
            writer,
            temp_path,
            write_error: None,
        })
    }

    fn record(&mut self, event: GpsEvent<GNode>) {
        let saved = match event {
            GpsEvent::TileIndexSnapshot => {
                self.counts.tile_index += 1;
                SavedEventKind::TileIndexSnapshot
            }
            GpsEvent::TileIndexSpan { tile, len, rank } => {
                self.counts.tile_index += 1;
                SavedEventKind::TileIndexSpan {
                    tile_x: tile.0,
                    tile_y: tile.1,
                    len,
                    rank,
                }
            }
            GpsEvent::Started { start, end } => {
                self.counts.started += 1;
                SavedEventKind::Started {
                    start: start.into(),
                    end: end.into(),
                }
            }
            GpsEvent::Pushed {
                node,
                cost,
                estimation,
            } => {
                self.counts.pushed += 1;
                SavedEventKind::Pushed {
                    node: node.into(),
                    cost,
                    estimation,
                }
            }
            GpsEvent::Popped {
                node,
                cost,
                estimation,
            } => {
                self.counts.popped += 1;
                SavedEventKind::Popped {
                    node: node.into(),
                    cost,
                    estimation,
                }
            }
            GpsEvent::Settled { node, cost } => {
                self.counts.settled += 1;
                SavedEventKind::Settled {
                    node: node.into(),
                    cost,
                }
            }
            GpsEvent::Relaxed {
                from,
                to,
                edge_cost,
                new_cost,
                estimation,
            } => {
                self.counts.relaxed += 1;
                SavedEventKind::Relaxed {
                    from: from.into(),
                    to: to.into(),
                    edge_cost,
                    new_cost,
                    estimation,
                }
            }
            GpsEvent::PathNode { node } => {
                self.counts.path_nodes += 1;
                SavedEventKind::PathNode { node: node.into() }
            }
            GpsEvent::CacheNode {
                node,
                source,
                tick,
                delta,
            } => {
                self.counts.cache_nodes += 1;
                SavedEventKind::CacheNode {
                    node: node.into(),
                    source: source.as_str(),
                    tick,
                    delta: delta.into(),
                }
            }
            GpsEvent::Collection {
                collection,
                action,
                changed,
                len_before,
                len_after,
                cache_delta,
                cache_state,
                ram_state,
            } => {
                self.counts.collections += 1;
                SavedEventKind::Collection {
                    collection: collection.as_str(),
                    action: action.as_str(),
                    changed,
                    len_before,
                    len_after,
                    cache_delta: cache_delta.into(),
                    cache_state: cache_state.map(Into::into),
                    ram_state: ram_state.map(Into::into),
                }
            }
            GpsEvent::Finished { found, path_len } => {
                self.counts.finished += 1;
                SavedEventKind::Finished { found, path_len }
            }
        };

        let event = SavedEvent {
            version: "2.4",
            seq: self.next_seq,
            event: saved,
        };
        self.next_seq += 1;

        if self.write_error.is_some() {
            return;
        }

        let Some(writer) = self.writer.as_mut() else {
            return;
        };

        if let Err(err) = serde_json::to_writer(&mut *writer, &event)
            .map_err(Error::other)
            .and_then(|_| writer.write_all(b"\n"))
        {
            self.write_error = Some(err);
        }
    }

    fn total(&self) -> u64 {
        self.counts.started
            + self.counts.pushed
            + self.counts.popped
            + self.counts.settled
            + self.counts.relaxed
            + self.counts.path_nodes
            + self.counts.cache_nodes
            + self.counts.collections
            + self.counts.finished
            + self.counts.tile_index
    }

    fn finish(&mut self, path: &Path) -> Result<()> {
        if let Some(err) = self.write_error.take() {
            return Err(err);
        }

        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
        }

        let Some(temp_path) = self.temp_path.take() else {
            return Ok(());
        };

        if path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(temp_path, path)
    }
}

#[derive(Serialize)]
struct SavedEvent {
    #[serde(rename = "v")]
    version: &'static str,
    seq: u64,
    #[serde(flatten)]
    event: SavedEventKind,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SavedEventKind {
    TileIndexSnapshot,
    TileIndexSpan {
        tile_x: u8,
        tile_y: u8,
        len: u8,
        rank: Option<usize>,
    },
    Started {
        start: SavedNode,
        end: SavedNode,
    },
    Pushed {
        node: SavedNode,
        cost: u32,
        estimation: u32,
    },
    Popped {
        node: SavedNode,
        cost: u32,
        estimation: u32,
    },
    Settled {
        node: SavedNode,
        cost: u32,
    },
    Relaxed {
        from: SavedNode,
        to: SavedNode,
        edge_cost: u32,
        new_cost: u32,
        estimation: u32,
    },
    PathNode {
        node: SavedNode,
    },
    CacheNode {
        node: SavedNode,
        source: &'static str,
        tick: u64,
        delta: SavedCacheDelta,
    },
    Collection {
        collection: &'static str,
        action: &'static str,
        changed: bool,
        len_before: usize,
        len_after: usize,
        cache_delta: SavedCacheDelta,
        cache_state: Option<SavedCacheState>,
        ram_state: Option<SavedRamState>,
    },
    Finished {
        found: bool,
        path_len: usize,
    },
}

#[derive(Serialize)]
struct SavedCacheDelta {
    cache_hits: u64,
    cache_misses: u64,
    flash_reads: u64,
    flash_writes: u64,
    flash_allocs: u64,
    flash_frees: u64,
    flash_erases: u64,
    errors_io: u64,
}

impl From<lib_gps::event::CacheDelta> for SavedCacheDelta {
    fn from(value: lib_gps::event::CacheDelta) -> Self {
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

#[derive(Serialize)]
struct SavedCacheState {
    entries_total: usize,
    entries_cached: usize,
    entries_uncached: usize,
    entry_size: usize,
    estimated_cached_bytes: usize,
    resident_blocks: usize,
    resident_bytes: usize,
    kept_entries: usize,
    entered_entries: usize,
    exited_entries: usize,
}

impl From<lib_gps::event::CollectionCacheState> for SavedCacheState {
    fn from(value: lib_gps::event::CollectionCacheState) -> Self {
        Self {
            entries_total: value.entries_total,
            entries_cached: value.entries_cached,
            entries_uncached: value.entries_uncached,
            entry_size: value.entry_size,
            estimated_cached_bytes: value.estimated_cached_bytes,
            resident_blocks: value.resident_blocks,
            resident_bytes: value.resident_bytes,
            kept_entries: value.kept_entries,
            entered_entries: value.entered_entries,
            exited_entries: value.exited_entries,
        }
    }
}

#[derive(Serialize)]
struct SavedRamCollectionState {
    collection: &'static str,
    resident_blocks: usize,
    resident_bytes: usize,
}

#[derive(Serialize)]
struct SavedRamState {
    cache_capacity_blocks: usize,
    cache_capacity_bytes: usize,
    observed_resident_blocks: usize,
    observed_resident_bytes: usize,
    resident_collections: Vec<SavedRamCollectionState>,
    unobserved_capacity_bytes: usize,
}

impl From<lib_gps::event::RamCacheState> for SavedRamState {
    fn from(value: lib_gps::event::RamCacheState) -> Self {
        let resident_collections = lib_gps::event::CollectionKind::ALL
            .into_iter()
            .filter_map(|collection| {
                let resident_blocks =
                    value.observed_resident_blocks_by_collection[collection.index()];
                (resident_blocks > 0).then_some(SavedRamCollectionState {
                    collection: collection.as_str(),
                    resident_blocks,
                    resident_bytes: resident_blocks * swap_cache::config::BLOCK_SIZE,
                })
            })
            .collect();

        Self {
            cache_capacity_blocks: value.cache_capacity_blocks,
            cache_capacity_bytes: value.cache_capacity_bytes,
            observed_resident_blocks: value.observed_resident_blocks,
            observed_resident_bytes: value.observed_resident_bytes,
            resident_collections,
            unobserved_capacity_bytes: value.unobserved_capacity_bytes,
        }
    }
}

#[derive(Serialize)]
struct SavedNode {
    tile_x: u8,
    tile_y: u8,
    index_x: u8,
    index_y: u8,
    x: u32,
    y: u32,
}

impl From<GNode> for SavedNode {
    fn from(node: GNode) -> Self {
        let (tile_x, tile_y) = node.tile();
        let (index_x, index_y) = node.index();
        Self {
            tile_x,
            tile_y,
            index_x,
            index_y,
            x: tile_x as u32 * 255 + index_x as u32,
            y: tile_y as u32 * 255 + index_y as u32,
        }
    }
}

#[derive(Clone, Copy)]
struct RunSummary {
    specific_metrics: SpecificMetrics,
    found: bool,
    cost: u32,
    path_len: usize,
    decode_us: u64,
    graph_us: u64,
    search_us: u64,
    rebuild_us: u64,
    graph_index_entries: usize,
    graph_index_logical_bytes: usize,
    graph_index_fallback_tiles: usize,
}

#[derive(Serialize)]
struct WindowTuningSummary {
    window: usize,
    delta: u32,
    batch: u32,
    search_us: u64,
    cost: u32,
    path_len: usize,
    found: bool,
    swap_cache_blocks: usize,
    relaxed_pops: u64,
    max_f_slack: u64,
    flash_reads: Option<u64>,
    flash_writes: Option<u64>,
    cache_hits: Option<u64>,
    cache_misses: Option<u64>,
}

impl WindowTuningSummary {
    fn new(config: &BenchConfig, summary: &RunSummary, counters: &SCCount) -> Self {
        Self {
            window: WINDOW_CAPACITY,
            delta: config.window_delta,
            batch: config.window_batch,
            search_us: summary.search_us,
            cost: summary.cost,
            path_len: summary.path_len,
            found: summary.found,
            swap_cache_blocks: SWAP_CACHE_BLOCKS,
            relaxed_pops: summary
                .specific_metrics
                .get("tile_relaxed_pops")
                .unwrap_or(0),
            max_f_slack: summary.specific_metrics.get("fslack_max").unwrap_or(0),
            flash_reads: cfg!(feature = "inspect").then_some(counters.flash_reads),
            flash_writes: cfg!(feature = "inspect").then_some(counters.flash_writes),
            cache_hits: cfg!(feature = "inspect").then_some(counters.cache_hits),
            cache_misses: cfg!(feature = "inspect").then_some(counters.cache_misses),
        }
    }
}

fn close_search_window(start: Instant) -> (embassy_time::Duration, SCCount) {
    let duration = start.elapsed();
    let counters = SCCount::get();
    (duration, counters)
}

async fn search<G: Graph<ULoc = GNode, Loc = Node> + InspectGraph, B: Bench>(
    graph: &G,
    args: &WorkerArgs,
    bench_config: &BenchConfig,
    event_log: &mut EventLog,
    bench: &mut B,
) -> AlgorithmRun<CACHE> {
    let start = Node::new(args.start_lon, args.start_lat);
    let end = Node::new(args.end_lon, args.end_lat);
    let emit = |event| record_event(event_log, event);
    let window_parameters = [
        ("delta", bench_config.window_delta),
        ("max_batch", bench_config.window_batch),
    ];
    args.algo
        .run::<G, CACHE, _, B>(
            graph,
            start,
            end,
            RunOptions {
                policy: &bench_config.exploration_policy,
                state: &bench_config.bucket_state,
                parameters: if args.algo == BenchAlgo::AstarWindow {
                    &window_parameters
                } else {
                    &[]
                },
            },
            emit,
            bench,
        )
        .await
        .expect("configuration validée avant décodage")
}

pub async fn run() -> Result<()> {
    let args = WorkerArgs::parse()?;
    let mut bench_config = BenchConfig::from_env(args.algo)?;
    bench_config.apply_window_tuning(&args)?;
    let graph_storage = GraphStorage::parse(&bench_config.graph_storage)
        .ok_or_else(|| invalid("BENCH_GRAPH_STORAGE attendu: tile_dicho | tile_index"))?;
    bench_config.graph_storage = graph_storage.as_str().to_string();
    bench_config.validate_current_backend(args.algo)?;

    // DÉCODAGE GPS
    swap_cache::STATS.reset();
    let gps_path = resolve_data_path(&args.file)?;
    let bytes = std::fs::read(&gps_path)?;
    let mut reader = Reader::new(&bytes);
    let now_gps = Instant::now();

    let gps = Gps::<CACHE>::new(&mut reader).await.map_err(|err| {
        Error::new(
            ErrorKind::InvalidData,
            format!("échec du décodage du fichier GPS: {err}"),
        )
    })?;
    let duration_gps = now_gps.elapsed();
    let gps_stats = SCCount::get();
    swap_cache::STATS.reset();

    // CONSTRUCTION DU GRAPHE
    let now_graph = Instant::now();
    let graph = UGraph::new(&gps, args.order).await.map_err(|err| {
        Error::new(
            ErrorKind::InvalidData,
            format!("erreur lors de la construction du graphe: {err}"),
        )
    })?;
    let indexed = match graph_storage {
        GraphStorage::Dichotomic => None,
        GraphStorage::Indexed => Some(
            TileIndexedGraph::new(&graph)
                .await
                .map_err(|err| invalid(format!("construction de l'index de tuiles: {err}")))?,
        ),
    };
    let duration_graph = now_graph.elapsed();
    let graph_stats = SCCount::get();
    swap_cache::STATS.reset();

    // CALCUL DU PLUS COURT CHEMIN
    #[cfg(feature = "perf")]
    let mut bench = PerfBench;
    #[cfg(feature = "inspect")]
    let mut bench = InspectBench::new(args.inspect_stride, SWAP_CACHE_BLOCKS * SWAP_BLOCK_BYTES);

    let mode = bench.report().0;
    let mut event_log = EventLog::new(args.save_dir.as_deref())?;
    let now_algo = Instant::now();

    let algorithm_run = match &indexed {
        Some(indexed) => search(indexed, &args, &bench_config, &mut event_log, &mut bench).await,
        None => search(&graph, &args, &bench_config, &mut event_log, &mut bench).await,
    };

    // Mesurer les compteurs principaux du cache swap autour du même appel
    // d'algorithme que la durée réelle en mode perf. La destruction du chemin
    // et du résultat a lieu après les deux mesures.
    let (duration_algo, search_window_stats) = close_search_window(now_algo);
    let AlgorithmRun {
        mut result,
        metrics: specific_metrics,
    } = algorithm_run;

    bench.instant(InstantMetric::DataMicros, duration_gps.as_micros());
    bench.instant(InstantMetric::GraphMicros, duration_graph.as_micros());
    drop(result.path.take());
    // Garder le relevé historique après nettoyage pour le diagnostic et la compatibilité,
    // sans jamais l'utiliser comme groupe principal de compteurs de recherche.
    let search_stats_after_cleanup = SCCount::get();
    swap_cache::STATS.reset();
    let metrics = *bench.report().1;

    let cost = result.cost.unwrap_or_default();
    let path_len = result.path_len.unwrap_or_default();
    let summary = RunSummary {
        specific_metrics,
        found: result.found,
        cost,
        path_len,
        graph_index_entries: indexed.as_ref().map_or(0, |index| index.tile_count()),
        graph_index_logical_bytes: indexed
            .as_ref()
            .map_or(0, |index| index.tile_count() * index.entry_size()),
        graph_index_fallback_tiles: indexed
            .as_ref()
            .map_or(0, |index| index.fallback_tile_count()),
        decode_us: duration_gps.as_micros(),
        graph_us: duration_graph.as_micros(),
        // En mode perf, Bench reste sans effet ni surcoût. La durée de référence
        // est donc mesurée autour de l'appel complet à l'algorithme.
        // Le mode inspect distingue la recherche et la reconstruction.
        search_us: if cfg!(feature = "perf") {
            duration_algo.as_micros()
        } else {
            metrics.instant_value(InstantMetric::SearchMicros)
        },
        rebuild_us: if cfg!(feature = "inspect") {
            metrics.instant_value(InstantMetric::RebuildMicros)
        } else {
            0
        },
    };

    if let Some(out_dir) = &args.save_dir {
        save_run(
            &args,
            mode,
            &bench_config,
            &summary,
            &metrics,
            &gps_stats,
            &graph_stats,
            &search_window_stats,
            &search_stats_after_cleanup,
            &mut event_log,
            out_dir,
        )?;
    }

    if args.tuning_output {
        let tuning_summary =
            WindowTuningSummary::new(&bench_config, &summary, &search_window_stats);
        serde_json::to_writer(std::io::stdout().lock(), &tuning_summary).map_err(Error::other)?;
        println!();
    } else if args.saved_output {
        print_saved_summary(
            args.algo,
            &args.case_id,
            duration_algo.as_micros(),
            path_len,
        );
    } else {
        print_quick_summary(
            &args,
            mode,
            duration_algo.as_micros(),
            result.found,
            cost,
            path_len,
            &metrics,
            &bench_config,
            &specific_metrics,
            &search_window_stats,
            &event_log,
        );
    }

    if result.found {
        Ok(())
    } else {
        Err(Error::other("aucun chemin trouvé"))
    }
}

#[allow(clippy::too_many_arguments)]
fn save_run(
    args: &WorkerArgs,
    mode: &str,
    bench_config: &BenchConfig,
    summary: &RunSummary,
    metrics: &BenchMetrics,
    gps_stats: &SCCount,
    graph_stats: &SCCount,
    search_window_stats: &SCCount,
    search_stats_after_cleanup: &SCCount,
    event_log: &mut EventLog,
    out_dir: &Path,
) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;

    let date = Local::now();
    let config_slug = bench_config.slug();
    let out_filename = format!(
        "{}_{}_{}_{}_{}",
        args.case_id,
        args.algo.csv_name(),
        args.file,
        config_slug,
        date.format("%Y-%m-%d_%H-%M-%S")
    );
    let out_path = out_dir.join(&out_filename);
    let events_path =
        cfg!(feature = "inspect").then(|| out_dir.join(format!("{out_filename}.events.jsonl")));

    if let Some(path) = &events_path {
        event_log.finish(path)?;
    }

    let events_file = events_path
        .as_deref()
        .map(project_relative)
        .unwrap_or_default();
    let run_id = env::var("BENCH_RUN_ID").unwrap_or_default();

    let mut header = vec![
        "case_id".to_string(),
        "mode".to_string(),
        "timestamp".to_string(),
        "run_id".to_string(),
        "events_file".to_string(),
        "events_count".to_string(),
        "events_version".to_string(),
        "inspect_stride".to_string(),
        "file".to_string(),
        "algo".to_string(),
        "algo_backend".to_string(),
        "bucket_state".to_string(),
        "graph_storage".to_string(),
        "neighbor_access".to_string(),
        "exploration_policy".to_string(),
        "heuristic".to_string(),
        "bucket_tile_side".to_string(),
        "start_lon".to_string(),
        "start_lat".to_string(),
        "end_lon".to_string(),
        "end_lat".to_string(),
        "found".to_string(),
        "cost".to_string(),
        "path_len".to_string(),
        "decode_us".to_string(),
        "graph_us".to_string(),
        "graph_index_entries".to_string(),
        "graph_index_logical_bytes".to_string(),
        "graph_index_fallback_tiles".to_string(),
        "search_us".to_string(),
        "rebuild_us".to_string(),
        "swap_block_bytes".to_string(),
        "swap_cache_blocks".to_string(),
        "swap_cache_ko".to_string(),
        "swap_flash_blocks".to_string(),
        "swap_flash_ko".to_string(),
        "swap_profile".to_string(),
        "swap_latency".to_string(),
    ];
    for metric in INSTANT_METRICS
        .iter()
        .copied()
        .filter(|metric| metric.exported())
    {
        header.push(metric.name().to_string());
    }
    push_sccount_header(&mut header, "gps");
    push_sccount_header(&mut header, "graph");
    // Mesure autour de l'appel exact à l'algorithme, comme search_us.
    push_sccount_header(&mut header, "search_window");
    // Relevé historique de débogage après nettoyage de result.path.
    push_sccount_header(&mut header, "search_stats");
    push_sccount_header(&mut header, "search_total");
    for metric in STATS_METRICS.iter().copied() {
        push_scdetail_header(&mut header, metric.name());
    }

    let mut row = vec![
        args.case_id.clone(),
        mode.to_string(),
        date.format("%Y-%m-%d_%H-%M-%S").to_string(),
        run_id,
        events_file,
        event_log.total().to_string(),
        if cfg!(feature = "inspect") { "2.4" } else { "" }.to_string(),
        if cfg!(feature = "inspect") {
            args.inspect_stride.to_string()
        } else {
            String::new()
        },
        args.file.clone(),
        args.algo.csv_name().to_string(),
        args.algo.backend_name().to_string(),
        bench_config.bucket_state.clone(),
        bench_config.graph_storage.clone(),
        bench_config.neighbor_access.clone(),
        bench_config.exploration_policy.clone(),
        bench_config.heuristic.clone(),
        String::new(),
        args.start_lon.to_string(),
        args.start_lat.to_string(),
        args.end_lon.to_string(),
        args.end_lat.to_string(),
        u8::from(summary.found).to_string(),
        summary.cost.to_string(),
        summary.path_len.to_string(),
        summary.decode_us.to_string(),
        summary.graph_us.to_string(),
        summary.graph_index_entries.to_string(),
        summary.graph_index_logical_bytes.to_string(),
        summary.graph_index_fallback_tiles.to_string(),
        summary.search_us.to_string(),
        summary.rebuild_us.to_string(),
        SWAP_BLOCK_BYTES.to_string(),
        SWAP_CACHE_BLOCKS.to_string(),
        SWAP_CACHE_KO.to_string(),
        SWAP_FLASH_BLOCKS.to_string(),
        SWAP_FLASH_KO.to_string(),
        format!(
            "{}_{}",
            SWAP_BLOCK_BYTES,
            PROFILE_CAPACITY * SWAP_BLOCK_BYTES / 1024
        ),
        if cfg!(feature = "latency") {
            "simulated"
        } else {
            "yield"
        }
        .to_string(),
    ];
    for metric in INSTANT_METRICS
        .iter()
        .copied()
        .filter(|metric| metric.exported())
    {
        row.push(metrics.instant_value(metric).to_string());
    }
    push_sccount_values(&mut row, gps_stats);
    push_sccount_values(&mut row, graph_stats);
    push_sccount_values(&mut row, search_window_stats);
    push_sccount_values(&mut row, search_stats_after_cleanup);
    push_sccount_values(&mut row, &metrics.total);
    for metric in STATS_METRICS.iter().copied() {
        push_scdetail_values(&mut row, metrics.detail(metric));
    }

    summary
        .specific_metrics
        .validate(args.algo.descriptor().specific_metrics)
        .map_err(invalid)?;
    header.push("algorithm_ram_budget_bytes".into());
    row.push((SWAP_CACHE_BLOCKS * SWAP_BLOCK_BYTES).to_string());
    for key in SPECIFIC_METRIC_NAMES {
        if header.iter().any(|name| name == key) {
            return Err(invalid(format!("métrique spécifique dupliquée: {key}")));
        }
        header.push((*key).to_string());
        row.push(
            summary
                .specific_metrics
                .get(key)
                .map(|value| value.to_string())
                .unwrap_or_default(),
        );
    }
    let mut out_file = BufWriter::new(File::create(out_path)?);
    writeln!(out_file, "{}", header.join(","))?;
    writeln!(out_file, "{}", row.join(","))?;
    out_file.flush()
}

fn print_saved_summary(algo: BenchAlgo, case_id: &str, duration_us: u64, path_len: usize) {
    println!(
        "{} {} — {:.3} s — {} noeuds",
        algo.short_name(),
        case_id,
        seconds(duration_us),
        path_len
    );
}

#[allow(clippy::too_many_arguments)]
fn print_quick_summary(
    args: &WorkerArgs,
    mode: &str,
    duration_us: u64,
    found: bool,
    cost: u32,
    path_len: usize,
    metrics: &BenchMetrics,
    bench_config: &BenchConfig,
    specific_metrics: &SpecificMetrics,
    search_window_stats: &SCCount,
    events: &EventLog,
) {
    let latency = if cfg!(feature = "latency") {
        "latence"
    } else {
        "yield"
    };

    println!(
        "{} · {} · {} · {}Ko · {}",
        args.algo.short_name(),
        args.case_id,
        mode,
        SWAP_CACHE_KO,
        latency
    );
    println!(
        "{:.3} s · {} · coût {} · chemin {} noeuds",
        seconds(duration_us),
        if found { "trouvé" } else { "introuvable" },
        cost,
        path_len
    );

    if args.algo == BenchAlgo::AstarWindow {
        println!(
            "Window: {} candidats · delta {} · batch {} · swap-cache {} blocs",
            WINDOW_CAPACITY,
            bench_config.window_delta,
            bench_config.window_batch,
            SWAP_CACHE_BLOCKS,
        );
        println!(
            "priorité: {} pops au-delà du minimum de f · écart max {}",
            specific_metrics.get("tile_relaxed_pops").unwrap_or(0),
            specific_metrics.get("fslack_max").unwrap_or(0),
        );
    }

    if mode != "perf" {
        println!(
            "travail: {} visités · {:.1}% pops obsolètes · {:.1}% voisins ignorés · heap max {}",
            metrics.instant_value(InstantMetric::NodesVisited),
            ratio(
                metrics.instant_value(InstantMetric::NodesSkipped),
                metrics.instant_value(InstantMetric::HeapPops)
            ),
            ratio(
                metrics.instant_value(InstantMetric::NeighborsSkipped),
                metrics.instant_value(InstantMetric::EdgesVisited)
            ),
            metrics.instant_value(InstantMetric::MaxHeap),
        );
    }

    if mode == "inspect" {
        println!(
            "swap: {:.2}% hits · {} reads · {} writes",
            hit_ratio(search_window_stats),
            search_window_stats.flash_reads,
            search_window_stats.flash_writes,
        );
    }

    if mode == "inspect" {
        println!(
            "événements: {} total · {} pushed · {} popped · {} settled · {} relaxed · {} path",
            events.total(),
            events.counts.pushed,
            events.counts.popped,
            events.counts.settled,
            events.counts.relaxed,
            events.counts.path_nodes,
        );
    }
}

fn resolve_data_path(filename: &str) -> Result<PathBuf> {
    let direct = PathBuf::from(filename);
    if direct.exists() {
        return Ok(direct);
    }

    let path = env::current_dir()?.join("data").join(filename);
    if path.exists() {
        Ok(path)
    } else {
        Err(Error::new(
            ErrorKind::NotFound,
            format!("fichier GPS introuvable: {filename}"),
        ))
    }
}

fn project_relative(path: &Path) -> String {
    env::current_dir()
        .ok()
        .and_then(|root| {
            path.canonicalize()
                .ok()
                .and_then(|p| p.strip_prefix(root).ok().map(Path::to_path_buf))
        })
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn push_sccount_header(cols: &mut Vec<String>, prefix: &str) {
    SCCount::visit_names(|name| cols.push(format!("{prefix}_{name}")));
}

fn push_sccount_values(cols: &mut Vec<String>, sc: &SCCount) {
    sc.visit_values(|value| cols.push(value.to_string()));
}

fn push_scdetail_header(cols: &mut Vec<String>, prefix: &str) {
    cols.push(format!("{prefix}_calls"));
    push_sccount_header(cols, prefix);
}

fn push_scdetail_values(cols: &mut Vec<String>, detail: &SCDetail) {
    cols.push(detail.calls.to_string());
    push_sccount_values(cols, &detail.cache_counter);
}

fn env_value(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn slug_part(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn ratio(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

fn hit_ratio(stats: &SCCount) -> f64 {
    ratio(stats.cache_hits, stats.cache_hits + stats.cache_misses)
}

fn seconds(micros: u64) -> f64 {
    micros as f64 / 1_000_000.0
}

fn next(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String> {
    args.next()
        .ok_or_else(|| invalid(format!("valeur manquante après {option}")))
}

fn read_point(args: &mut impl Iterator<Item = String>, option: &str) -> Result<(f64, f64)> {
    let lon = parse_f64(&next(args, option)?, "longitude")?;
    let lat = parse_f64(&next(args, option)?, "latitude")?;
    Ok((lon, lat))
}

fn parse_f64(value: &str, label: &str) -> Result<f64> {
    value
        .parse::<f64>()
        .map_err(|_| invalid(format!("{label} invalide: {value}")))
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidInput, message.into())
}
