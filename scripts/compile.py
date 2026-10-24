import csv
import os
import shutil
from collections import defaultdict
from pathlib import Path
from publish import bucket_tile_side, bucket_state

OUTPUT_DIR = Path(os.environ.get("BENCH_OUTPUT_DIR", "output"))
src = OUTPUT_DIR / "all_runs.csv"
dst = OUTPUT_DIR / "all_runs_enriched.csv"
summary_by_run_dst = OUTPUT_DIR / "summary_by_run.csv"
summary_by_case_dst = OUTPUT_DIR / "summary_by_case.csv"
summary_dst = OUTPUT_DIR / "summary_by_algo.csv"

FLASH_READ_LATENCY_MS = float(os.environ.get("BENCH_FLASH_READ_LATENCY_MS", "1"))
FLASH_WRITE_LATENCY_MS = float(os.environ.get("BENCH_FLASH_WRITE_LATENCY_MS", "3"))
DEFAULT_SWAP_PROFILE = os.environ.get("BENCH_SWAP_PROFILE", "")
DEFAULT_SWAP_BLOCK_BYTES = os.environ.get("BENCH_SWAP_BLOCK_BYTES", "")
DEFAULT_SWAP_CACHE_KO = os.environ.get("BENCH_SWAP_CACHE_KO", "")
DEFAULT_SWAP_CACHE_BLOCKS = os.environ.get("BENCH_SWAP_CACHE_BLOCKS", "")
DEFAULT_GRAPH_STORAGE = os.environ.get("BENCH_GRAPH_STORAGE", "tile_dicho")
DEFAULT_NEIGHBOR_ACCESS = os.environ.get("BENCH_NEIGHBOR_ACCESS", "tile_segments")
DEFAULT_EXPLORATION_POLICY = os.environ.get("BENCH_EXPLORATION_POLICY", "strict")
DEFAULT_HEURISTIC = os.environ.get("BENCH_HEURISTIC", "standard")

SC_FIELDS = [
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
]

SC_DETAIL_PREFIXES = [
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
]

ALGORITHM_WORK_GRAPH_PREFIXES = ["neighbors_open", "neighbors_next"]
ALGORITHM_WORK_FRONTIER_PREFIXES = [
    "heap_push",
    "heap_pop",
    "global_push",
    "global_pop",
    "local_push",
    "local_pop",
]
ALGORITHM_WORK_STATE_PREFIXES = [
    "done_get",
    "done_insert",
    "done_remove",
    "previous_get",
    "previous_insert",
    "states_get",
    "states_insert",
    "states_remove",
]

SEARCH_COUNTER_PREFIXES = ["search", "search_window", "search_stats", "search_total"]

SUMMARY_METRICS = [
    "search_ms",
    "search_flash_measured_ms",
    "search_flash_estimated_ms",
    "nodes_visited",
    "neighbors_visited",
    "heap_pushes",
    "heap_pops",
    "countdown_edges",
    "countdown_done_decs",
    "countdown_done_removes",
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
    "stale_pop_ratio",
    "pop_skipped_ratio",
    "neighbors_skipped_ratio",
    "heap_pop_reads_per_call",
    "heap_push_reads_per_call",
    "done_get_reads_per_call",
    "done_insert_reads_per_call",
    "previous_get_reads_per_call",
    "previous_insert_reads_per_call",
    "neighbors_open_reads_per_call",
    "neighbors_next_reads_per_call",
]


def safe_int(value, default=0):
    if value is None or value == "":
        return default
    try:
        return int(float(value))
    except ValueError:
        return default


def safe_float(value, default=0.0):
    if value is None or value == "":
        return default
    try:
        return float(value)
    except ValueError:
        return default


def safe_float_or_none(value):
    if value is None or value == "":
        return None
    try:
        value = float(value)
    except ValueError:
        return None
    return value


def safe_div(a, b):
    return "" if b == 0 else a / b


def sum_detail_calls(row, prefixes):
    return sum(safe_int(row.get(f"{prefix}_calls")) for prefix in prefixes)


def mean(values):
    values = [v for v in values if v is not None]
    return "" if not values else sum(values) / len(values)


def basename(path):
    return str(path or "").split("/")[-1]


def infer_layout(file):
    name = basename(file).lower()
    if "hilbert" in name:
        return "hilbert"
    if "zcurve" in name or "z_curve" in name:
        return "z_curve"
    return "base"


def first_counter(row, field):
    # search_window est l'instantané du worker pris aux mêmes limites d'appel
    # de l'algorithme que search_us en perf. Les anciens fichiers bruts utilisent leurs groupes
    # historiques search_total/search_stats sans réinterpréter search_stats.
    for prefix in ["search_window", "search_total", "search_stats", "search"]:
        key = f"{prefix}_{field}"
        if key in row and row.get(key, "") != "":
            return row.get(key)
    return None


def ensure_swap_profile(row):
    if row.get("swap_profile"):
        return

    block = row.get("swap_block_bytes") or DEFAULT_SWAP_BLOCK_BYTES
    ko = row.get("swap_cache_ko") or DEFAULT_SWAP_CACHE_KO
    blocks = row.get("swap_cache_blocks") or DEFAULT_SWAP_CACHE_BLOCKS

    if not row.get("swap_block_bytes") and block:
        row["swap_block_bytes"] = block
    if not row.get("swap_cache_ko") and ko:
        row["swap_cache_ko"] = ko
    if not row.get("swap_cache_blocks") and blocks:
        row["swap_cache_blocks"] = blocks

    if DEFAULT_SWAP_PROFILE:
        row["swap_profile"] = DEFAULT_SWAP_PROFILE
    elif block and ko:
        row["swap_profile"] = f"{safe_int(block)}_{safe_int(ko)}"
    else:
        row["swap_profile"] = "unknown"


def add_ratio_values(row, key, num, den):
    row[key] = safe_div(num, den)


def ensure_bench_dimensions(row):
    if not row.get("graph_storage"):
        row["graph_storage"] = DEFAULT_GRAPH_STORAGE
    if not row.get("neighbor_access"):
        row["neighbor_access"] = DEFAULT_NEIGHBOR_ACCESS
    if not row.get("exploration_policy"):
        row["exploration_policy"] = DEFAULT_EXPLORATION_POLICY
    row["bucket_state"] = bucket_state(row)
    if not row.get("heuristic"):
        row["heuristic"] = DEFAULT_HEURISTIC
    row["bucket_tile_side"] = bucket_tile_side(row)


rows = list(csv.DictReader(src.open(encoding="utf-8")))
assert rows, "CSV vide"

for r in rows:
    r["mode"] = r.get("mode") or r.get("profile") or "perf"
    r["profile"] = r.get("profile") or r["mode"]
    ensure_swap_profile(r)
    ensure_bench_dimensions(r)
    r["layout"] = r.get("layout") or infer_layout(r.get("file", ""))
    r["scenario_key"] = f"{basename(r.get('file', ''))}:{r.get('case_id', '')}"

    search_us = safe_int(r.get("search_us"))
    decode_us = safe_int(r.get("decode_us"))
    graph_us = safe_int(r.get("graph_us"))
    rebuild_us = safe_int(r.get("rebuild_us"))

    heap_pushes = safe_int(r.get("heap_pushes") or r.get("pushed"))
    heap_pops = safe_int(r.get("heap_pops") or r.get("popped"))
    nodes_skipped = safe_int(r.get("nodes_skipped") or r.get("skipped"))
    nodes_visited = safe_int(r.get("nodes_visited") or r.get("settled"))
    neighbors_skipped = safe_int(r.get("neighbors_skipped") or r.get("ignored"))
    neighbors_visited = safe_int(r.get("neighbors_visited") or r.get("visited"))
    edges_visited = safe_int(r.get("edges_visited") or (neighbors_skipped + neighbors_visited))
    path_len = safe_int(r.get("path_len"))
    max_heap = safe_int(r.get("max_heap") or r.get("max_open_set_len"))

    # Normalisation de noms utiles pour compatibilité front.
    r["heap_pushes"] = heap_pushes
    r["heap_pops"] = heap_pops
    r["nodes_skipped"] = nodes_skipped
    r["nodes_visited"] = nodes_visited
    r["neighbors_skipped"] = neighbors_skipped
    r["neighbors_visited"] = neighbors_visited
    r["edges_visited"] = edges_visited
    r["max_heap"] = max_heap

    r["decode_ms"] = decode_us / 1000
    r["graph_ms"] = graph_us / 1000
    r["search_ms"] = search_us / 1000
    r["rebuild_ms"] = rebuild_us / 1000
    r["total_ms"] = (decode_us + graph_us + search_us + rebuild_us) / 1000
    r["measured_search_ms"] = r["search_ms"]

    for field in SC_FIELDS:
        value = first_counter(r, field)
        if value is not None:
            r[f"search_{field}"] = value

    hits = safe_float(r.get("search_cache_hits"))
    misses = safe_float(r.get("search_cache_misses"))
    reads = safe_float(r.get("search_flash_reads"))
    writes = safe_float(r.get("search_flash_writes"))
    read_us = safe_float(r.get("search_flash_reads_micros"))
    write_us = safe_float(r.get("search_flash_writes_micros"))

    r["search_cache_accesses"] = hits + misses
    r["search_cache_hit_rate"] = safe_div(hits, hits + misses)
    r["search_cache_miss_rate"] = safe_div(misses, hits + misses)
    r["search_cache_misses_per_million"] = safe_div(misses * 1_000_000, hits + misses)

    # Temps flash : measured = compteurs µs de swap-cache ; estimated = modèle reads/writes.
    r["search_flash_measured_ms"] = (read_us + write_us) / 1000
    r["search_flash_estimated_ms"] = reads * FLASH_READ_LATENCY_MS + writes * FLASH_WRITE_LATENCY_MS
    r["search_residual_after_measured_flash_ms"] = safe_float(r["search_ms"]) - safe_float(r["search_flash_measured_ms"])
    r["search_residual_after_estimated_flash_ms"] = safe_float(r["search_ms"]) - safe_float(r["search_flash_estimated_ms"])

    r["stale_pop_ratio"] = safe_div(nodes_skipped, heap_pops)
    r["pop_skipped_ratio"] = safe_div(nodes_skipped, heap_pops)
    r["neighbors_skipped_ratio"] = safe_div(neighbors_skipped, edges_visited)
    r["neighbors_visited_ratio"] = safe_div(neighbors_visited, edges_visited)
    r["reads_per_visited_node"] = safe_div(reads, nodes_visited)
    r["writes_per_visited_node"] = safe_div(writes, nodes_visited)
    r["visited_per_path_node"] = safe_div(nodes_visited, path_len)
    r["heuristic_per_visited_node"] = safe_div(safe_float(r.get("heuristic_calls")), nodes_visited)
    r["graph_edge_work"] = edges_visited + safe_int(r.get("countdown_edges"))

    # Compteur synthétique d'appels instrumentés, regroupés par famille.
    # Ce total décrit un volume d'activité ; il ne constitue pas un coût CPU pondéré.
    r["algorithmic_work_graph"] = sum_detail_calls(r, ALGORITHM_WORK_GRAPH_PREFIXES)
    r["algorithmic_work_frontier"] = sum_detail_calls(r, ALGORITHM_WORK_FRONTIER_PREFIXES)
    r["algorithmic_work_state"] = sum_detail_calls(r, ALGORITHM_WORK_STATE_PREFIXES)
    r["algorithmic_work_heuristic"] = safe_int(r.get("heuristic_calls"))
    r["algorithmic_work"] = (
        r["algorithmic_work_graph"]
        + r["algorithmic_work_frontier"]
        + r["algorithmic_work_state"]
        + r["algorithmic_work_heuristic"]
    )

    r["queue_operations"] = heap_pushes + heap_pops + safe_int(r.get("global_pops"))
    popped_transitions = safe_int(r.get("popped_same_tile")) + safe_int(r.get("popped_cross_tile"))
    skipped_transitions = safe_int(r.get("skipped_same_tile")) + safe_int(r.get("skipped_cross_tile"))
    visited_transitions = safe_int(r.get("visited_same_tile")) + safe_int(r.get("visited_cross_tile"))

    add_ratio_values(r, "popped_cross_tile_rate", safe_float(r.get("popped_cross_tile")), popped_transitions)
    add_ratio_values(r, "skipped_cross_tile_rate", safe_float(r.get("skipped_cross_tile")), skipped_transitions)
    add_ratio_values(r, "visited_cross_tile_rate", safe_float(r.get("visited_cross_tile")), visited_transitions)
    add_ratio_values(r, "avg_popped_tile_jump", safe_float(r.get("popped_tile_jump_sum")), safe_float(r.get("popped_cross_tile")))
    add_ratio_values(r, "avg_skipped_tile_jump", safe_float(r.get("skipped_tile_jump_sum")), safe_float(r.get("skipped_cross_tile")))
    add_ratio_values(r, "avg_visited_tile_jump", safe_float(r.get("visited_tile_jump_sum")), safe_float(r.get("visited_cross_tile")))

    edge_interior = safe_int(r.get("edges_from_same_node"))
    edge_left = safe_int(r.get("edges_from_left_node"))
    edge_bottom = safe_int(r.get("edges_from_bottom_node"))
    edge_corner = safe_int(r.get("edges_from_bottom_left_node"))
    edge_border = edge_left + edge_bottom + edge_corner
    edge_total_by_origin = edge_interior + edge_border
    r["edges_from_border_node"] = edge_border
    r["edges_from_interior_node_rate"] = safe_div(edge_interior, edge_total_by_origin)
    r["edges_from_left_node_rate"] = safe_div(edge_left, edge_total_by_origin)
    r["edges_from_bottom_node_rate"] = safe_div(edge_bottom, edge_total_by_origin)
    r["edges_from_bottom_left_node_rate"] = safe_div(edge_corner, edge_total_by_origin)
    r["edges_from_border_node_rate"] = safe_div(edge_border, edge_total_by_origin)

    heap_push_same_calls = safe_float(r.get("heap_push_same_tile_calls"))
    heap_push_cross_calls = safe_float(r.get("heap_push_cross_tile_calls"))
    r["heap_push_cross_tile_rate"] = safe_div(heap_push_cross_calls, heap_push_same_calls + heap_push_cross_calls)

    next_same_calls = safe_float(r.get("neighbors_next_same_tile_calls"))
    next_cross_calls = safe_float(r.get("neighbors_next_cross_tile_calls"))
    r["neighbors_next_cross_tile_rate"] = safe_div(next_cross_calls, next_same_calls + next_cross_calls)

    open_same_calls = safe_float(r.get("neighbors_same_tile_calls"))
    open_left_calls = safe_float(r.get("neighbors_cross_left_calls"))
    open_bottom_calls = safe_float(r.get("neighbors_cross_bottom_calls"))
    open_corner_calls = safe_float(r.get("neighbors_cross_bottom_left_calls"))
    open_total = open_same_calls + open_left_calls + open_bottom_calls + open_corner_calls
    r["neighbors_open_cross_rate"] = safe_div(open_left_calls + open_bottom_calls + open_corner_calls, open_total)

    for prefix in SC_DETAIL_PREFIXES + SEARCH_COUNTER_PREFIXES:
        prefix_hits = safe_float(r.get(f"{prefix}_cache_hits"))
        prefix_misses = safe_float(r.get(f"{prefix}_cache_misses"))
        prefix_reads = safe_float(r.get(f"{prefix}_flash_reads"))
        prefix_writes = safe_float(r.get(f"{prefix}_flash_writes"))
        prefix_read_us = safe_float(r.get(f"{prefix}_flash_reads_micros"))
        prefix_write_us = safe_float(r.get(f"{prefix}_flash_writes_micros"))
        prefix_calls = safe_float(r.get(f"{prefix}_calls"))

        r[f"{prefix}_cache_accesses"] = prefix_hits + prefix_misses
        r[f"{prefix}_cache_hit_rate"] = safe_div(prefix_hits, prefix_hits + prefix_misses)
        r[f"{prefix}_cache_miss_rate"] = safe_div(prefix_misses, prefix_hits + prefix_misses)
        r[f"{prefix}_flash_measured_ms"] = (prefix_read_us + prefix_write_us) / 1000
        r[f"{prefix}_flash_estimated_ms"] = prefix_reads * FLASH_READ_LATENCY_MS + prefix_writes * FLASH_WRITE_LATENCY_MS

        if f"{prefix}_calls" in r:
            r[f"{prefix}_reads_per_call"] = safe_div(prefix_reads, prefix_calls)
            r[f"{prefix}_writes_per_call"] = safe_div(prefix_writes, prefix_calls)
            r[f"{prefix}_estimated_ms_per_call"] = safe_div(safe_float(r[f"{prefix}_flash_estimated_ms"]), prefix_calls)
            r[f"{prefix}_measured_ms_per_call"] = safe_div(safe_float(r[f"{prefix}_flash_measured_ms"]), prefix_calls)

# Header stable.
fieldnames = []
for r in rows:
    for key in r.keys():
        if key not in fieldnames:
            fieldnames.append(key)

with dst.open("w", encoding="utf-8", newline="") as f:
    writer = csv.DictWriter(f, fieldnames=fieldnames)
    writer.writeheader()
    for r in rows:
        writer.writerow({name: r.get(name, "") for name in fieldnames})

print(f"OK: écrit {dst} ({len(rows)} lignes)")

# Fichiers enrichis par profil swap.
profiles = sorted({r.get("swap_profile", "unknown") or "unknown" for r in rows})
for profile in profiles:
    profile_path = OUTPUT_DIR / f"all_runs_enriched_{profile}.csv"
    with profile_path.open("w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()
        for r in rows:
            if (r.get("swap_profile", "unknown") or "unknown") == profile:
                writer.writerow({name: r.get(name, "") for name in fieldnames})
    print(f"OK: écrit {profile_path}")

# Compat si un seul profil: copie explicite du fichier enrichi principal vers le nom profilé déjà faite ci-dessus.


def write_summary(path, groups):
    fields = [
        "swap_profile",
        "layout",
        "file",
        "graph_storage",
        "neighbor_access",
        "exploration_policy",
        "bucket_state",
        "heuristic",
        "bucket_tile_side",
        "mode",
        "algo",
        "groups",
        "runs",
    ] + [f"avg_{m}" for m in SUMMARY_METRICS]
    with path.open("w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        for (swap_profile, layout, file, graph_storage, neighbor_access, exploration_policy, state_mode, heuristic, tile_side, mode, algo), group_rows in sorted(groups.items()):
            out = {
                "swap_profile": swap_profile,
                "layout": layout,
                "file": file,
                "graph_storage": graph_storage,
                "neighbor_access": neighbor_access,
                "exploration_policy": exploration_policy,
                "bucket_state": state_mode,
                "heuristic": heuristic,
                "bucket_tile_side": tile_side,
                "mode": mode,
                "algo": algo,
                "groups": len(group_rows),
                "runs": sum(int(r.get("_runs", 1)) for r in group_rows),
            }
            for metric in SUMMARY_METRICS:
                out[f"avg_{metric}"] = mean(safe_float_or_none(r.get(metric)) for r in group_rows)
            writer.writerow(out)

# 1. Chaque run compte pour 1.
by_run = defaultdict(list)
for r in rows:
    key = (
        r.get("swap_profile", "unknown"),
        r.get("layout", "base"),
        r.get("file", ""),
        r.get("graph_storage", DEFAULT_GRAPH_STORAGE),
        r.get("neighbor_access", DEFAULT_NEIGHBOR_ACCESS),
        r.get("exploration_policy", DEFAULT_EXPLORATION_POLICY),
        bucket_state(r),
        r.get("heuristic", DEFAULT_HEURISTIC),
        bucket_tile_side(r),
        r.get("mode", "perf"),
        r.get("algo", ""),
    )
    by_run[key].append({**r, "_runs": "1"})
write_summary(summary_by_run_dst, by_run)

# 2. Moyenne par scénario fichier/case/algo/mode, puis résumé par algo/mode/layout/profile.
case_groups = defaultdict(list)
for r in rows:
    key = (
        r.get("swap_profile", "unknown"),
        r.get("layout", "base"),
        r.get("file", ""),
        r.get("graph_storage", DEFAULT_GRAPH_STORAGE),
        r.get("neighbor_access", DEFAULT_NEIGHBOR_ACCESS),
        r.get("exploration_policy", DEFAULT_EXPLORATION_POLICY),
        bucket_state(r),
        r.get("heuristic", DEFAULT_HEURISTIC),
        bucket_tile_side(r),
        r.get("mode", "perf"),
        r.get("algo", ""),
        r.get("case_id", ""),
    )
    case_groups[key].append(r)

case_rows = []
for (swap_profile, layout, file, graph_storage, neighbor_access, exploration_policy, state_mode, heuristic, tile_side, mode, algo, case_id), group_rows in case_groups.items():
    row = {
        "swap_profile": swap_profile,
        "layout": layout,
        "file": file,
        "graph_storage": graph_storage,
        "neighbor_access": neighbor_access,
        "exploration_policy": exploration_policy,
        "bucket_state": state_mode,
        "heuristic": heuristic,
        "bucket_tile_side": tile_side,
        "mode": mode,
        "algo": algo,
        "case_id": case_id,
        "_runs": str(len(group_rows)),
    }
    for metric in SUMMARY_METRICS:
        row[metric] = mean(safe_float_or_none(r.get(metric)) for r in group_rows)
    case_rows.append(row)

with summary_by_case_dst.open("w", encoding="utf-8", newline="") as f:
    fields = [
        "swap_profile",
        "layout",
        "file",
        "graph_storage",
        "neighbor_access",
        "exploration_policy",
        "bucket_state",
        "heuristic",
        "bucket_tile_side",
        "mode",
        "algo",
        "case_id",
        "_runs",
    ] + SUMMARY_METRICS
    writer = csv.DictWriter(f, fieldnames=fields)
    writer.writeheader()
    for r in sorted(case_rows, key=lambda x: (
        x["swap_profile"],
        x["layout"],
        x["file"],
        x["graph_storage"],
        x["neighbor_access"],
        x["exploration_policy"],
        x["bucket_state"],
        x["heuristic"],
        x["bucket_tile_side"],
        x["mode"],
        x["algo"],
        x["case_id"],
    )):
        writer.writerow(r)

by_case_algo = defaultdict(list)
for r in case_rows:
    key = (
        r["swap_profile"],
        r["layout"],
        r["file"],
        r["graph_storage"],
        r["neighbor_access"],
        r["exploration_policy"],
        r["bucket_state"],
        r["heuristic"],
        r["bucket_tile_side"],
        r["mode"],
        r["algo"],
    )
    by_case_algo[key].append(r)
write_summary(summary_dst, by_case_algo)

print(f"OK: écrit {summary_by_run_dst}, {summary_by_case_dst}, {summary_dst}")
