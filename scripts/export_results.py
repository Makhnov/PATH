#!/usr/bin/env python3
import csv
from algorithm_registry import descriptor, normalize_algo
import json
import math
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from publish import bucket_state

ROOT = Path.cwd()

SC_FIELDS = [
    "cache_hits",
    "cache_misses",
    "flash_reads",
    "flash_writes",
    "flash_allocs",
    "flash_frees",
    "flash_erases",
    "cache_total_micros",
    "cache_used_micros",
    "cache_unused_micros",
    "flash_reads_micros",
    "flash_writes_micros",
    "btree_micros",
    "heap_micros",
    "rrb_micros",
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

MEAN_METRICS = [
    "search_ms",
    "search_flash_measured_ms",
    "search_flash_estimated_ms",
    "nodes_visited",
    "neighbors_visited",
    "edges_visited",
    "heap_pushes",
    "heap_pops",
    "countdown_edges",
    "countdown_done_decs",
    "countdown_done_removes",
    "search_flash_reads",
    "search_flash_writes",
]

CASE_LABELS = {
    "case_001": "Hypercentre",
    "case_002": "Centre ville",
    "case_003": "Banlieue (NE-SO)",
    "case_004": "Banlieue (N-S)",
    "case_005": "Banlieue (E-O)",
}

LAYOUT_LABELS = {
    "base": "Base",
    "hilbert": "Hilbert",
    "z_curve": "Z-curve",
    "zcurve": "Z-curve",
}


def rel(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def basename(value: str) -> str:
    return str(value or "").split("/")[-1]


def infer_layout(file: str) -> str:
    name = basename(file).lower()
    if "hilbert" in name:
        return "hilbert"
    if "zcurve" in name or "z_curve" in name:
        return "z_curve"
    return "base"


def profile_from_path(path: Path) -> str:
    name = path.name
    prefix = "all_runs_enriched_"
    if name.startswith(prefix) and name.endswith(".csv"):
        return name[len(prefix):-4]
    return "unknown"


def read_csv(path: Path):
    with path.open(encoding="utf-8", newline="") as f:
        reader = csv.DictReader(f)
        return reader.fieldnames or [], list(reader)


def clean_str(value):
    if value is None:
        return ""
    return str(value).strip()


def as_bool(value):
    v = clean_str(value).lower()
    if v in {"true", "1", "yes", "y"}:
        return True
    if v in {"false", "0", "no", "n"}:
        return False
    return None


def as_number(value):
    v = clean_str(value)
    if v == "":
        return None
    try:
        n = float(v)
    except ValueError:
        return None
    if not math.isfinite(n):
        return None
    if n.is_integer():
        return int(n)
    return n


def first(row, *keys, default=""):
    for key in keys:
        value = row.get(key)
        if clean_str(value) != "":
            return value
    return default


def row_mode(row):
    return clean_str(first(row, "mode", "profile", default="perf")).lower() or "perf"


def algorithm_variant(row):
    algo = normalize_algo(row.get("algo"))
    entry = descriptor(algo)
    if entry and (len(entry["states"]) > 1 or len(entry["policies"]) > 1):
        policy = clean_str(first(row, "exploration_policy", default=entry["policies"][0]))
        return f"{algo}/{bucket_state(row)}/{policy}"
    return algo


def number(row, *keys):
    return as_number(first(row, *keys))


def compact_dict(d):
    return {k: v for k, v in d.items() if v not in (None, "", {}, [])}


def counter_group(row, prefix: str):
    values = {}
    for field in SC_FIELDS:
        value = number(row, f"{prefix}_{field}")
        if value is not None:
            values[field] = value
    return values


def detail_group(row, prefix: str):
    values = {}
    calls = number(row, f"{prefix}_calls")
    if calls is not None:
        values["calls"] = calls
    counters = counter_group(row, prefix)
    if counters:
        values["cache_counter"] = counters
    return values


def run_object(row):
    file_name = basename(row.get("file", ""))
    layout = first(row, "gps_layout", "layout", default="") or infer_layout(file_name)
    if layout == "zcurve":
        layout = "z_curve"

    details = {}
    for prefix in SC_DETAIL_PREFIXES:
        detail = detail_group(row, prefix)
        if detail:
            details[prefix] = detail

    return compact_dict({
        "case_id": row.get("case_id", ""),
        "case_label": CASE_LABELS.get(row.get("case_id", ""), row.get("case_id", "")),
        "mode": first(row, "mode", "profile", default=""),
        "algo": row.get("algo", ""),
        "file": file_name,
        "layout": layout,
        "swap_profile": first(row, "swap_profile", default=""),
        "config": compact_dict({
            "graph_storage": first(row, "graph_storage", default="tile_dicho"),
            "neighbor_access": first(row, "neighbor_access", default="tile_segments"),
            "exploration_policy": first(row, "exploration_policy", default="strict"),
            "bucket_state": bucket_state(row),
            "heuristic": first(row, "heuristic", default="standard"),
        }),
        "coordinates": compact_dict({
            "start_lon": number(row, "start_lon"),
            "start_lat": number(row, "start_lat"),
            "end_lon": number(row, "end_lon"),
            "end_lat": number(row, "end_lat"),
        }),
        "result": compact_dict({
            "found": as_bool(row.get("found")),
            "cost": number(row, "cost"),
            "path_len": number(row, "path_len"),
        }),
        "timing": compact_dict({
            "decode_ms": number(row, "decode_ms"),
            "graph_ms": number(row, "graph_ms"),
            "search_ms": number(row, "search_ms", "measured_search_ms"),
            "total_ms": number(row, "total_ms"),
            "search_flash_measured_ms": number(row, "search_flash_measured_ms"),
            "search_flash_estimated_ms": number(row, "search_flash_estimated_ms"),
            "search_residual_after_measured_flash_ms": number(row, "search_residual_after_measured_flash_ms"),
            "search_residual_after_estimated_flash_ms": number(row, "search_residual_after_estimated_flash_ms"),
        }),
        "work": compact_dict({
            "heap_pushes": number(row, "heap_pushes"),
            "heap_pops": number(row, "heap_pops"),
            "nodes_skipped": number(row, "nodes_skipped"),
            "nodes_visited": number(row, "nodes_visited"),
            "neighbors_skipped": number(row, "neighbors_skipped"),
            "neighbors_visited": number(row, "neighbors_visited"),
            "edges_visited": number(row, "edges_visited"),
            "heuristic_calls": number(row, "heuristic_calls"),
            "max_heap": number(row, "max_heap"),
            "max_done": number(row, "max_done"),
            "max_previous": number(row, "max_previous"),
        }),
        "locality": compact_dict({
            "popped_same_tile": number(row, "popped_same_tile"),
            "popped_cross_tile": number(row, "popped_cross_tile"),
            "popped_tile_jump_sum": number(row, "popped_tile_jump_sum"),
            "popped_tile_jump_max": number(row, "popped_tile_jump_max"),
            "popped_cross_tile_rate": number(row, "popped_cross_tile_rate"),
            "visited_same_tile": number(row, "visited_same_tile"),
            "visited_cross_tile": number(row, "visited_cross_tile"),
            "visited_tile_jump_sum": number(row, "visited_tile_jump_sum"),
            "visited_tile_jump_max": number(row, "visited_tile_jump_max"),
            "visited_cross_tile_rate": number(row, "visited_cross_tile_rate"),
            "avg_visited_tile_jump": number(row, "avg_visited_tile_jump"),
            "neighbors_next_cross_tile_rate": number(row, "neighbors_next_cross_tile_rate"),
        }),
        "cache": compact_dict({
            "gps": counter_group(row, "gps"),
            "graph": counter_group(row, "graph"),
            "search": counter_group(row, "search"),
            "search_window": counter_group(row, "search_window"),
            "search_stats": counter_group(row, "search_stats"),
            "search_total": counter_group(row, "search_total"),
        }),
        "details": details,
    })


def unique_sorted(rows, key, fallback=None):
    values = set()
    for row in rows:
        if key == "layout":
            value = row.get("layout") or infer_layout(row.get("file", ""))
        else:
            value = row.get(key, "")
        value = clean_str(value)
        if value:
            values.add(value)
    if not values and fallback:
        values.add(fallback)
    return sorted(values)


def dataset_kind(csv_path: Path):
    parts = csv_path.parts
    if "base" in parts:
        return "base"
    if "layouts" in parts:
        return "layouts"
    return "unknown"


def summarize_group(rows):
    out = {"row_count": len(rows)}
    found_values = [as_bool(r.get("found")) for r in rows if as_bool(r.get("found")) is not None]
    if found_values:
        out["found_true"] = sum(1 for v in found_values if v)
        out["found_false"] = sum(1 for v in found_values if not v)

    modes = {row_mode(row) for row in rows}
    means = {}
    for metric in MEAN_METRICS:
        candidates = rows
        if len(modes) > 1:
            # Un jeu de données final mixte combine volontairement deux exécutions complémentaires :
            # le temps de référence vient de perf, les détails du travail et du cache
            # viennent d'inspect. Ne pas moyenner leurs temps ensemble.
            wanted = "perf" if metric == "search_ms" else "inspect"
            selected = [row for row in rows if row_mode(row) == wanted]
            if selected:
                candidates = selected
        vals = [as_number(r.get(metric)) for r in candidates]
        vals = [v for v in vals if v is not None]
        if vals:
            means[metric] = sum(vals) / len(vals)
    if means:
        out["mean"] = means
    return out


def build_summary(rows):
    summary = {
        "row_count": len(rows),
        "by_algorithm": {},
        "by_algorithm_and_mode": {},
        "by_case": {},
    }

    by_algo = defaultdict(list)
    by_algo_mode = defaultdict(list)
    by_case = defaultdict(list)
    for row in rows:
        variant = algorithm_variant(row)
        by_algo[variant].append(row)
        by_algo_mode[(variant, row_mode(row))].append(row)
        by_case[row.get("case_id", "")].append(row)

    summary["by_algorithm"] = {k: summarize_group(v) for k, v in sorted(by_algo.items()) if k}
    summary["by_algorithm_and_mode"] = {
        f"{algo}/{mode}": summarize_group(v)
        for (algo, mode), v in sorted(by_algo_mode.items())
        if algo or mode
    }
    summary["by_case"] = {k: summarize_group(v) for k, v in sorted(by_case.items()) if k}
    return summary


def build_manifest(csv_path: Path, fieldnames, rows):
    profile = unique_sorted(rows, "swap_profile", profile_from_path(csv_path))
    scenario_keys = sorted({f"{basename(r.get('file', ''))}:{r.get('case_id', '')}" for r in rows if r.get("case_id")})
    configs = sorted({
        (
            first(r, "graph_storage", default="tile_dicho"),
            first(r, "neighbor_access", default="tile_segments"),
            first(r, "exploration_policy", default="strict"),
            bucket_state(r),
            first(r, "heuristic", default="standard"),
        )
        for r in rows
    })
    algos = unique_sorted(rows, "algo")
    modes = sorted({row_mode(r) for r in rows})
    variants = sorted({
        (
            r.get("algo", ""),
            first(r, "graph_storage", default="tile_dicho"),
            first(r, "neighbor_access", default="tile_segments"),
            first(r, "exploration_policy", default="strict"),
            bucket_state(r),
            first(r, "heuristic", default="standard"),
        )
        for r in rows
        if r.get("algo")
    })
    expected = len(scenario_keys) * max(1, len(variants)) * max(1, len(modes))

    counts_by_mode = Counter(row_mode(r) for r in rows)
    counts_by_algo = Counter(r.get("algo", "") for r in rows)
    counts_by_file = Counter(basename(r.get("file", "")) for r in rows)

    return {
        "schema_version": 2,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "dataset_dir": rel(csv_path.parent),
        "csv": csv_path.name,
        "kind": dataset_kind(csv_path),
        "row_count": len(rows),
        "expected_row_count": expected,
        "complete": len(rows) == expected,
        "columns": fieldnames,
        "swap_profiles": profile,
        "layouts": unique_sorted(rows, "layout"),
        "gps_files": sorted(k for k in counts_by_file if k),
        "cases": sorted({r.get("case_id", "") for r in rows if r.get("case_id")}),
        "scenario_keys": scenario_keys,
        "algorithms": algos,
        "modes": modes,
        "configs": [
            {
                "graph_storage": graph_storage,
                "neighbor_access": neighbor_access,
                "exploration_policy": exploration_policy,
                "bucket_state": state_mode,
                "heuristic": heuristic,
            }
            for graph_storage, neighbor_access, exploration_policy, state_mode, heuristic in configs
        ],
        "variants": [
            {
                "algo": algo,
                "graph_storage": graph_storage,
                "neighbor_access": neighbor_access,
                "exploration_policy": exploration_policy,
                "bucket_state": state_mode,
                "heuristic": heuristic,
            }
            for algo, graph_storage, neighbor_access, exploration_policy, state_mode, heuristic in variants
        ],
        "counts": {
            "by_mode": dict(sorted(counts_by_mode.items())),
            "by_algorithm": dict(sorted(counts_by_algo.items())),
            "by_file": dict(sorted(counts_by_file.items())),
        },
        "files": {
            "runs_jsonl": "runs.jsonl",
            "summary_json": "summary.json",
            "readme": "README.md",
        },
    }


def readme_for(csv_path: Path, manifest, summary):
    title = rel(csv_path.parent)
    lines = []
    lines.append(f"# {title}")
    lines.append("")
    lines.append(f"- CSV: `{csv_path.name}`")
    lines.append(f"- Type: `{manifest['kind']}`")
    lines.append(f"- Lignes: `{manifest['row_count']}` / attendu `{manifest['expected_row_count']}`")
    lines.append(f"- Complet: `{manifest['complete']}`")
    lines.append(f"- Profils RAM: `{', '.join(manifest['swap_profiles'])}`")
    lines.append(f"- Layouts: `{', '.join(manifest['layouts'])}`")
    lines.append(f"- Fichiers GPS: `{', '.join(manifest['gps_files'])}`")
    lines.append(f"- Algorithmes: `{', '.join(manifest['algorithms'])}`")
    lines.append(f"- Modes: `{', '.join(manifest['modes'])}`")
    lines.append("")
    lines.append("## Configurations")
    lines.append("")
    lines.append("| graph_storage | neighbor_access | exploration_policy | bucket_state | heuristic |")
    lines.append("|---|---|---|---|---|")
    for cfg in manifest["configs"]:
        lines.append(
            f"| {cfg['graph_storage']} | {cfg['neighbor_access']} | {cfg['exploration_policy']} | {cfg['bucket_state']} | {cfg['heuristic']} |"
        )
    lines.append("")
    lines.append("## Cas")
    lines.append("")
    lines.append("| case_id | label |")
    lines.append("|---|---|")
    for case_id in manifest["cases"]:
        lines.append(f"| {case_id} | {CASE_LABELS.get(case_id, case_id)} |")
    lines.append("")
    lines.append("## Résumé par algorithme")
    lines.append("")
    lines.append("| algo | lignes | search_ms moyen | nodes_visited moyen | flash_reads moyen |")
    lines.append("|---|---:|---:|---:|---:|")
    for algo, data in sorted(summary.get("by_algorithm", {}).items()):
        mean = data.get("mean", {})
        lines.append(
            f"| {algo} | {data.get('row_count', 0)} | "
            f"{mean.get('search_ms', '')} | {mean.get('nodes_visited', '')} | {mean.get('search_flash_reads', '')} |"
        )
    lines.append("")
    lines.append("## Fichiers générés")
    lines.append("")
    lines.append("- `manifest.json` : métadonnées du dataset")
    lines.append("- `runs.jsonl` : une ligne JSON par run")
    lines.append("- `summary.json` : agrégats par algo/mode/cas")
    return "\n".join(lines) + "\n"


def export_csv(csv_path: Path):
    fieldnames, rows = read_csv(csv_path)
    if not rows:
        print(f"SKIP: {csv_path} est vide")
        return

    manifest = build_manifest(csv_path, fieldnames, rows)
    summary = build_summary(rows)

    out_dir = csv_path.parent
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )
    (out_dir / "summary.json").write_text(
        json.dumps(summary, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )
    with (out_dir / "runs.jsonl").open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(run_object(row), ensure_ascii=False, separators=(",", ":")) + "\n")
    (out_dir / "README.md").write_text(readme_for(csv_path, manifest, summary), encoding="utf-8")

    print(f"OK: export {csv_path} -> manifest.json, runs.jsonl, summary.json, README.md")


def discover(target: Path):
    if target.is_file():
        return [target]
    files = sorted(target.rglob("all_runs_enriched_*.csv"))
    return [f for f in files if f.name != "all_runs_enriched.csv"]


def main():
    target = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("bench/results/stats")
    if not target.is_absolute():
        target = ROOT / target
    files = discover(target)
    if not files:
        raise SystemExit(f"Aucun all_runs_enriched_<profile>.csv trouvé dans {target}")
    for csv_path in files:
        export_csv(csv_path)
    print(f"OK: exports humains générés pour {len(files)} CSV")


if __name__ == "__main__":
    main()
