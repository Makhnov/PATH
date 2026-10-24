use lib_gps::algo::registry::{Algorithm, AlgorithmDescriptor, CATALOG_JSON};
use lib_gps::graph::GNode;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::path::{Path, PathBuf};

const CATALOG_PATH: &str = "bench/catalog.csv";
const EVENTS_INDEX_PATH: &str = "bench/results/events/index.csv";
const SCENARIOS_PATH: &str = "bench/scenarios.csv";

const DEFAULT_SWAP_PROFILE: &str = "256_128";
const DEFAULT_ALGO: &str = "astar_lazy";
const DEFAULT_GPS_LAYOUT: &str = "base";
const DEFAULT_GRAPH_STORAGE: &str = "tile_dicho";
const DEFAULT_NEIGHBOR_ACCESS: &str = "tile_segments";
const DEFAULT_EXPLORATION_POLICY: &str = "strict";
const DEFAULT_BUCKET_STATE: &str = "coupled";
const DEFAULT_HEURISTIC: &str = "standard";
const DATASET_OPTION_KEYS: [&str; 7] = [
    "gps_layout",
    "graph_storage",
    "neighbor_access",
    "exploration_policy",
    "bucket_state",
    "heuristic",
    "swap_profile",
];

const SC_DETAIL_PREFIXES: &[&str] = &[
    "heap_push",
    "heap_pop",
    "done_get",
    "done_insert",
    "done_remove",
    "previous_get",
    "previous_insert",
    "global_push",
    "global_pop",
    "local_push",
    "local_pop",
    "states_get",
    "states_insert",
    "states_remove",
    "neighbors_open",
    "neighbors_next",
    "rebuild",
];

const ALGORITHM_WORK_GRAPH_PREFIXES: &[&str] = &["neighbors_open", "neighbors_next"];
const ALGORITHM_WORK_FRONTIER_PREFIXES: &[&str] = &[
    "heap_push",
    "heap_pop",
    "global_push",
    "global_pop",
    "local_push",
    "local_pop",
];
const ALGORITHM_WORK_STATE_PREFIXES: &[&str] = &[
    "done_get",
    "done_insert",
    "done_remove",
    "previous_get",
    "previous_insert",
    "states_get",
    "states_insert",
    "states_remove",
];

const DERIVED_METRICS: &[&str] = &[
    "decode_ms",
    "graph_ms",
    "search_ms",
    "total_ms",
    "measured_search_ms",
    "search_flash_measured_ms",
    "search_flash_estimated_ms",
    "search_residual_after_measured_flash_ms",
    "search_residual_after_estimated_flash_ms",
    "search_cache_accesses",
    "search_cache_hit_rate",
    "search_cache_miss_rate",
    "search_cache_misses_per_million",
    "stale_pop_ratio",
    "pop_skipped_ratio",
    "neighbors_skipped_ratio",
    "neighbors_visited_ratio",
    "reads_per_visited_node",
    "writes_per_visited_node",
    "visited_per_path_node",
    "heuristic_per_visited_node",
    "graph_edge_work",
    "algorithmic_work",
    "algorithmic_work_graph",
    "algorithmic_work_frontier",
    "algorithmic_work_state",
    "algorithmic_work_heuristic",
    "queue_operations",
    "max_heap_bytes",
    "max_done_bytes",
    "max_previous_bytes",
    "max_global_bytes",
    "max_local_bytes",
    "max_states_bytes",
    "max_collection_bytes",
    "popped_cross_tile_rate",
    "skipped_cross_tile_rate",
    "visited_cross_tile_rate",
    "avg_popped_tile_jump",
    "avg_skipped_tile_jump",
    "avg_visited_tile_jump",
    "heap_push_cross_tile_rate",
    "neighbors_next_cross_tile_rate",
    "neighbors_open_cross_rate",
    "edges_from_border_node",
    "edges_from_interior_node_rate",
    "edges_from_left_node_rate",
    "edges_from_bottom_node_rate",
    "edges_from_bottom_left_node_rate",
    "edges_from_border_node_rate",
];

#[derive(Debug, Clone)]
pub struct BenchStore {
    root: PathBuf,
    scenarios: Vec<BenchScenario>,
    datasets: Vec<BenchDataset>,
    rows: HashMap<String, Vec<BenchCsvRow>>,
    event_rows: Vec<BenchCsvRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchScenario {
    pub name: String,
    pub file: String,
    pub start_lon: f64,
    pub start_lat: f64,
    pub end_lon: f64,
    pub end_lat: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchDataset {
    pub id: String,
    pub label: String,
    pub csv: String,
    pub gps_file: String,
    pub row_count: usize,
    pub options: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct BenchCsvRow {
    values: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchConfigResponse {
    pub scenarios: Vec<BenchScenario>,
    pub option_groups: Vec<BenchOptionGroup>,
    pub baseline_algorithms: Vec<BenchOption>,
    pub algorithms: Vec<serde_json::Value>,
    pub datasets: Vec<BenchDataset>,
    pub defaults: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchOptionGroup {
    pub id: String,
    pub label: String,
    pub mode: String,
    pub default: String,
    pub options: Vec<BenchOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BenchResultQuery {
    pub swap_profile: Option<String>,
    pub case_id: String,
    pub algo: Option<String>,
    pub baseline_algo: Option<String>,
    pub gps_layout: Option<String>,
    pub graph_storage: Option<String>,
    pub neighbor_access: Option<String>,
    pub exploration_policy: Option<String>,
    pub bucket_state: Option<String>,
    pub heuristic: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchResultResponse {
    pub status: String,
    pub message: String,
    pub query: BenchResolvedQuery,
    pub baseline_query: BenchResolvedQuery,
    pub baseline_exact_query: BenchResolvedQuery,
    pub baseline_mode: String,
    pub baseline_note: Option<String>,
    pub scenario: Option<BenchScenario>,
    pub selected: Option<BenchAggregate>,
    pub baseline: Option<BenchAggregate>,
    pub graph_storage_comparison: BTreeMap<String, BenchAggregate>,
    pub work_algorithms: BTreeMap<String, BenchAggregate>,
    pub delta: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VisualizeResultResponse {
    pub status: String,
    pub message: String,
    pub query: BenchResolvedQuery,
    pub scenario: Option<BenchScenario>,
    pub run: Option<VisualizeRun>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VisualizeRun {
    pub dataset_id: String,
    pub dataset_label: String,
    pub run_id: String,
    pub timestamp: String,
    pub events_file: String,
    pub events_count: u64,
    pub events_version: String,
    pub inspect_stride: u64,
    pub found: Option<String>,
    pub cost: Option<String>,
    pub path_len: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BenchResolvedQuery {
    pub swap_profile: String,
    pub case_id: String,
    pub algo: String,
    pub gps_layout: String,
    pub graph_storage: String,
    pub neighbor_access: String,
    pub exploration_policy: String,
    pub bucket_state: String,
    pub heuristic: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchAggregate {
    pub dataset_id: String,
    pub dataset_label: String,
    pub csv: String,
    pub gps_file: String,
    pub runs_total: usize,
    pub runs_perf: usize,
    pub runs_inspect: usize,
    pub aggregation: String,
    pub run_ids: Vec<String>,
    pub timestamps: Vec<String>,
    pub raw_dirs: Vec<String>,
    pub metric_samples: BTreeMap<String, usize>,
    pub validation: BenchValidation,
    pub metrics: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchValidation {
    pub found: Option<String>,
    pub found_stable: bool,
    pub cost: Option<String>,
    pub cost_stable: bool,
    pub path_len: Option<String>,
    pub path_len_stable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetricSource {
    Perf,
    Inspect,
}

impl BenchStore {
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let mut datasets = load_catalog(&root.join(CATALOG_PATH))?;

        let mut rows = HashMap::new();

        for dataset in &mut datasets {
            let path = root.join(&dataset.csv);
            let dataset_rows = if path.exists() {
                load_enriched_rows(&path)?
            } else {
                Vec::new()
            };

            dataset.row_count = dataset_rows.len();
            rows.insert(dataset.id.clone(), dataset_rows);
        }

        // COnserver les datasets ayant un run correspondant au fichier et aux options du catalogue
        datasets.retain(|dataset| {
            rows.get(&dataset.id).is_some_and(|rows| {
                rows.iter().any(|row| {
                    row_file_matches_dataset(row, dataset)
                        && DATASET_OPTION_KEYS.iter().all(|key| {
                            let row_value = match *key {
                                "gps_layout" => row_layout(row),
                                "bucket_state" => row_bucket_state(row),
                                _ => row_option_or_default(row, key),
                            };
                            let dataset_value = option_value(dataset, key);
                            row_value == dataset_value
                                || (*key == "exploration_policy"
                                    && row.get(key).unwrap_or("") == dataset_value)
                                || (*key == "bucket_state"
                                    && dataset_value.is_empty()
                                    && row.get(key).unwrap_or("").trim().is_empty())
                        })
                })
            })
        });

        let events_path = root.join(EVENTS_INDEX_PATH);
        let mut event_rows = if events_path.exists() {
            load_enriched_rows(&events_path)?
        } else {
            Vec::new()
        };
        restore_replay_parameters(&rows, &mut event_rows);
        let scenarios_path = std::env::var_os("BENCH_SCENARIOS_PATH")
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    root.join(path)
                }
            })
            .unwrap_or_else(|| root.join(SCENARIOS_PATH));
        let scenarios = scenarios_from_sources(&scenarios_path, &rows, &event_rows)?;

        Ok(Self {
            root,
            scenarios,
            datasets,
            rows,
            event_rows,
        })
    }

    pub fn config(&self) -> BenchConfigResponse {
        BenchConfigResponse {
            scenarios: self.scenarios.clone(),
            option_groups: option_groups(),
            baseline_algorithms: comparison_algorithms(),
            algorithms: registry_entries().clone(),
            datasets: self.datasets.clone(),
            defaults: default_selection(),
        }
    }

    pub fn query(&self, query: BenchResultQuery) -> BenchResultResponse {
        let baseline_algo = query
            .baseline_algo
            .clone()
            .unwrap_or_else(|| DEFAULT_ALGO.to_string());
        let resolved = resolve_query(query);

        let scenario = self
            .scenarios
            .iter()
            .find(|scenario| scenario.name == resolved.case_id)
            .cloned();

        let selected_dataset = self.find_dataset(&resolved);
        let selected = selected_dataset.and_then(|dataset| self.aggregate(dataset, &resolved));

        // Comparer les stockages du graphe avec la même configuration de recherche. (inspect & perf séparés et conservés)
        let mut graph_storage_comparison = BTreeMap::new();
        for storage in ["tile_dicho", "tile_index"] {
            let aggregate = if storage == resolved.graph_storage {
                selected.clone()
            } else {
                let storage_query = BenchResolvedQuery {
                    graph_storage: storage.to_string(),
                    ..resolved.clone()
                };
                self.find_dataset(&storage_query)
                    .filter(|dataset| {
                        selected_dataset.is_none_or(|selected| {
                            file_basename(&dataset.gps_file) == file_basename(&selected.gps_file)
                        })
                    })
                    .and_then(|dataset| self.aggregate(dataset, &storage_query))
            };
            if let Some(aggregate) = aggregate {
                graph_storage_comparison.insert(storage.to_string(), aggregate);
            }
        }

        // TCEPA : choisir seulement l'algorithme de référence. Les options communes restent identiques (les options propres à l'algorithme s'appliquent si possible)
        let baseline_exact_query = comparison_query(&resolved, &baseline_algo);

        let exact_baseline = self
            .find_dataset(&baseline_exact_query)
            .filter(|dataset| {
                selected_dataset.is_none_or(|selected| {
                    file_basename(&dataset.gps_file) == file_basename(&selected.gps_file)
                })
            })
            .and_then(|dataset| self.aggregate(dataset, &baseline_exact_query));

        let baseline_fallback_query = legacy_strict_fallback_query(&resolved, &baseline_algo);
        let fallback_allowed = baseline_fallback_query != baseline_exact_query;
        let fallback_baseline = if exact_baseline.is_none() && fallback_allowed {
            self.find_dataset(&baseline_fallback_query)
                .filter(|dataset| {
                    selected_dataset.is_none_or(|selected| {
                        file_basename(&dataset.gps_file) == file_basename(&selected.gps_file)
                    })
                })
                .and_then(|dataset| self.aggregate(dataset, &baseline_fallback_query))
        } else {
            None
        };

        let (baseline_query, baseline, baseline_mode, baseline_note) = if let Some(aggregate) =
            exact_baseline.clone()
        {
            (
                baseline_exact_query.clone(),
                Some(aggregate),
                "tcepa".to_string(),
                None,
            )
        } else if let Some(aggregate) = fallback_baseline {
            (
                baseline_fallback_query.clone(),
                Some(aggregate),
                "legacy_strict_fallback".to_string(),
                Some(format!(
                    "Comparaison exacte TCEPA indisponible ; comparaison affichée avec l’équivalent strict legacy de {}. Toutes les options compatibles sont conservées ; seules les options non supportées par la référence reviennent à leur équivalent legacy.",
                    algorithm_label(&baseline_fallback_query.algo)
                )),
            )
        } else {
            (
                baseline_exact_query.clone(),
                None,
                "missing".to_string(),
                None,
            )
        };

        let mut work_algorithms = BTreeMap::new();
        for variant in comparison_algorithms() {
            let peer_query = comparison_query(&resolved, &variant.id);
            if let Some(aggregate) = self
                .find_dataset(&peer_query)
                .filter(|dataset| {
                    selected_dataset.is_none_or(|selected| {
                        file_basename(&dataset.gps_file) == file_basename(&selected.gps_file)
                    })
                })
                .and_then(|dataset| self.aggregate(dataset, &peer_query))
            {
                work_algorithms.insert(variant.id, aggregate);
            }
        }

        let delta = match (&selected, &baseline) {
            (Some(selected), Some(baseline)) => compute_delta(selected, baseline),
            _ => BTreeMap::new(),
        };

        let status = if selected.is_some() { "ok" } else { "no_data" };
        let message = if selected.is_some() {
            if baseline.is_some() {
                baseline_note
                    .clone()
                    .unwrap_or_else(|| "Données disponibles pour cette combinaison.".to_string())
            } else {
                "Données disponibles pour la sélection. Référence indisponible pour cette configuration TCEPA."
                    .to_string()
            }
        } else if selected_dataset.is_none() {
            "Aucun dataset ne correspond à cette combinaison d'options.".to_string()
        } else {
            "Dataset trouvé, mais aucun run ne correspond à ce scénario et cet algo.".to_string()
        };

        BenchResultResponse {
            status: status.to_string(),
            message,
            query: resolved,
            baseline_query,
            baseline_exact_query,
            baseline_mode,
            baseline_note,
            scenario,
            selected,
            baseline,
            graph_storage_comparison,
            work_algorithms,
            delta,
        }
    }

    pub fn visualize(&self, query: BenchResultQuery) -> VisualizeResultResponse {
        let resolved = resolve_query(query);
        let scenario = self
            .scenarios
            .iter()
            .find(|scenario| scenario.name == resolved.case_id)
            .cloned();

        let run = self.find_visualize_row(&resolved).map(|row| {
            let dataset = self.find_dataset(&resolved);
            VisualizeRun {
                dataset_id: dataset
                    .map(|dataset| dataset.id.clone())
                    .unwrap_or_else(|| event_dataset_id(&resolved)),
                dataset_label: dataset
                    .map(|dataset| dataset.label.clone())
                    .unwrap_or_else(|| event_dataset_label(&resolved)),
                run_id: row.get("run_id").unwrap_or("").to_string(),
                timestamp: row.get("timestamp").unwrap_or("").to_string(),
                events_file: row.get("events_file").unwrap_or("").to_string(),
                events_count: row.u64("events_count").unwrap_or(0),
                events_version: row.get("events_version").unwrap_or("1").to_string(),
                inspect_stride: row.u64("inspect_stride").unwrap_or(1).max(1),
                found: row.get("found").map(ToOwned::to_owned),
                cost: row.get("cost").map(ToOwned::to_owned),
                path_len: row.get("path_len").map(ToOwned::to_owned),
            }
        });

        let (status, message) = if run.is_some() {
            (
                "ok",
                "Replay instrumenté disponible pour cette combinaison.",
            )
        } else {
            (
                "no_data",
                "Aucun run instrumenté sauvegardé ne correspond à ce scénario et cette configuration.",
            )
        };

        VisualizeResultResponse {
            status: status.to_string(),
            message: message.to_string(),
            query: resolved,
            scenario,
            run,
        }
    }

    pub fn visualize_events_path(&self, query: BenchResultQuery) -> Result<Option<PathBuf>> {
        let resolved = resolve_query(query);
        let Some(row) = self.find_visualize_row(&resolved) else {
            return Ok(None);
        };
        let Some(value) = row
            .get("events_file")
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };

        let path = if Path::new(value).is_absolute() {
            PathBuf::from(value)
        } else {
            self.root.join(value)
        };

        if !path.exists() {
            return Ok(None);
        }

        let root = self.root.canonicalize()?;
        let path = path.canonicalize()?;
        if !path.starts_with(&root) {
            return Err(Error::new(
                ErrorKind::PermissionDenied,
                "fichier d'événements hors du projet",
            ));
        }

        Ok(Some(path))
    }

    fn find_visualize_row(&self, query: &BenchResolvedQuery) -> Option<&BenchCsvRow> {
        let indexed = self
            .event_rows
            .iter()
            .filter(|row| row.has_replay())
            .filter(|row| row.get("case_id") == Some(query.case_id.as_str()))
            .filter(|row| row_layout_matches_query(row, query))
            .filter(|row| row_options_match_query(row, query))
            .filter(|row| row.get("swap_profile") == Some(query.swap_profile.as_str()))
            .filter(|row| normalize_algo(row.get("algo").unwrap_or("")) == query.algo)
            .filter(|row| {
                row.get("events_file")
                    .is_some_and(|value| !value.trim().is_empty())
            })
            .max_by(|left, right| {
                left.get("timestamp")
                    .unwrap_or("")
                    .cmp(right.get("timestamp").unwrap_or(""))
            });

        if indexed.is_some() {
            return indexed;
        }

        // Si l'index ne fournit aucun replay correspondant, on vérifie dans les anciens CSV
        self.rows
            .values()
            .flat_map(|rows| rows.iter())
            .filter(|row| row.has_replay())
            .filter(|row| row.get("case_id") == Some(query.case_id.as_str()))
            .filter(|row| row_layout_matches_query(row, query))
            .filter(|row| row_options_match_query(row, query))
            .filter(|row| row.get("swap_profile") == Some(query.swap_profile.as_str()))
            .filter(|row| normalize_algo(row.get("algo").unwrap_or("")) == query.algo)
            .filter(|row| {
                row.get("events_file")
                    .is_some_and(|value| !value.trim().is_empty())
            })
            .max_by(|left, right| {
                left.get("timestamp")
                    .unwrap_or("")
                    .cmp(right.get("timestamp").unwrap_or(""))
            })
    }

    fn find_dataset(&self, query: &BenchResolvedQuery) -> Option<&BenchDataset> {
        let options_match = |dataset: &&BenchDataset| {
            option_value(dataset, "gps_layout") == query.gps_layout
                && option_value(dataset, "swap_profile") == query.swap_profile
                && option_value(dataset, "graph_storage") == query.graph_storage
                && option_value(dataset, "neighbor_access") == query.neighbor_access
                && option_value(dataset, "exploration_policy") == query.exploration_policy
                && (option_value(dataset, "bucket_state") == query.bucket_state
                    || (query.algo == "astar_countdown"
                        && query.bucket_state == "coupled"
                        && option_value(dataset, "bucket_state").is_empty()))
                && option_value(dataset, "heuristic") == query.heuristic
        };
        self.datasets.iter().find(options_match)
    }

    fn aggregate(
        &self,
        dataset: &BenchDataset,
        query: &BenchResolvedQuery,
    ) -> Option<BenchAggregate> {
        let rows = self.rows.get(&dataset.id)?;
        let matching: Vec<&BenchCsvRow> = rows
            .iter()
            .filter(|row| row.get("case_id") == Some(query.case_id.as_str()))
            .filter(|row| row_file_matches_dataset(row, dataset))
            .filter(|row| row_options_match_query(row, query))
            .filter(|row| normalize_algo(row.get("algo").unwrap_or("")) == query.algo)
            .collect();

        if matching.is_empty() {
            return None;
        }

        let runs_inspect = matching.iter().filter(|row| row.is_inspect()).count();
        let runs_perf = matching.iter().filter(|row| row.is_perf()).count();

        let mut metric_names: HashSet<String> = HashSet::new();
        for row in &matching {
            for (key, value) in &row.values {
                if value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .is_some_and(|v| v.is_finite())
                {
                    metric_names.insert(key.clone());
                }
            }
        }

        for row in &matching {
            if let Some(items) = registry_entry(row.get("algo").unwrap_or(""))
                .and_then(|entry| entry["collections"].as_array())
            {
                for item in items {
                    if let Some(key) = item["key"].as_str() {
                        metric_names.insert(key.to_string());
                    }
                }
            }
        }

        for metric in DERIVED_METRICS {
            metric_names.insert((*metric).to_string());
        }

        for &prefix in SC_DETAIL_PREFIXES {
            for suffix in [
                "cache_accesses",
                "cache_hit_rate",
                "cache_miss_rate",
                "flash_measured_ms",
                "flash_estimated_ms",
                "reads_per_call",
                "writes_per_call",
                "measured_ms_per_call",
                "estimated_ms_per_call",
            ] {
                metric_names.insert(format!("{prefix}_{suffix}"));
            }
        }

        let mut metrics = BTreeMap::new();
        let mut metric_samples = BTreeMap::new();
        for metric in metric_names {
            let source = metric_source(&metric);
            let mut values = Vec::new();

            for row in &matching {
                if source == MetricSource::Perf && !row.is_perf() {
                    continue;
                }
                if source == MetricSource::Inspect && !row.is_inspect() {
                    continue;
                }
                if let Some(value) = row.metric(&metric) {
                    values.push(value);
                }
            }

            if !values.is_empty() {
                metric_samples.insert(metric.clone(), values.len());
                metrics.insert(metric, values.iter().sum::<f64>() / values.len() as f64);
            }
        }

        // Estimation du temps de recherche (hors E/S) = Durée totale(perf) - E/S (inspect)
        if let Some(search_ms) = metrics.get("search_ms").copied() {
            for (flash_metric, residual_metric) in [
                (
                    "search_flash_measured_ms",
                    "search_residual_after_measured_flash_ms",
                ),
                (
                    "search_flash_estimated_ms",
                    "search_residual_after_estimated_flash_ms",
                ),
            ] {
                if let Some(flash_ms) = metrics.get(flash_metric).copied() {
                    metrics.insert(residual_metric.to_string(), search_ms - flash_ms);
                    let samples = metric_samples
                        .get("search_ms")
                        .copied()
                        .unwrap_or(0)
                        .min(metric_samples.get(flash_metric).copied().unwrap_or(0));
                    if samples > 0 {
                        metric_samples.insert(residual_metric.to_string(), samples);
                    }
                }
            }
        }

        let mut run_ids = matching
            .iter()
            .filter_map(|row| row.get("run_id"))
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        run_ids.sort();
        run_ids.dedup();

        let mut timestamps = matching
            .iter()
            .filter_map(|row| row.get("timestamp"))
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        timestamps.sort();
        timestamps.dedup();

        let mut raw_dirs = matching
            .iter()
            .filter_map(|row| row.get("raw_dir"))
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        raw_dirs.sort();
        raw_dirs.dedup();

        Some(BenchAggregate {
            dataset_id: dataset.id.clone(),
            dataset_label: dataset.label.clone(),
            csv: dataset.csv.clone(),
            gps_file: dataset.gps_file.clone(),
            runs_total: matching.len(),
            runs_perf,
            runs_inspect,
            aggregation: "paired".to_string(),
            run_ids,
            timestamps,
            raw_dirs,
            metric_samples,
            validation: validation(&matching),
            metrics,
        })
    }
}

impl BenchCsvRow {
    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    fn is_perf(&self) -> bool {
        matches!(
            self.get("mode"),
            Some("perf") | Some("none") | Some("normal")
        ) || (self.get("mode").is_none() && !self.is_inspect())
    }

    fn is_inspect(&self) -> bool {
        matches!(
            self.get("mode"),
            Some("inspect") | Some("stats") | Some("full")
        ) || matches!(self.get("full_mode"), Some("true") | Some("1"))
            || matches!(self.get("profile"), Some("full"))
    }

    fn has_replay(&self) -> bool {
        self.is_inspect()
            && self
                .get("events_file")
                .is_some_and(|value| !value.trim().is_empty())
    }

    fn u64(&self, key: &str) -> Option<u64> {
        self.get(key)?.trim().parse::<u64>().ok()
    }

    fn number(&self, key: &str) -> Option<f64> {
        let raw = self.get(key)?.trim();
        if raw.is_empty() {
            return None;
        }
        raw.parse::<f64>().ok().filter(|v| v.is_finite())
    }

    fn number_any(&self, keys: &[&str]) -> Option<f64> {
        keys.iter().find_map(|key| self.number(key))
    }

    fn search_counter(&self, field: &str) -> Option<f64> {
        self.number(&format!("search_window_{field}"))
            .or_else(|| self.number(&format!("search_total_{field}")))
            .or_else(|| self.number(&format!("search_stats_{field}")))
            .or_else(|| self.number(&format!("search_{field}")))
    }

    fn detail_calls_sum(&self, prefixes: &[&str]) -> f64 {
        prefixes
            .iter()
            .map(|prefix| self.number(&format!("{prefix}_calls")).unwrap_or(0.0))
            .sum()
    }

    fn metric(&self, key: &str) -> Option<f64> {
        if registry_entry(self.get("algo").unwrap_or(""))
            .and_then(|entry| entry["collections"].as_array())
            .is_some_and(|items| items.iter().any(|item| item["key"].as_str() == Some(key)))
        {
            return collection_metric_bytes(self, key);
        }

        match key {
            "decode_ms" => self.number("decode_us").map(|v| v / 1000.0),
            "graph_ms" => self.number("graph_us").map(|v| v / 1000.0),
            "search_ms" | "measured_search_ms" => self
                .number("search_ms")
                .or_else(|| self.number("search_us").map(|v| v / 1000.0)),
            "rebuild_ms" => self.number("rebuild_us").map(|v| v / 1000.0),
            "total_ms" => Some(
                self.number("decode_us")? / 1000.0
                    + self.number("graph_us")? / 1000.0
                    + self.number("search_us")? / 1000.0
                    + self.number("rebuild_us").unwrap_or(0.0) / 1000.0,
            ),
            "heap_pushes" => self.number_any(&["heap_pushes", "pushed"]),
            "heap_pops" => self.number_any(&["heap_pops", "popped"]),
            "nodes_skipped" => self.number_any(&["nodes_skipped", "skipped"]),
            "nodes_visited" => self.number_any(&["nodes_visited", "settled"]),
            "neighbors_skipped" => self.number_any(&["neighbors_skipped", "ignored"]),
            "neighbors_visited" => self.number_any(&["neighbors_visited", "visited"]),
            "max_heap" => self.number_any(&["max_heap", "max_open_set_len"]),
            "search_cache_hits" => self.search_counter("cache_hits"),
            "search_cache_misses" => self.search_counter("cache_misses"),
            "search_flash_reads" => self.search_counter("flash_reads"),
            "search_flash_writes" => self.search_counter("flash_writes"),
            "search_flash_reads_micros" => self.search_counter("flash_reads_micros"),
            "search_flash_writes_micros" => self.search_counter("flash_writes_micros"),
            "search_cache_accesses" => Some(
                self.metric("search_cache_hits").unwrap_or(0.0)
                    + self.metric("search_cache_misses").unwrap_or(0.0),
            ),
            "search_cache_hit_rate" => div(
                self.metric("search_cache_hits")?,
                self.metric("search_cache_accesses")?,
            ),
            "search_cache_miss_rate" => div(
                self.metric("search_cache_misses")?,
                self.metric("search_cache_accesses")?,
            ),
            "search_cache_misses_per_million" => div(
                self.metric("search_cache_misses")? * 1_000_000.0,
                self.metric("search_cache_accesses")?,
            ),
            "search_flash_measured_ms" => Some(
                (self.metric("search_flash_reads_micros").unwrap_or(0.0)
                    + self.metric("search_flash_writes_micros").unwrap_or(0.0))
                    / 1000.0,
            ),
            "search_flash_estimated_ms" => Some(
                self.metric("search_flash_reads").unwrap_or(0.0)
                    + 3.0 * self.metric("search_flash_writes").unwrap_or(0.0),
            ),
            "search_residual_after_measured_flash_ms" => {
                Some(self.metric("search_ms")? - self.metric("search_flash_measured_ms")?)
            }
            "search_residual_after_estimated_flash_ms" => {
                Some(self.metric("search_ms")? - self.metric("search_flash_estimated_ms")?)
            }
            "stale_pop_ratio" | "pop_skipped_ratio" => {
                div(self.metric("nodes_skipped")?, self.metric("heap_pops")?)
            }
            "neighbors_skipped_ratio" => div(
                self.metric("neighbors_skipped")?,
                self.metric("edges_visited")?,
            ),
            "neighbors_visited_ratio" => div(
                self.metric("neighbors_visited")?,
                self.metric("edges_visited")?,
            ),
            "reads_per_visited_node" => div(
                self.metric("search_flash_reads")?,
                self.metric("nodes_visited")?,
            ),
            "writes_per_visited_node" => div(
                self.metric("search_flash_writes")?,
                self.metric("nodes_visited")?,
            ),
            "visited_per_path_node" => div(self.metric("nodes_visited")?, self.metric("path_len")?),
            "heuristic_per_visited_node" => div(
                self.metric("heuristic_calls")?,
                self.metric("nodes_visited")?,
            ),
            "graph_edge_work" => {
                Some(self.metric("edges_visited")? + self.number("countdown_edges").unwrap_or(0.0))
            }
            "algorithmic_work_graph" => Some(self.detail_calls_sum(ALGORITHM_WORK_GRAPH_PREFIXES)),
            "algorithmic_work_frontier" => {
                Some(self.detail_calls_sum(ALGORITHM_WORK_FRONTIER_PREFIXES))
            }
            "algorithmic_work_state" => Some(self.detail_calls_sum(ALGORITHM_WORK_STATE_PREFIXES)),
            "algorithmic_work_heuristic" => self.number("heuristic_calls"),

            // Expérimental (pas sûr de la pertinence) = Total des appels comptés ()chaque appel vaut 1, quelle que soit sa durée)
            "algorithmic_work" => Some(
                self.metric("algorithmic_work_graph")?
                    + self.metric("algorithmic_work_frontier")?
                    + self.metric("algorithmic_work_state")?
                    + self.metric("algorithmic_work_heuristic").unwrap_or(0.0),
            ),
            "queue_operations" => Some(
                self.metric("heap_pushes")?
                    + self.metric("heap_pops")?
                    + self.number("global_pops").unwrap_or(0.0),
            ),
            "max_heap_bytes" | "max_done_bytes" | "max_previous_bytes" | "max_global_bytes"
            | "max_local_bytes" | "max_states_bytes" => collection_metric_bytes(self, key),
            "max_collection_bytes" => max_collection_bytes(self),
            "popped_cross_tile_rate" => div(
                self.number("popped_cross_tile")?,
                self.number("popped_same_tile").unwrap_or(0.0)
                    + self.number("popped_cross_tile").unwrap_or(0.0),
            ),
            "skipped_cross_tile_rate" => div(
                self.number("skipped_cross_tile")?,
                self.number("skipped_same_tile").unwrap_or(0.0)
                    + self.number("skipped_cross_tile").unwrap_or(0.0),
            ),
            "visited_cross_tile_rate" => div(
                self.number("visited_cross_tile")?,
                self.number("visited_same_tile").unwrap_or(0.0)
                    + self.number("visited_cross_tile").unwrap_or(0.0),
            ),
            "avg_popped_tile_jump" => div(
                self.number("popped_tile_jump_sum")?,
                self.number("popped_cross_tile")?,
            ),
            "avg_skipped_tile_jump" => div(
                self.number("skipped_tile_jump_sum")?,
                self.number("skipped_cross_tile")?,
            ),
            "avg_visited_tile_jump" => div(
                self.number("visited_tile_jump_sum")?,
                self.number("visited_cross_tile")?,
            ),
            "heap_push_cross_tile_rate" => div(
                self.number("heap_push_cross_tile_calls")?,
                self.number("heap_push_same_tile_calls").unwrap_or(0.0)
                    + self.number("heap_push_cross_tile_calls").unwrap_or(0.0),
            ),
            "neighbors_next_cross_tile_rate" => div(
                self.number("neighbors_next_cross_tile_calls")?,
                self.number("neighbors_next_same_tile_calls").unwrap_or(0.0)
                    + self
                        .number("neighbors_next_cross_tile_calls")
                        .unwrap_or(0.0),
            ),
            "neighbors_open_cross_rate" => div(
                self.number("neighbors_cross_left_calls").unwrap_or(0.0)
                    + self.number("neighbors_cross_bottom_calls").unwrap_or(0.0)
                    + self
                        .number("neighbors_cross_bottom_left_calls")
                        .unwrap_or(0.0),
                self.number("neighbors_same_tile_calls").unwrap_or(0.0)
                    + self.number("neighbors_cross_left_calls").unwrap_or(0.0)
                    + self.number("neighbors_cross_bottom_calls").unwrap_or(0.0)
                    + self
                        .number("neighbors_cross_bottom_left_calls")
                        .unwrap_or(0.0),
            ),
            "edges_from_border_node" => {
                let left = self.number("edges_from_left_node");
                let bottom = self.number("edges_from_bottom_node");
                let bottom_left = self.number("edges_from_bottom_left_node");
                if left.is_none() && bottom.is_none() && bottom_left.is_none() {
                    None
                } else {
                    Some(left.unwrap_or(0.0) + bottom.unwrap_or(0.0) + bottom_left.unwrap_or(0.0))
                }
            }
            "edges_from_interior_node_rate" => div(
                self.number("edges_from_same_node")?,
                self.metric("edges_visited")?,
            ),
            "edges_from_left_node_rate" => div(
                self.number("edges_from_left_node")?,
                self.metric("edges_visited")?,
            ),
            "edges_from_bottom_node_rate" => div(
                self.number("edges_from_bottom_node")?,
                self.metric("edges_visited")?,
            ),
            "edges_from_bottom_left_node_rate" => div(
                self.number("edges_from_bottom_left_node")?,
                self.metric("edges_visited")?,
            ),
            "edges_from_border_node_rate" => div(
                self.metric("edges_from_border_node")?,
                self.metric("edges_visited")?,
            ),
            _ => self.derived_detail_metric(key).or_else(|| self.number(key)),
        }
    }

    fn derived_detail_metric(&self, key: &str) -> Option<f64> {
        for &prefix in SC_DETAIL_PREFIXES {
            if !key.starts_with(prefix) {
                continue;
            }

            let suffix = key.strip_prefix(prefix)?.strip_prefix('_')?;
            let hits = self.number(&format!("{prefix}_cache_hits")).unwrap_or(0.0);
            let misses = self
                .number(&format!("{prefix}_cache_misses"))
                .unwrap_or(0.0);
            let reads = self.number(&format!("{prefix}_flash_reads")).unwrap_or(0.0);
            let writes = self
                .number(&format!("{prefix}_flash_writes"))
                .unwrap_or(0.0);
            let read_us = self
                .number(&format!("{prefix}_flash_reads_micros"))
                .unwrap_or(0.0);
            let write_us = self
                .number(&format!("{prefix}_flash_writes_micros"))
                .unwrap_or(0.0);
            let calls = self.number(&format!("{prefix}_calls")).unwrap_or(0.0);

            return match suffix {
                "cache_accesses" => Some(hits + misses),
                "cache_hit_rate" => div(hits, hits + misses),
                "cache_miss_rate" => div(misses, hits + misses),
                "flash_measured_ms" => Some((read_us + write_us) / 1000.0),
                "flash_estimated_ms" => Some(reads + 3.0 * writes),
                "reads_per_call" => div(reads, calls),
                "writes_per_call" => div(writes, calls),
                "measured_ms_per_call" => div((read_us + write_us) / 1000.0, calls),
                "estimated_ms_per_call" => div(reads + 3.0 * writes, calls),
                _ => None,
            };
        }

        None
    }
}

fn collection_metric_bytes(row: &BenchCsvRow, metric: &str) -> Option<f64> {
    if let Some(entry) = registry_entry(row.get("algo").unwrap_or(""))
        && let Some(collection) = entry["collections"].as_array().and_then(|items| {
            items.iter().find(|item| {
                item["key"].as_str() == Some(metric)
                    && item["state"]
                        .as_str()
                        .is_none_or(|state| state == row_bucket_state(row))
            })
        })
    {
        return Some(
            row.number(collection["count_key"].as_str()?)?
                * collection["entry_bytes"].as_u64()? as f64,
        );
    }
    let gnode = size_of::<GNode>();
    let u32_size = size_of::<u32>();
    let algo = normalize_algo(row.get("algo").unwrap_or(""));
    let state = row_bucket_state(row);
    let policy = row_option_or_default(row, "exploration_policy");
    let spatial_queue = algo == "astar_bucket" && policy == "spatial_tiebreak";
    let (count_key, entry_size) = match metric {
        "max_heap_bytes" => {
            let size = if algo == "astar_countdown" {
                match (state.as_str(), policy.as_str()) {
                    ("decoupled", "spatial_tiebreak") => size_of::<(((u32, u32), GNode), ())>(),
                    ("decoupled", _) => size_of::<(((u32, u32), u16), GNode)>(),
                    (_, "spatial_tiebreak") => {
                        size_of::<(((u32, u32, GNode), u16), (GNode, GNode))>()
                    }
                    _ => size_of::<(((u32, u32), u16), (GNode, GNode))>(),
                }
            } else {
                gnode + 3 * u32_size
            };
            ("max_heap", size)
        }
        "max_done_bytes" => {
            let size = if algo == "astar_countdown" && state == "decoupled" {
                size_of::<(GNode, u32)>()
            } else if algo == "astar_countdown" {
                size_of::<(GNode, u8)>()
            } else {
                size_of::<(GNode, ())>()
            };
            ("max_done", size)
        }
        "max_previous_bytes" => ("max_previous", size_of::<(GNode, GNode)>()),
        "max_global_bytes" => {
            let size = if spatial_queue {
                size_of::<(((u32, u32, u32), GNode), ())>()
            } else {
                size_of::<(((u32, u32, u32), u16), GNode)>()
            };
            ("max_global", size)
        }
        "max_local_bytes" => {
            let size = if spatial_queue {
                size_of::<(((u32, u32), GNode), ())>()
            } else {
                size_of::<(((u32, u32), u16), GNode)>()
            };
            ("max_local", size)
        }
        "max_states_bytes" => {
            let size = if algo == "astar_countdown" && state == "decoupled" {
                size_of::<(GNode, (u32, u32))>()
            } else if algo == "astar_bucket" && state == "decoupled" {
                size_of::<(GNode, u32)>()
            } else {
                size_of::<(GNode, (u32, GNode))>()
            };
            ("max_states", size)
        }
        _ => return None,
    };
    Some(row.number(count_key)? * entry_size as f64)
}

fn max_collection_bytes(row: &BenchCsvRow) -> Option<f64> {
    if let Some(items) = registry_entry(row.get("algo").unwrap_or(""))
        .and_then(|entry| entry["collections"].as_array())
        .filter(|items| !items.is_empty())
    {
        return items
            .iter()
            .filter_map(|item| collection_metric_bytes(row, item["key"].as_str()?))
            .reduce(f64::max);
    }
    let algo = normalize_algo(row.get("algo").unwrap_or(""));
    let state = row_bucket_state(row);
    let metrics: &[&str] = if algo == "astar_countdown" && state == "decoupled" {
        &[
            "max_heap_bytes",
            "max_done_bytes",
            "max_states_bytes",
            "max_previous_bytes",
        ]
    } else if algo == "astar_bucket" && state == "decoupled" {
        &[
            "max_global_bytes",
            "max_local_bytes",
            "max_states_bytes",
            "max_previous_bytes",
        ]
    } else if algo == "astar_bucket" {
        &["max_global_bytes", "max_local_bytes", "max_states_bytes"]
    } else {
        &["max_heap_bytes", "max_done_bytes", "max_previous_bytes"]
    };

    metrics
        .iter()
        .filter_map(|metric| collection_metric_bytes(row, metric))
        .reduce(f64::max)
}

fn resolve_query(query: BenchResultQuery) -> BenchResolvedQuery {
    let algo = normalize_algo(query.algo.as_deref().unwrap_or(DEFAULT_ALGO));
    let raw_policy = query
        .exploration_policy
        .as_deref()
        .unwrap_or_else(|| {
            registry_descriptor(&algo)
                .map(|d| d.policies[0])
                .unwrap_or(DEFAULT_EXPLORATION_POLICY)
        })
        .trim()
        .to_lowercase()
        .replace('-', "_");
    let requested_policy =
        if registry_descriptor(&algo).is_some_and(|d| d.policies.contains(&raw_policy.as_str())) {
            raw_policy
        } else {
            normalize_bucket_policy(Some(&raw_policy))
        };
    let exploration_policy = registry_descriptor(&algo)
        .map(|d| d.policy(Some(&requested_policy)).to_string())
        .unwrap_or_else(|| DEFAULT_EXPLORATION_POLICY.into());
    let bucket_state = normalized_bucket_state(&algo, query.bucket_state.as_deref());
    BenchResolvedQuery {
        swap_profile: query
            .swap_profile
            .unwrap_or_else(|| DEFAULT_SWAP_PROFILE.to_string()),
        case_id: query.case_id,
        algo,
        gps_layout: query
            .gps_layout
            .unwrap_or_else(|| DEFAULT_GPS_LAYOUT.to_string()),
        graph_storage: query
            .graph_storage
            .unwrap_or_else(|| DEFAULT_GRAPH_STORAGE.to_string()),
        neighbor_access: query
            .neighbor_access
            .unwrap_or_else(|| DEFAULT_NEIGHBOR_ACCESS.to_string()),
        exploration_policy,
        bucket_state,
        heuristic: query
            .heuristic
            .unwrap_or_else(|| DEFAULT_HEURISTIC.to_string()),
    }
}

fn comparison_query(selected: &BenchResolvedQuery, algo: &str) -> BenchResolvedQuery {
    let algo = normalize_algo(algo);
    let descriptor = registry_descriptor(&algo);
    BenchResolvedQuery {
        bucket_state: if descriptor.is_some_and(|d| d.states.len() > 1)
            && !selected.bucket_state.is_empty()
        {
            selected.bucket_state.clone()
        } else {
            normalized_bucket_state(&algo, Some(&selected.bucket_state))
        },
        exploration_policy: descriptor
            .map(|d| {
                if d.policies.len() > 1 {
                    selected.exploration_policy.clone()
                } else {
                    d.policies[0].to_string()
                }
            })
            .unwrap_or_else(|| DEFAULT_EXPLORATION_POLICY.into()),
        // Dijkstra n'a pas d'heuristique. Ses métadonnées de mesure utilisent "standard", quelle que soit la variante sélectionnée
        heuristic: if descriptor.is_some_and(|d| !d.uses_heuristic) {
            DEFAULT_HEURISTIC.to_string()
        } else {
            selected.heuristic.clone()
        },
        algo,
        ..selected.clone()
    }
}

fn legacy_strict_fallback_query(selected: &BenchResolvedQuery, algo: &str) -> BenchResolvedQuery {
    let algo = normalize_algo(algo);
    let descriptor = registry_descriptor(&algo);
    let mut query = comparison_query(selected, &algo);

    if descriptor.is_some_and(|d| {
        !d.states.is_empty() && !d.states.contains(&selected.bucket_state.as_str())
    }) {
        query.bucket_state = normalized_bucket_state(&algo, Some(DEFAULT_BUCKET_STATE));
    }

    if descriptor.is_some_and(|d| !d.policies.contains(&selected.exploration_policy.as_str())) {
        query.exploration_policy = descriptor
            .map(|d| d.policy(Some(DEFAULT_EXPLORATION_POLICY)).to_string())
            .unwrap_or_else(|| DEFAULT_EXPLORATION_POLICY.to_string());
    }

    query
}

fn algorithm_label(algo: &str) -> String {
    registry_descriptor(algo)
        .map(|descriptor| descriptor.label.to_string())
        .unwrap_or_else(|| algo.to_string())
}

fn row_option_or_default(row: &BenchCsvRow, key: &str) -> String {
    row.get(key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| option_default(key))
}

fn replay_measurement_identity(row: &BenchCsvRow) -> Vec<String> {
    [
        "run_id",
        "timestamp",
        "case_id",
        "algo",
        "file",
        "layout",
        "swap_profile",
        "graph_storage",
        "neighbor_access",
        "bucket_state",
        "exploration_policy",
        "heuristic",
    ]
    .iter()
    .map(|key| {
        if *key == "algo" {
            normalize_algo(row.get(key).unwrap_or(""))
        } else {
            row.get(key).unwrap_or("").to_string()
        }
    })
    .collect()
}

fn restore_replay_parameters(
    rows: &HashMap<String, Vec<BenchCsvRow>>,
    event_rows: &mut [BenchCsvRow],
) {
    // On essaie de compléter les métriques manquantes avec celles issues du CSV inspect du même run
    let mut inspect_parameters: HashMap<_, Option<&BenchCsvRow>> = HashMap::new();
    for source in rows.values().flatten().filter(|row| row.is_inspect()) {
        inspect_parameters
            .entry(replay_measurement_identity(source))
            .and_modify(|value| {
                if value.is_some_and(|prior| prior.values != source.values) {
                    *value = None;
                }
            })
            .or_insert(Some(source));
    }
    for event in event_rows {
        let source = inspect_parameters
            .get(&replay_measurement_identity(event))
            .copied()
            .flatten();
        if let Some(source) = source
            && let Some(entry) = registry_entry(event.get("algo").unwrap_or(""))
        {
            for metric in entry["specific_metrics"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|value| value.as_str())
            {
                if let Some(value) = source.get(metric)
                    && event.get(metric).is_none_or(|v| v.trim().is_empty())
                {
                    event.values.insert(metric.into(), value.into());
                }
            }
        }
    }
}

fn row_options_match_query(row: &BenchCsvRow, query: &BenchResolvedQuery) -> bool {
    row_option_or_default(row, "graph_storage") == query.graph_storage
        && row_option_or_default(row, "neighbor_access") == query.neighbor_access
        && row_option_or_default(row, "exploration_policy") == query.exploration_policy
        && row_bucket_state(row) == query.bucket_state
        && row_option_or_default(row, "heuristic") == query.heuristic
}

fn normalized_layout(value: &str) -> String {
    match value.trim().to_lowercase().as_str() {
        "zcurve" | "z_curve" => "z_curve".to_string(),
        "hilbert" => "hilbert".to_string(),
        _ => "base".to_string(),
    }
}

fn row_layout(row: &BenchCsvRow) -> String {
    if let Some(layout) = row.get("layout").filter(|value| !value.trim().is_empty()) {
        return normalized_layout(layout);
    }
    let file = file_basename(row.get("file").unwrap_or("")).to_lowercase();
    if file.contains("hilbert") {
        "hilbert".to_string()
    } else if file.contains("zcurve") || file.contains("z_curve") {
        "z_curve".to_string()
    } else {
        "base".to_string()
    }
}

fn row_layout_matches_query(row: &BenchCsvRow, query: &BenchResolvedQuery) -> bool {
    row_layout(row) == normalized_layout(&query.gps_layout)
}

fn event_dataset_id(query: &BenchResolvedQuery) -> String {
    let mut id = format!(
        "inspect_{}_{}_{}_{}_{}_{}_{}",
        normalized_layout(&query.gps_layout),
        query.swap_profile,
        query.graph_storage,
        query.neighbor_access,
        query.exploration_policy,
        query.heuristic,
        query.algo,
    );
    if registry_descriptor(&query.algo).is_some_and(|descriptor| !descriptor.states.is_empty()) {
        id.push_str(&format!("_{}", query.bucket_state));
    }
    id
}

fn event_dataset_label(query: &BenchResolvedQuery) -> String {
    let descriptor = registry_descriptor(&query.algo);
    let mut label = format!(
        "Inspect / {} / {} / {}",
        normalized_layout(&query.gps_layout),
        query.swap_profile,
        descriptor.map(|entry| entry.label).unwrap_or(&query.algo),
    );
    for (key, value, visible) in [
        (
            "states",
            &query.bucket_state,
            descriptor.is_some_and(|entry| !entry.states.is_empty()),
        ),
        (
            "policies",
            &query.exploration_policy,
            descriptor.is_some_and(|entry| entry.policies.len() > 1),
        ),
    ] {
        if visible {
            let option_label = registered_options(key)
                .into_iter()
                .find(|option| option.id == *value)
                .map(|option| option.label)
                .unwrap_or_else(|| value.clone());
            label.push_str(&format!(" / {option_label}"));
        }
    }
    label
}

fn row_file_matches_dataset(row: &BenchCsvRow, dataset: &BenchDataset) -> bool {
    let Some(row_file) = row.get("file") else {
        return true;
    };

    if dataset.gps_file.trim().is_empty() {
        return true;
    }

    let row_file = file_basename(row_file);
    let dataset_file = file_basename(&dataset.gps_file);
    row_file == dataset_file
}

fn file_basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn metric_source(metric: &str) -> MetricSource {
    // Sélection du run perf pour les durées de référence et du run inspect pour les autres métriques, sans mélanger les modes
    if matches!(
        metric,
        "decode_us"
            | "graph_us"
            | "search_us"
            | "decode_ms"
            | "graph_ms"
            | "search_ms"
            | "total_ms"
            | "measured_search_ms"
    ) {
        MetricSource::Perf
    } else {
        MetricSource::Inspect
    }
}

fn validation(rows: &[&BenchCsvRow]) -> BenchValidation {
    let (found, found_stable) = stable_field(rows, "found");
    let (cost, cost_stable) = stable_field(rows, "cost");
    let (path_len, path_len_stable) = stable_field(rows, "path_len");

    BenchValidation {
        found,
        found_stable,
        cost,
        cost_stable,
        path_len,
        path_len_stable,
    }
}

fn stable_field(rows: &[&BenchCsvRow], key: &str) -> (Option<String>, bool) {
    let mut values = rows
        .iter()
        .filter_map(|row| row.get(key))
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned);

    let Some(first) = values.next() else {
        return (None, true);
    };

    let stable = values.all(|value| value == first);
    (Some(first), stable)
}

fn compute_delta(selected: &BenchAggregate, baseline: &BenchAggregate) -> BTreeMap<String, f64> {
    let mut delta = BTreeMap::new();

    insert_ratio(
        &mut delta,
        "speedup_search",
        baseline,
        selected,
        "search_ms",
    );
    insert_reduction(
        &mut delta,
        "search_flash_read_reduction",
        baseline,
        selected,
        "search_flash_reads",
    );
    insert_reduction(
        &mut delta,
        "search_flash_write_reduction",
        baseline,
        selected,
        "search_flash_writes",
    );
    insert_reduction(
        &mut delta,
        "reads_per_visited_node_reduction",
        baseline,
        selected,
        "reads_per_visited_node",
    );
    insert_reduction(
        &mut delta,
        "neighbors_next_cross_reduction",
        baseline,
        selected,
        "neighbors_next_cross_tile_rate",
    );
    delta
}

fn insert_ratio(
    out: &mut BTreeMap<String, f64>,
    name: &str,
    baseline: &BenchAggregate,
    selected: &BenchAggregate,
    metric: &str,
) {
    let Some(base) = baseline.metrics.get(metric).copied() else {
        return;
    };
    let Some(value) = selected.metrics.get(metric).copied() else {
        return;
    };
    if value != 0.0 {
        out.insert(name.to_string(), base / value);
    }
}

fn insert_reduction(
    out: &mut BTreeMap<String, f64>,
    name: &str,
    baseline: &BenchAggregate,
    selected: &BenchAggregate,
    metric: &str,
) {
    let Some(base) = baseline.metrics.get(metric).copied() else {
        return;
    };
    let Some(value) = selected.metrics.get(metric).copied() else {
        return;
    };
    if base != 0.0 {
        out.insert(name.to_string(), 1.0 - value / base);
    }
}

fn div(a: f64, b: f64) -> Option<f64> {
    if b == 0.0 { None } else { Some(a / b) }
}

fn scenarios_from_sources(
    scenarios_path: &Path,
    rows_by_dataset: &HashMap<String, Vec<BenchCsvRow>>,
    event_rows: &[BenchCsvRow],
) -> Result<Vec<BenchScenario>> {
    let mut scenarios = BTreeMap::<String, BenchScenario>::new();

    if scenarios_path.exists() {
        for row in read_simple_csv(scenarios_path)? {
            let name = required(&row, "name")?.to_string();
            let parse_coord = |key: &str| -> Result<f64> {
                required(&row, key)?.parse::<f64>().map_err(|_| {
                    Error::new(
                        ErrorKind::InvalidData,
                        format!(
                            "coordonnée invalide {key} dans {}",
                            scenarios_path.display()
                        ),
                    )
                })
            };

            scenarios.insert(
                name.clone(),
                BenchScenario {
                    name,
                    file: String::new(),
                    start_lon: parse_coord("start_lon")?,
                    start_lat: parse_coord("start_lat")?,
                    end_lon: parse_coord("end_lon")?,
                    end_lat: parse_coord("end_lat")?,
                },
            );
        }
    }

    for rows in rows_by_dataset.values() {
        for row in rows {
            let Some(case_id) = row.get("case_id") else {
                continue;
            };

            if case_id.trim().is_empty() {
                continue;
            }

            scenarios
                .entry(case_id.to_string())
                .or_insert_with(|| BenchScenario {
                    name: case_id.to_string(),
                    file: row.get("file").unwrap_or("").to_string(),
                    start_lon: row.number("start_lon").unwrap_or(0.0),
                    start_lat: row.number("start_lat").unwrap_or(0.0),
                    end_lon: row.number("end_lon").unwrap_or(0.0),
                    end_lat: row.number("end_lat").unwrap_or(0.0),
                });
        }
    }

    for row in event_rows {
        let Some(case_id) = row.get("case_id") else {
            continue;
        };
        if case_id.trim().is_empty() {
            continue;
        }
        scenarios
            .entry(case_id.to_string())
            .or_insert_with(|| BenchScenario {
                name: case_id.to_string(),
                file: row.get("file").unwrap_or("").to_string(),
                start_lon: row.number("start_lon").unwrap_or(0.0),
                start_lat: row.number("start_lat").unwrap_or(0.0),
                end_lon: row.number("end_lon").unwrap_or(0.0),
                end_lat: row.number("end_lat").unwrap_or(0.0),
            });
    }

    Ok(scenarios.into_values().collect())
}

fn load_catalog(path: &Path) -> Result<Vec<BenchDataset>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let rows = read_simple_csv(path)?;
    let mut datasets = Vec::new();

    for row in rows {
        let mut options = BTreeMap::new();
        for key in DATASET_OPTION_KEYS {
            options.insert(
                key.to_string(),
                row.get(key).cloned().unwrap_or_else(|| option_default(key)),
            );
        }

        datasets.push(BenchDataset {
            id: required(&row, "dataset")?.to_string(),
            label: row
                .get("label")
                .cloned()
                .unwrap_or_else(|| required(&row, "dataset").unwrap_or("dataset").to_string()),
            csv: required(&row, "csv")?.to_string(),
            gps_file: row.get("gps_file").cloned().unwrap_or_default(),
            row_count: 0,
            options,
        });
    }

    Ok(datasets)
}

fn load_enriched_rows(path: &Path) -> Result<Vec<BenchCsvRow>> {
    let rows = read_simple_csv(path)?;
    Ok(rows
        .into_iter()
        .filter(|values| {
            registry_descriptor(values.get("algo").map_or("", String::as_str)).is_some()
        })
        .map(|values| BenchCsvRow { values })
        .collect())
}

fn read_simple_csv(path: &Path) -> Result<Vec<HashMap<String, String>>> {
    let content = fs::read_to_string(path)?;
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());

    let Some(header_line) = lines.next() else {
        return Ok(Vec::new());
    };

    let headers: Vec<String> = split_csv_line(header_line)
        .into_iter()
        .map(|s| s.trim().to_string())
        .collect();

    let mut rows = Vec::new();

    for line in lines {
        let cells = split_csv_line(line);
        let mut row = HashMap::new();

        for (index, header) in headers.iter().enumerate() {
            row.insert(
                header.clone(),
                cells.get(index).cloned().unwrap_or_default(),
            );
        }

        rows.push(row);
    }

    Ok(rows)
}

fn split_csv_line(line: &str) -> Vec<String> {
    line.split(',')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn required<'a>(row: &'a HashMap<String, String>, key: &str) -> Result<&'a str> {
    row.get(key)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, format!("champ CSV manquant: {key}")))
}

fn registry_descriptor(value: &str) -> Option<&'static AlgorithmDescriptor> {
    let value = value.trim().to_lowercase();
    Algorithm::ALL
        .iter()
        .map(|a| a.descriptor())
        .find(|d| d.id == value || d.command == value)
}

fn registry_entries() -> &'static Vec<serde_json::Value> {
    static ENTRIES: std::sync::OnceLock<Vec<serde_json::Value>> = std::sync::OnceLock::new();
    ENTRIES.get_or_init(|| serde_json::from_str::<serde_json::Value>(CATALOG_JSON).expect("registre généré")["algorithms"].as_array().unwrap().clone())
}

fn registry_entry(value: &str) -> Option<&'static serde_json::Value> {
    let id = normalize_algo(value);
    registry_entries()
        .iter()
        .find(|entry| entry["id"].as_str() == Some(&id))
}

fn normalize_algo(value: &str) -> String {
    registry_descriptor(value)
        .map(|d| d.id.to_string())
        .unwrap_or_else(|| value.trim().to_lowercase().to_string())
}

fn normalize_bucket_policy(value: Option<&str>) -> String {
    let value = value
        .unwrap_or(DEFAULT_EXPLORATION_POLICY)
        .trim()
        .to_lowercase()
        .replace('-', "_");
    if value == "spatial" {
        "spatial_tiebreak".into()
    } else {
        value
    }
}

fn normalized_bucket_state(algo: &str, value: Option<&str>) -> String {
    registry_descriptor(algo)
        .map(|d| d.state(value).to_string())
        .unwrap_or_default()
}

fn row_bucket_state(row: &BenchCsvRow) -> String {
    // Conserver les états actuels ou historiques déclarés dans le registre (sinon on prend l'état par défaut)
    if let Some(state) = row
        .get("bucket_state")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        && (registry_descriptor(row.get("algo").unwrap_or(""))
            .is_some_and(|descriptor| descriptor.states.contains(&state))
            || registry_entry(row.get("algo").unwrap_or(""))
                .and_then(|entry| entry["historical_states"].as_array())
                .is_some_and(|states| states.iter().any(|value| value.as_str() == Some(state))))
    {
        return state.to_string();
    }
    normalized_bucket_state(
        &normalize_algo(row.get("algo").unwrap_or("")),
        row.get("bucket_state"),
    )
}

fn option_value(dataset: &BenchDataset, key: &str) -> String {
    dataset
        .options
        .get(key)
        .cloned()
        .unwrap_or_else(|| option_default(key))
}

fn option_default(key: &str) -> String {
    match key {
        "gps_layout" => DEFAULT_GPS_LAYOUT,
        "graph_storage" => DEFAULT_GRAPH_STORAGE,
        "neighbor_access" => DEFAULT_NEIGHBOR_ACCESS,
        "exploration_policy" => DEFAULT_EXPLORATION_POLICY,
        "bucket_state" => "",
        "heuristic" => DEFAULT_HEURISTIC,
        "swap_profile" => DEFAULT_SWAP_PROFILE,
        _ => "",
    }
    .to_string()
}

fn default_selection() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("algo".to_string(), DEFAULT_ALGO.to_string()),
        ("baseline_algo".to_string(), DEFAULT_ALGO.to_string()),
        ("gps_layout".to_string(), DEFAULT_GPS_LAYOUT.to_string()),
        (
            "graph_storage".to_string(),
            DEFAULT_GRAPH_STORAGE.to_string(),
        ),
        (
            "neighbor_access".to_string(),
            DEFAULT_NEIGHBOR_ACCESS.to_string(),
        ),
        (
            "exploration_policy".to_string(),
            DEFAULT_EXPLORATION_POLICY.to_string(),
        ),
        ("bucket_state".to_string(), DEFAULT_BUCKET_STATE.to_string()),
        ("heuristic".to_string(), DEFAULT_HEURISTIC.to_string()),
        ("swap_profile".to_string(), DEFAULT_SWAP_PROFILE.to_string()),
    ])
}

fn comparison_algorithms() -> Vec<BenchOption> {
    let mut algorithms: Vec<_> = Algorithm::ALL
        .iter()
        .map(|algo| algo.descriptor())
        .filter(|d| d.enabled)
        .map(|d| option(d.id, d.label))
        .collect();
    algorithms.sort_by_key(|algo| algo.id != "dijkstra_lazy");
    algorithms
}

fn registered_options(key: &str) -> Vec<BenchOption> {
    let mut ids: std::collections::BTreeSet<&str> = Algorithm::ALL
        .iter()
        .map(|a| a.descriptor())
        .filter(|d| d.enabled)
        .flat_map(|d| {
            if key == "states" {
                d.states.iter().copied()
            } else {
                d.policies.iter().copied()
            }
        })
        .collect();
    let catalog: serde_json::Value = serde_json::from_str(CATALOG_JSON).expect("registre généré");
    let mut options = Vec::new();
    if let Some(labels) = catalog["option_labels"][key].as_array() {
        for label in labels {
            let id = label["id"].as_str().unwrap();
            if ids.remove(id) {
                options.push(option(id, label["label"].as_str().unwrap()));
            }
        }
    }
    options.extend(ids.into_iter().map(|id| option(id, id)));
    options
}

fn option_groups() -> Vec<BenchOptionGroup> {
    vec![
        BenchOptionGroup {
            id: "algo".to_string(),
            label: "Structure algorithmique".to_string(),
            mode: "single".to_string(),
            default: DEFAULT_ALGO.to_string(),
            options: comparison_algorithms(),
        },
        BenchOptionGroup {
            id: "gps_layout".to_string(),
            label: "Encodage des tuiles".to_string(),
            mode: "single".to_string(),
            default: DEFAULT_GPS_LAYOUT.to_string(),
            options: vec![
                option("base", "Simple"),
                option("hilbert", "Hilbert"),
                option("z_curve", "Z-Curve"),
            ],
        },
        BenchOptionGroup {
            id: "swap_profile".to_string(),
            label: "Architecture cible".to_string(),
            mode: "single".to_string(),
            default: DEFAULT_SWAP_PROFILE.to_string(),
            options: vec![
                option("256_64", "RAM 64 Ko (blocksize : 256)"),
                option("256_128", "RAM 128 Ko (blocksize : 256)"),
                option("256_256", "RAM 256 Ko (blocksize : 256)"),
            ],
        },
        BenchOptionGroup {
            id: "graph_storage".to_string(),
            label: "Abstraction du graphe".to_string(),
            mode: "single".to_string(),
            default: DEFAULT_GRAPH_STORAGE.to_string(),
            options: vec![
                option("tile_dicho", "Minimale"),
                option("tile_index", "Tuiles indexées"),
            ],
        },
        BenchOptionGroup {
            id: "neighbor_access".to_string(),
            label: "Abstraction des voisins (Hot Path)".to_string(),
            mode: "single".to_string(),
            default: DEFAULT_NEIGHBOR_ACCESS.to_string(),
            options: vec![option("tile_segments", "Minimale")],
        },
        BenchOptionGroup {
            id: "bucket_state".to_string(),
            label: "Structuration des collections".to_string(),
            mode: "single".to_string(),
            default: DEFAULT_BUCKET_STATE.to_string(),
            options: registered_options("states"),
        },
        BenchOptionGroup {
            id: "exploration_policy".to_string(),
            label: "Politique d'exploration".to_string(),
            mode: "single".to_string(),
            default: DEFAULT_EXPLORATION_POLICY.to_string(),
            options: registered_options("policies"),
        },
        BenchOptionGroup {
            id: "heuristic".to_string(),
            label: "Heuristique".to_string(),
            mode: "single".to_string(),
            default: DEFAULT_HEURISTIC.to_string(),
            options: vec![
                option("standard", "Équilibrée"),
                option("zero", "Faible/Nulle"),
                option("weighted", "Agressive"),
            ],
        },
    ]
}

fn option(id: &str, label: &str) -> BenchOption {
    BenchOption {
        id: id.to_string(),
        label: label.to_string(),
    }
}
