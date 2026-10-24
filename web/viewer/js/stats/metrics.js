// Lecture, classement et noms des métriques.

import { PHASE_PREFIXES, state } from "../state.js";
import { usesDecoupledState } from "../controls.js";

function metricOf(aggregate, key) {
  if (!aggregate || !aggregate.metrics) return null;

  if (key === "search_cache_accesses") {
    const hits = metricOf(aggregate, "search_cache_hits");
    const misses = metricOf(aggregate, "search_cache_misses");

    if (hits == null || misses == null) return null;

    return hits + misses;
  }

  if (key === "search_cache_misses_per_million") {
    const accesses = metricOf(aggregate, "search_cache_accesses");
    const misses = metricOf(aggregate, "search_cache_misses");

    if (!accesses || misses == null) return null;

    return (misses / accesses) * 1000000;
  }

  const value = aggregate.metrics[key];

  if (value == null || value === "") return null;

  const n = Number(value);

  return Number.isFinite(n) ? n : null;
}

function allMetricKeys(aggregate) {
  if (!aggregate || !aggregate.metrics) return [];
  return Object.keys(aggregate.metrics).sort((a, b) => a.localeCompare(b));
}

function classifyMetricKey(key) {
  if (isPhaseMetric(key)) return "hotpath";
  if (isLocalityMetric(key)) return "locality";
  if (isTimeMetric(key)) return "time";
  if (isMemoryMetric(key)) return "memory";
  if (isWorkMetric(key)) return "work";
  return "result";
}

function isTimeMetric(key) {
  return key.endsWith("_ms") || key.endsWith("_us") || key.includes("residual_after");
}

function isPhaseMetric(key) {
  return PHASE_PREFIXES.some(([prefix]) => key.startsWith(prefix + "_"));
}

function isMemoryMetric(key) {
  return key.includes("cache") || key.includes("flash") || key.includes("reads") || key.includes("writes") || key.includes("hit_rate") || key.includes("miss_rate") || key.includes("misses_per_million");
}

function isLocalityMetric(key) {
  return key.includes("tile") || key.includes("jump") || key.includes("cross") || key.includes("same_node") || key.includes("border") || key.startsWith("edges_from_");
}

function isWorkMetric(key) {
  return [
    "heap_pushes", "heap_pops", "nodes_skipped", "nodes_visited", "neighbors_skipped", "neighbors_visited", "edges_visited",
    "heuristic_calls", "max_heap", "max_done", "max_previous", "max_collection_bytes",
    "countdown_done_checks", "countdown_done_inserts", "countdown_done_decs", "countdown_done_removes", "countdown_scans", "countdown_edges",
    "global_pushes", "global_pops", "local_pushes", "local_pops", "max_global", "max_local", "max_states",
    "stale_pop_ratio", "pop_skipped_ratio", "neighbors_skipped_ratio", "neighbors_visited_ratio", "visited_per_path_node",
    "heuristic_per_visited_node", "graph_edge_work", "algorithmic_work", "algorithmic_work_graph", "algorithmic_work_frontier", "algorithmic_work_state", "algorithmic_work_heuristic", "queue_operations", "max_heap_bytes", "max_done_bytes", "max_previous_bytes", "max_global_bytes", "max_local_bytes", "max_states_bytes",
  ].includes(key);
}

function themeMetricKeys(themeId, aggregate) {
  const keys = allMetricKeys(aggregate);
  if (themeId === "memory") {
    return keys.filter((key) => key.startsWith("search_cache_") || key.startsWith("search_flash_"));
  }
  return keys.filter((key) => classifyMetricKey(key) === themeId);
}

function metricSourceLabel(key) {
  if (["search_ms", "decode_ms", "graph_ms", "total_ms", "measured_search_ms"].includes(key)) return "perf";
  if (key.endsWith("_flash_estimated_ms") || key.endsWith("_estimated_ms") || key.includes("estimated")) return "instrumenté + projection";
  if (key.startsWith("algorithmic_work") || key.includes("residual") || key.includes("_rate") || key.includes("_ratio") || key.startsWith("avg_") || key.includes("per_")) return "calculé";
  return "instrumenté";
}

function metricDisplayKey(key) {
  const declared = (state.benchConfig?.algorithms || []).find((entry) => entry.metric_labels?.[key])?.metric_labels?.[key];
  if (declared) return declared;

  if (!usesDecoupledState()) return key;
  if (key === "max_states") return "max_costs";
  if (key.startsWith("states_")) return "costs_" + key.slice("states_".length);
  return key;
}

function phaseLabel(prefix, fallback) {
  if (usesDecoupledState() && prefix.startsWith("states_")) {
    return fallback.replace("State (Cost + Previous)", "Cost");
  }
  return fallback;
}

export { metricDisplayKey, metricOf, metricSourceLabel, phaseLabel, themeMetricKeys };
