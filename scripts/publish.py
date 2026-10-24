#!/usr/bin/env python3
"""Fonctions de publication partagées pour le pipeline de benchmark fondé sur la reconstruction.

Le parcours utilisateur est défini dans scripts/bench.sh. Ce module ne fournit volontairement
aucune commande d'ajout ou de mise à jour : bench/results est toujours reconstruit à partir de bench/raw.
"""

import csv
from algorithm_registry import normalize_algo, state_for, policy_for
import hashlib
import os
import shutil
import sys
from pathlib import Path

ROOT = Path.cwd()

def root_path_from_env(name: str, default: str) -> Path:
    value = Path(os.environ.get(name, default))
    return value if value.is_absolute() else ROOT / value


RESULTS = root_path_from_env("BENCH_RESULTS_ROOT", "bench/results")
PUBLIC_RESULTS = root_path_from_env("BENCH_RESULTS_PUBLIC_ROOT", "bench/results")
STATS_RESULTS = RESULTS / "stats"
EVENTS_RESULTS = RESULTS / "events"
STATS_RUN_INDEX = STATS_RESULTS / "run_index.csv"
EVENTS_INDEX = EVENTS_RESULTS / "index.csv"

RUN_INDEX_FIELDS = [
    "published_at",
    "run_id",
    "raw_dir",
    "published_csv",
    "duplicate_key",
    "duplicate_index",
    "duplicate_count_after",
    "warning",
    "case_id",
    "file",
    "layout",
    "mode",
    "algo",
    "bucket_state",
    "swap_profile",
    "graph_storage",
    "neighbor_access",
    "exploration_policy",
    "heuristic",
    "bucket_tile_side",
    "start_lon",
    "start_lat",
    "end_lon",
    "end_lat",
    "events_file",
    "events_count",
    "events_version",
    "inspect_stride",
]

EVENT_INDEX_FIELDS = [
    "published_at",
    "run_id",
    "raw_dir",
    "duplicate_key",
    "duplicate_index",
    "duplicate_count_after",
    "warning",
    "case_id",
    "mode",
    "timestamp",
    "events_file",
    "events_count",
    "events_version",
    "inspect_stride",
    "file",
    "layout",
    "algo",
    "algo_backend",
    "bucket_state",
    "swap_profile",
    "profile",
    "graph_storage",
    "neighbor_access",
    "exploration_policy",
    "heuristic",
    "bucket_tile_side",
    "start_lon",
    "start_lat",
    "end_lon",
    "end_lat",
    "found",
    "cost",
    "path_len",
]


def basename(path: str) -> str:
    return str(path or "").split("/")[-1]


def infer_layout(file: str) -> str:
    name = basename(file).lower()
    if "hilbert" in name:
        return "hilbert"
    if "zcurve" in name or "z_curve" in name:
        return "zcurve"
    return "base"


def normalize_layout(value: str) -> str:
    value = (value or "").strip().lower()
    if value == "z_curve":
        return "zcurve"
    return value or "base"


def csv_layout_value(layout: str) -> str:
    return "z_curve" if layout == "zcurve" else layout


def read_csv(path: Path):
    with path.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        return reader.fieldnames or [], list(reader)


def row_mode(row) -> str:
    return (row.get("mode") or row.get("profile") or "perf").strip().lower()



def bucket_state(row) -> str:
    return state_for(row)


def bucket_tile_side(row) -> str:
    value = str(row.get("bucket_tile_side") or "").strip()
    if value:
        try:
            return str(int(value))
        except ValueError:
            pass
    return value


def exact_key(row):
    """Identité stable d'une mesure brute, sans timestamp ni run_id."""
    parts = [
        row.get("case_id", ""),
        basename(row.get("file", "")),
        normalize_layout(row.get("layout") or infer_layout(row.get("file", ""))),
        row_mode(row),
        normalize_algo(row.get("algo")),
        row.get("swap_profile", ""),
        row.get("graph_storage") or "tile_dicho",
        row.get("neighbor_access") or "tile_segments",
        policy_for(row),
        row.get("heuristic") or "standard",
        row.get("start_lon", ""),
        row.get("start_lat", ""),
        row.get("end_lon", ""),
        row.get("end_lat", ""),
    ]
    # Conserver les identités historiques à l'octet près lorsque les nouvelles dimensions sont vides.
    state = bucket_state(row)
    if state:
        parts.append(f"bucket_state={state}")
    tile_side = bucket_tile_side(row)
    if tile_side:
        parts.append(f"bucket_tile_side={tile_side}")
    raw = "\x1f".join(str(part) for part in parts)
    return hashlib.sha1(raw.encode("utf-8")).hexdigest()[:16]


def project_relative(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def public_result_path(physical_path: Path) -> str:
    """Chemin final stable, même pendant une reconstruction dans un répertoire temporaire."""
    relative = physical_path.relative_to(RESULTS)
    return project_relative(PUBLIC_RESULTS / relative)


def published_stats_path(profile: str, layout: str) -> Path:
    if layout == "base":
        return STATS_RESULTS / "base" / profile / f"all_runs_enriched_{profile}.csv"
    return STATS_RESULTS / "layouts" / profile / layout / f"all_runs_enriched_{profile}.csv"


def publish_event_artifact(row, profile: str, layout: str, run_id: str, raw_dir: str) -> str:
    value = (row.get("events_file") or "").strip()
    if not value:
        return ""

    source = Path(value)
    candidates = []
    if source.is_absolute():
        candidates.append(source)
    else:
        if raw_dir:
            candidates.append(Path(raw_dir) / source.name)
        candidates.append(ROOT / source)

    source = next((candidate for candidate in candidates if candidate.is_file()), None)
    if source is None:
        print(f"WARN: fichier events introuvable: {value}", file=sys.stderr)
        return ""

    safe_run_id = run_id or row.get("run_id") or "untracked"
    destination = EVENTS_RESULTS / profile / layout / safe_run_id / source.name
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    return public_result_path(destination)
