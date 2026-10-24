import csv
import re
import os
import sys
from pathlib import Path
from publish import bucket_tile_side, bucket_state

ROOT = Path.cwd()
_catalog = Path(os.environ.get("BENCH_CATALOG_PATH", "bench/catalog.csv"))
CATALOG = _catalog if _catalog.is_absolute() else ROOT / _catalog
_public_stats = os.environ.get("BENCH_CATALOG_PUBLIC_STATS_ROOT", "").strip()
PUBLIC_STATS_ROOT = (
    Path(_public_stats) if Path(_public_stats).is_absolute() else ROOT / _public_stats
) if _public_stats else None
FIELDS = [
    "dataset",
    "label",
    "csv",
    "gps_layout",
    "graph_storage",
    "neighbor_access",
    "exploration_policy",
    "bucket_state",
    "heuristic",
    "bucket_tile_side",
    "gps_file",
    "swap_profile",
]

LABELS = {
    "base": "Base",
    "hilbert": "Hilbert",
    "z_curve": "Z-curve",
    "zcurve": "Z-curve",
}


def slug(value: str) -> str:
    value = value.strip().lower()
    value = re.sub(r"[^a-z0-9]+", "_", value)
    return value.strip("_") or "dataset"


def rel(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def basename(path: str) -> str:
    return str(path or "").split("/")[-1]


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
        return list(csv.DictReader(f))


def write_catalog(rows):
    CATALOG.parent.mkdir(parents=True, exist_ok=True)
    with CATALOG.open("w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=FIELDS)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in FIELDS})


def config_values(row):
    return (
        row.get("graph_storage") or "tile_dicho",
        row.get("neighbor_access") or "tile_segments",
        row.get("exploration_policy") or "strict",
        bucket_state(row),
        row.get("heuristic") or "standard",
        bucket_tile_side(row),
    )


def dataset_label(layout, profile, graph_storage, neighbor_access, exploration_policy, state_mode, heuristic, tile_side=""):
    layout_label = LABELS.get(layout, layout)
    ram = profile.split("_")[-1] if "_" in profile else profile
    return (
        f"{layout_label} / {ram}Ko / "
        f"{graph_storage} / {neighbor_access} / {exploration_policy}"
        + (f" / {state_mode}" if state_mode else "")
        + f" / {heuristic}"
        + (f" / tile{tile_side}" if tile_side else "")
    )


def catalog_csv_path(csv_path: Path, scan_root: Path) -> str:
    if PUBLIC_STATS_ROOT is not None:
        relative = csv_path.resolve().relative_to(scan_root.resolve())
        return rel(PUBLIC_STATS_ROOT / relative)
    return rel(csv_path)


def discover_file(csv_path: Path, scan_root: Path):
    rows = read_csv(csv_path)
    if not rows:
        return []

    combos = sorted({
        (
            row.get("swap_profile") or profile_from_path(csv_path),
            row.get("layout") or infer_layout(row.get("file", "")),
            basename(row.get("file", "")),
            *config_values(row),
        )
        for row in rows
        if row.get("file")
    })

    out = []
    for profile, layout, file, graph_storage, neighbor_access, exploration_policy, state_mode, heuristic, tile_side in combos:
        if layout == "zcurve":
            layout = "z_curve"
        csv_rel = catalog_csv_path(csv_path, scan_root)
        dataset = slug(
            f"{layout}_{profile}_{graph_storage}_{neighbor_access}_{exploration_policy}_{state_mode}_{heuristic}_{file}"
            + (f"_tile{tile_side}" if tile_side else "")
        )
        out.append({
            "dataset": dataset,
            "label": dataset_label(layout, profile, graph_storage, neighbor_access, exploration_policy, state_mode, heuristic, tile_side),
            "csv": csv_rel,
            "gps_layout": layout,
            "graph_storage": graph_storage,
            "neighbor_access": neighbor_access,
            "exploration_policy": exploration_policy,
            "bucket_state": state_mode,
            "heuristic": heuristic,
            "bucket_tile_side": tile_side,
            "gps_file": file,
            "swap_profile": profile,
        })
    return out


def should_index(path: Path, scan_root: Path):
    try:
        parts = path.resolve().relative_to(scan_root.resolve()).parts
    except ValueError:
        return False

    # Racine attendue : <stats_root>/base/<profile>/... ou
    # <stats_root>/layouts/<profile>/<layout>/...
    if len(parts) >= 3 and parts[0] == "base":
        return path.name.startswith("all_runs_enriched_")
    if len(parts) >= 4 and parts[0] == "layouts":
        # Ignorer un éventuel CSV global layouts/<profile>/... afin d'éviter les doublons.
        return parts[2] in {"hilbert", "zcurve"} and path.name.startswith("all_runs_enriched_")
    return False


def discover(paths):
    files = []
    for raw in paths:
        scan_root = Path(raw)
        if not scan_root.is_absolute():
            scan_root = ROOT / scan_root
        if scan_root.is_file():
            files.append((scan_root, scan_root.parent))
        elif scan_root.exists():
            files.extend(
                (path, scan_root)
                for path in sorted(scan_root.rglob("all_runs_enriched_*.csv"))
            )

    rows = []
    seen = set()
    for csv_path, scan_root in files:
        if csv_path.name == "all_runs_enriched.csv":
            continue
        if not should_index(csv_path, scan_root):
            continue
        for row in discover_file(csv_path, scan_root):
            key = row["dataset"]
            if key not in seen:
                rows.append(row)
                seen.add(key)
    return sorted(rows, key=lambda r: (r["gps_layout"], r["swap_profile"], r["graph_storage"], r["neighbor_access"], r["exploration_policy"], r["bucket_state"], r["heuristic"], r["bucket_tile_side"]))


def main():
    paths = sys.argv[1:] or ["bench/results/stats"]
    rows = discover(paths)
    write_catalog(rows)
    print(f"OK: catalogue écrit: {CATALOG} ({len(rows)} datasets)")


if __name__ == "__main__":
    main()
