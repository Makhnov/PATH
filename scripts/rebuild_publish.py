#!/usr/bin/env python3
import argparse
import csv
import json
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from time import monotonic

import publish

PROFILES = {"256_64", "256_128", "256_256"}


def profile_dirs(raw_root: Path) -> list[Path]:
    return sorted(
        path
        for path in raw_root.rglob("*")
        if path.is_dir() and path.name in PROFILES
    )


def write_csv(path: Path, fieldnames: list[str], rows: list[dict[str, str]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        writer.writerows(
            {field: row.get(field, "") for field in fieldnames}
            for row in rows
        )
    print(f"OK: écrit {path} ({len(rows)} lignes)", flush=True)


def add_fields(target: list[str], source: list[str]) -> None:
    for field in source:
        if field not in target:
            target.append(field)


def source_rows(raw_root: Path):
    stats_groups: dict[tuple[str, str], list[dict[str, str]]] = defaultdict(list)
    event_rows: list[tuple[dict[str, str], str, str, str]] = []
    stats_fields: list[str] = []
    profile_count = 0

    for directory in profile_dirs(raw_root):
        profile_count += 1
        profile = directory.name
        relative = directory.resolve().relative_to(raw_root.resolve())
        fallback_run_id = f"raw_{profile}" if relative.parent == Path(".") else relative.parent.name
        enriched = directory / f"all_runs_enriched_{profile}.csv"
        if not enriched.is_file():
            raise SystemExit(f"CSV enrichi absent: {enriched}")

        fieldnames, rows = publish.read_csv(enriched)
        add_fields(stats_fields, fieldnames)

        for original in rows:
            row = dict(original)
            row["algo"] = publish.normalize_algo(row.get("algo"))
            row["swap_profile"] = row.get("swap_profile") or profile
            row["profile"] = row.get("profile") or publish.row_mode(row)
            row["graph_storage"] = row.get("graph_storage") or "tile_dicho"
            row["neighbor_access"] = row.get("neighbor_access") or "tile_segments"
            row["exploration_policy"] = publish.policy_for(row)
            row["bucket_state"] = publish.bucket_state(row)
            row["heuristic"] = row.get("heuristic") or "standard"
            row["bucket_tile_side"] = publish.bucket_tile_side(row)
            layout = publish.normalize_layout(
                row.get("layout") or publish.infer_layout(row.get("file", ""))
            )
            row["layout"] = publish.csv_layout_value(layout)
            row["run_id"] = row.get("run_id") or fallback_run_id
            row["raw_dir"] = row.get("raw_dir") or str(directory.resolve())

            stats_groups[(profile, layout)].append(row)
            if publish.row_mode(row) == "inspect":
                event_rows.append((row, profile, layout, str(directory.resolve())))

    add_fields(stats_fields, ["run_id", "raw_dir", "duplicate_key", "duplicate_index", "bucket_state", "bucket_tile_side"])
    return profile_count, stats_fields, stats_groups, event_rows


def publish_stats(
    fieldnames: list[str],
    groups: dict[tuple[str, str], list[dict[str, str]]],
    now: str,
):
    index_rows = []
    stats_count = 0

    for profile, layout in sorted(groups):
        rows = groups[(profile, layout)]
        counts = defaultdict(int)
        published_rows = []
        for source_row in rows:
            row = dict(source_row)
            key = publish.exact_key(row)
            counts[key] += 1
            row["duplicate_key"] = key
            row["duplicate_index"] = str(counts[key])
            # Le CSV des statistiques ne doit pas pointer vers les données brutes. Conserver
            # events_file dans source_row pour que publish_events puisse copier le replay.
            row["events_file"] = ""
            published_rows.append(row)
            warning = "duplicate_exact_run" if counts[key] > 1 else ""

            path = publish.published_stats_path(profile, layout)
            index_rows.append({
                "published_at": now,
                "run_id": row.get("run_id", ""),
                "raw_dir": row.get("raw_dir", ""),
                "published_csv": publish.public_result_path(path),
                "duplicate_key": key,
                "duplicate_index": str(counts[key]),
                "duplicate_count_after": str(counts[key]),
                "warning": warning,
                "case_id": row.get("case_id", ""),
                "file": publish.basename(row.get("file", "")),
                "layout": row.get("layout", ""),
                "mode": publish.row_mode(row),
                "algo": row.get("algo", ""),
                "bucket_state": publish.bucket_state(row),
                "swap_profile": row.get("swap_profile", ""),
                "graph_storage": row.get("graph_storage", "tile_dicho"),
                "neighbor_access": row.get("neighbor_access", "tile_segments"),
                "exploration_policy": row.get("exploration_policy", "strict"),
                "heuristic": row.get("heuristic", "standard"),
                "bucket_tile_side": publish.bucket_tile_side(row),
                "start_lon": row.get("start_lon", ""),
                "start_lat": row.get("start_lat", ""),
                "end_lon": row.get("end_lon", ""),
                "end_lat": row.get("end_lat", ""),
                "events_file": "",
                "events_count": "",
                "events_version": "",
                "inspect_stride": "",
            })

        write_csv(publish.published_stats_path(profile, layout), fieldnames, published_rows)
        stats_count += len(published_rows)

    write_csv(publish.STATS_RUN_INDEX, publish.RUN_INDEX_FIELDS, index_rows)
    return stats_count


def publish_events(rows, now: str):
    out_rows = []
    counts = defaultdict(int)
    total = len(rows)
    started = last_progress = monotonic()
    interactive = sys.stdout.isatty()
    progress_interval = 0.2 if interactive else 5
    print(f"=== copie des replays : {total} fichier(s) ===", flush=True)

    def show_progress(index: int, current: float) -> None:
        nonlocal last_progress
        filled = index * 30 // total if total else 30
        percent = index * 100 // total if total else 100
        bar = "█" * filled + "░" * (30 - filled)
        message = (
            f"Replays [{bar}] {percent:3d} % · {index}/{total} "
            f"· {current - started:.1f} s"
        )
        print(
            ("\r" if interactive else "") + message,
            end="" if interactive and index < total else "\n",
            flush=True,
        )
        last_progress = current

    show_progress(0, started)
    for index, (row, profile, layout, raw_dir) in enumerate(rows, start=1):
        key = publish.exact_key(row)
        counts[key] += 1
        warning = "duplicate_exact_run" if counts[key] > 1 else ""
        try:
            events_file = publish.publish_event_artifact(
                row,
                profile,
                layout,
                row.get("run_id", ""),
                raw_dir,
            )
        except BaseException:
            if interactive:
                print(flush=True)
            raise
        if not events_file:
            if interactive:
                print(flush=True)
            raise SystemExit(
                "Run inspect sans artefact: "
                f"{row.get('run_id','')} {row.get('case_id','')} {row.get('algo','')}"
            )
        current = monotonic()
        if index == total or current - last_progress >= progress_interval:
            show_progress(index, current)
        # En staging, le chemin public pointe vers le futur results. Le fichier physique
        # est garanti par publish_event_artifact; la validation résout ce mapping.
        out_rows.append({
            "published_at": now,
            "run_id": row.get("run_id", ""),
            "raw_dir": row.get("raw_dir") or raw_dir,
            "duplicate_key": key,
            "duplicate_index": str(counts[key]),
            "duplicate_count_after": str(counts[key]),
            "warning": warning,
            "case_id": row.get("case_id", ""),
            "mode": "inspect",
            "timestamp": row.get("timestamp", ""),
            "events_file": events_file,
            "events_count": row.get("events_count", ""),
            "events_version": row.get("events_version", ""),
            "inspect_stride": row.get("inspect_stride", ""),
            "file": row.get("file", ""),
            "layout": publish.csv_layout_value(layout),
            "algo": row.get("algo", ""),
            "algo_backend": row.get("algo_backend", ""),
            "bucket_state": publish.bucket_state(row),
            "swap_profile": row.get("swap_profile") or profile,
            "profile": row.get("profile", ""),
            "graph_storage": row.get("graph_storage") or "tile_dicho",
            "neighbor_access": row.get("neighbor_access") or "tile_segments",
            "exploration_policy": row.get("exploration_policy") or "strict",
            "heuristic": row.get("heuristic") or "standard",
            "bucket_tile_side": publish.bucket_tile_side(row),
            "start_lon": row.get("start_lon", ""),
            "start_lat": row.get("start_lat", ""),
            "end_lon": row.get("end_lon", ""),
            "end_lat": row.get("end_lat", ""),
            "found": row.get("found", ""),
            "cost": row.get("cost", ""),
            "path_len": row.get("path_len", ""),
        })

    write_csv(publish.EVENTS_INDEX, publish.EVENT_INDEX_FIELDS, out_rows)
    return len(out_rows)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw-root", default="bench/raw")
    args = parser.parse_args()
    raw_root = Path(args.raw_root)
    if not raw_root.is_absolute():
        raw_root = publish.ROOT / raw_root
    raw_root = raw_root.resolve()

    now = datetime.now(timezone.utc).isoformat()
    profile_count, fields, stats_groups, events = source_rows(raw_root)
    if profile_count == 0:
        raise SystemExit(f"Aucun dossier de profil sous {raw_root}")

    stats_count = publish_stats(fields, stats_groups, now)
    event_count = publish_events(events, now)

    manifest = {
        "rebuilt_at": now,
        "source": str(raw_root),
        "profile_directories": profile_count,
        "stats_rows": stats_count,
        "event_rows": event_count,
        "stats_csvs": len(stats_groups),
        "public_results": str(publish.PUBLIC_RESULTS),
    }
    manifest_path = publish.RESULTS / "rebuild_manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding="utf-8")
    print(
        f"OK: publication globale rebuild: profils={profile_count}, "
        f"stats={stats_count}, events={event_count}",
        flush=True,
    )


if __name__ == "__main__":
    main()
