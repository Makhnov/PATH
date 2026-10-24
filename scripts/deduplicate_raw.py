#!/usr/bin/env python3
import argparse
import csv
import shutil
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

import publish

PROFILES = {"256_128", "256_256", "256_64"}
DERIVED_EXACT = {
    "all_runs.csv",
    "all_runs_enriched.csv",
    "all_runs_latest.csv",
    "summary_by_algo.csv",
    "summary_by_case.csv",
    "summary_by_run.csv",
    "publish_manifest.json",
}


@dataclass
class RawRun:
    path: Path
    profile_dir: Path
    row: dict[str, str]

    @property
    def run_id(self) -> str:
        return (self.row.get("run_id") or "").strip()

    def sort_key(self) -> tuple[str, int, str]:
        timestamp = (self.row.get("timestamp") or "").strip()
        try:
            mtime = self.path.stat().st_mtime_ns
        except FileNotFoundError:
            mtime = 0
        return timestamp, mtime, self.path.as_posix()


def profile_dirs(raw_root: Path) -> list[Path]:
    return sorted(
        path
        for path in raw_root.rglob("*")
        if path.is_dir() and path.name in PROFILES
    )


def is_derived(path: Path) -> bool:
    name = path.name
    return (
        name in DERIVED_EXACT
        or name.startswith("all_runs_enriched_")
        or name.startswith("summary_")
        or name.startswith("compare_")
    )


def read_raw_run(path: Path, profile_dir: Path) -> RawRun | None:
    if not path.is_file() or is_derived(path) or path.name.endswith(".events.jsonl"):
        return None

    try:
        with path.open(encoding="utf-8", newline="") as handle:
            reader = csv.DictReader(handle)
            if not reader.fieldnames or "case_id" not in reader.fieldnames:
                return None
            rows = [row for row in reader if row and row.get("case_id")]
    except (OSError, UnicodeError, csv.Error):
        return None

    if len(rows) != 1:
        return None
    return RawRun(path=path, profile_dir=profile_dir, row=rows[0])


def event_path(run: RawRun, raw_root: Path) -> Path | None:
    value = (run.row.get("events_file") or "").strip()
    if not value:
        return None

    source = Path(value)
    candidates = [run.profile_dir / source.name]
    if source.is_absolute():
        candidates.append(source)
    else:
        candidates.append(publish.ROOT / source)

    root = raw_root.resolve()
    for candidate in candidates:
        try:
            resolved = candidate.resolve()
            resolved.relative_to(root)
        except (OSError, ValueError):
            continue
        if resolved.is_file():
            return resolved
    return None


def clear_derived(profile_dir: Path) -> None:
    for path in list(profile_dir.iterdir()):
        if path.is_file() and is_derived(path):
            path.unlink()


def remaining_raw_runs(profile_dir: Path) -> bool:
    return any(read_raw_run(path, profile_dir) is not None for path in profile_dir.iterdir())


def remove_empty_parents(raw_root: Path) -> None:
    for directory in sorted(
        (path for path in raw_root.rglob("*") if path.is_dir()),
        key=lambda path: len(path.parts),
        reverse=True,
    ):
        try:
            directory.rmdir()
        except OSError:
            pass


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw-root", default="bench/raw")
    parser.add_argument("--prefer-run-id", default="")
    args = parser.parse_args()

    raw_root = Path(args.raw_root)
    if not raw_root.is_absolute():
        raw_root = publish.ROOT / raw_root
    raw_root = raw_root.resolve()
    if not raw_root.exists():
        return

    groups: dict[str, list[RawRun]] = defaultdict(list)
    for profile_dir in profile_dirs(raw_root):
        for path in sorted(profile_dir.iterdir()):
            run = read_raw_run(path, profile_dir)
            if run is not None:
                groups[publish.exact_key(run.row)].append(run)

    removed = 0
    affected: set[Path] = set()
    for runs in groups.values():
        if len(runs) < 2:
            continue

        preferred = [run for run in runs if run.run_id == args.prefer_run_id]
        if args.prefer_run_id and not preferred:
            continue
        keep = max(preferred or runs, key=RawRun.sort_key)

        for run in runs:
            if run.path == keep.path:
                continue
            event = event_path(run, raw_root)
            run.path.unlink(missing_ok=True)
            if event is not None:
                event.unlink(missing_ok=True)
            affected.add(run.profile_dir)
            removed += 1
            print(f"REPLACE: {run.path} -> {keep.path}")

    for profile_dir in affected:
        if not profile_dir.exists():
            continue
        clear_derived(profile_dir)
        if not remaining_raw_runs(profile_dir):
            shutil.rmtree(profile_dir)

    remove_empty_parents(raw_root)
    if removed:
        print(f"OK: {removed} ancien(s) run(s) remplacé(s).")


if __name__ == "__main__":
    main()
