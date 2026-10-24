#!/usr/bin/env python3
import argparse
import csv
from collections import Counter
from pathlib import Path
from publish import bucket_state, normalize_algo, policy_for

ROOT = Path.cwd().resolve()
PROFILES = {"256_128", "256_256", "256_64"}


def absolute(path: Path) -> Path:
    return path if path.is_absolute() else (ROOT / path)


def basename(value: str) -> str:
    return str(value or "").replace("\\", "/").rsplit("/", 1)[-1]


def normalize_layout(value: str, file_value: str = "") -> str:
    value = (value or "").strip().lower()
    if value in {"z_curve", "zcurve"}:
        return "zcurve"
    if value:
        return value
    name = basename(file_value).lower()
    if "hilbert" in name:
        return "hilbert"
    if "zcurve" in name or "z_curve" in name:
        return "zcurve"
    return "base"


def row_mode(row: dict[str, str]) -> str:
    value = (row.get("mode") or row.get("profile") or "perf").strip().lower()
    return value


def normalized(value: str) -> str:
    return (value or "").strip()


def row_signature(row: dict[str, str], *, event: bool) -> tuple[str, ...]:
    fields = [
        normalized(row.get("run_id", "")),
        normalized(row.get("swap_profile", "")),
        normalized(row.get("case_id", "")),
        row_mode(row),
        normalized(row.get("timestamp", "")),
        basename(row.get("file", "")),
        normalize_layout(row.get("layout", ""), row.get("file", "")),
        normalize_algo(row.get("algo")),
        normalized(row.get("algo_backend", "")),
        normalized(row.get("graph_storage", "") or "tile_dicho"),
        normalized(row.get("neighbor_access", "") or "tile_segments"),
        policy_for(row),
        bucket_state(row),
        normalized(row.get("heuristic", "") or "standard"),
        normalized(row.get("start_lon", "")),
        normalized(row.get("start_lat", "")),
        normalized(row.get("end_lon", "")),
        normalized(row.get("end_lat", "")),
        normalized(row.get("found", "")),
        normalized(row.get("cost", "")),
        normalized(row.get("path_len", "")),
    ]
    if event:
        fields.extend(
            [
                basename(row.get("events_file", "")),
                normalized(row.get("events_count", "")),
                normalized(row.get("events_version", "")),
                normalized(row.get("inspect_stride", "")),
            ]
        )
    else:
        fields.append(normalized(row.get("search_us", "")))
    return tuple(fields)


def read_rows(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle))


def profile_dirs(raw_root: Path) -> list[Path]:
    return sorted(
        path
        for path in raw_root.rglob("*")
        if path.is_dir() and path.name in PROFILES
    )


def source_event_path(profile_dir: Path, value: str) -> Path | None:
    value = (value or "").strip()
    if not value:
        return None
    candidate = Path(value)
    candidates = []
    if candidate.is_absolute():
        candidates.append(candidate)
    else:
        candidates.extend([profile_dir / candidate.name, ROOT / candidate])
    return next((path for path in candidates if path.is_file()), None)


def physical_public_path(value: str, physical_results: Path, public_results: Path) -> Path:
    value_path = Path(value)
    absolute_value = value_path if value_path.is_absolute() else ROOT / value_path
    public_abs = public_results if public_results.is_absolute() else ROOT / public_results
    try:
        relative = absolute_value.absolute().relative_to(public_abs.absolute())
    except ValueError:
        return absolute_value
    return physical_results / relative


def compare_counters(label: str, expected: Counter, actual: Counter, errors: list[str]) -> None:
    if expected == actual:
        return
    missing = expected - actual
    extra = actual - expected
    errors.append(
        f"{label}: attendu={sum(expected.values())}, obtenu={sum(actual.values())}, "
        f"manquants={sum(missing.values())}, supplémentaires={sum(extra.values())}"
    )
    for signature, count in list(missing.items())[:10]:
        errors.append(f"  MANQUANT x{count}: {signature}")
    for signature, count in list(extra.items())[:10]:
        errors.append(f"  SUPPLÉMENTAIRE x{count}: {signature}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw-root", default="bench/raw")
    parser.add_argument("--results-root", default="bench/results")
    parser.add_argument("--public-results-root", default="bench/results")
    parser.add_argument("--catalog", default="bench/catalog.csv")
    args = parser.parse_args()

    raw_root = absolute(Path(args.raw_root)).resolve()
    results_root = absolute(Path(args.results_root)).resolve()
    public_results_root = absolute(Path(args.public_results_root)).absolute()
    catalog_path = absolute(Path(args.catalog))

    errors: list[str] = []
    expected_stats = Counter()
    expected_events = Counter()
    source_event_by_signature: dict[tuple[str, ...], list[Path]] = {}
    referenced_raw_events: set[Path] = set()

    dirs = profile_dirs(raw_root)
    if not dirs:
        errors.append(f"aucun dossier de profil trouvé sous {raw_root}")

    for directory in dirs:
        relative_dir = directory.resolve().relative_to(raw_root)
        parent_dir = relative_dir.parent
        fallback_run_id = (
            f"raw_{directory.name}"
            if parent_dir == Path(".")
            else parent_dir.name
        )
        enriched = directory / f"all_runs_enriched_{directory.name}.csv"
        if not enriched.is_file():
            errors.append(f"CSV enrichi absent après compilation: {enriched}")
            continue
        for row in read_rows(enriched):
            if not row.get("swap_profile"):
                row["swap_profile"] = directory.name
            if not row.get("run_id"):
                row["run_id"] = fallback_run_id
            expected_stats[row_signature(row, event=False)] += 1
            if row_mode(row) == "inspect":
                signature = row_signature(row, event=True)
                expected_events[signature] += 1
                event_path = source_event_path(directory, row.get("events_file", ""))
                if event_path is None:
                    errors.append(
                        "artefact inspect source introuvable: "
                        f"{directory} / {row.get('events_file', '')}"
                    )
                else:
                    resolved = event_path.resolve()
                    referenced_raw_events.add(resolved)
                    source_event_by_signature.setdefault(signature, []).append(resolved)

    all_raw_events = {path.resolve() for path in raw_root.rglob("*.events.jsonl")}
    for orphan in sorted(all_raw_events - referenced_raw_events):
        errors.append(f"événement brut sans ligne inspect correspondante: {orphan}")

    actual_stats = Counter()
    stats_csvs = sorted((results_root / "stats").rglob("all_runs_enriched_*.csv"))
    for path in stats_csvs:
        for row in read_rows(path):
            actual_stats[row_signature(row, event=False)] += 1

    compare_counters("lignes stats", expected_stats, actual_stats, errors)

    stats_index = results_root / "stats" / "run_index.csv"
    if not stats_index.is_file():
        errors.append(f"index stats absent: {stats_index}")
    else:
        index_rows = read_rows(stats_index)
        if len(index_rows) != sum(actual_stats.values()):
            errors.append(
                f"index stats: {len(index_rows)} lignes pour {sum(actual_stats.values())} lignes stats"
            )

    event_index = results_root / "events" / "index.csv"
    actual_events = Counter()
    indexed_event_files: set[Path] = set()
    seen_event_values: set[str] = set()
    if not event_index.is_file():
        errors.append(f"index events absent: {event_index}")
    else:
        for row in read_rows(event_index):
            signature = row_signature(row, event=True)
            actual_events[signature] += 1
            value = (row.get("events_file") or "").strip()
            if not value:
                errors.append(f"events_file vide dans {event_index}: {signature}")
                continue
            if value in seen_event_values:
                errors.append(f"events_file dupliqué dans l'index: {value}")
            seen_event_values.add(value)
            physical = physical_public_path(
                value,
                physical_results=results_root,
                public_results=public_results_root,
            )
            if not physical.is_file():
                errors.append(f"référence events morte: {value} -> {physical}")
                continue
            indexed_event_files.add(physical.resolve())
            candidates = source_event_by_signature.get(signature, [])
            if candidates and all(source.stat().st_size != physical.stat().st_size for source in candidates):
                errors.append(
                    f"taille events publiée différente de la source pour {value}: "
                    f"publié={physical.stat().st_size}, sources={[p.stat().st_size for p in candidates]}"
                )

    compare_counters("lignes events", expected_events, actual_events, errors)

    published_events = {
        path.resolve()
        for path in (results_root / "events").rglob("*.events.jsonl")
    }
    for orphan in sorted(published_events - indexed_event_files):
        errors.append(f"événement publié non indexé: {orphan}")
    for missing in sorted(indexed_event_files - published_events):
        errors.append(f"événement indexé absent du dossier events: {missing}")

    if not catalog_path.is_file():
        errors.append(f"catalogue absent: {catalog_path}")
        catalog_rows = []
    else:
        catalog_rows = read_rows(catalog_path)
        for row in catalog_rows:
            value = (row.get("csv") or "").strip()
            if not value:
                errors.append(f"csv vide dans le catalogue: {row}")
                continue
            physical = physical_public_path(
                value,
                physical_results=results_root,
                public_results=public_results_root,
            )
            if not physical.is_file():
                errors.append(f"dataset catalogue mort: {value} -> {physical}")

    if expected_stats and not catalog_rows:
        errors.append("aucun dataset dans le catalogue malgré des statistiques présentes")

    print(f"Profils raw analysés : {len(dirs)}")
    print(f"Lignes stats raw     : {sum(expected_stats.values())}")
    print(f"Lignes stats publiées: {sum(actual_stats.values())}")
    print(f"Visualisations raw   : {sum(expected_events.values())}")
    print(f"Visualisations index : {sum(actual_events.values())}")
    print(f"Fichiers events raw  : {len(all_raw_events)}")
    print(f"Fichiers events publiés/indexés : {len(published_events)}/{len(indexed_event_files)}")
    print(f"Datasets catalogue   : {len(catalog_rows)}")

    if errors:
        print("\nÉCHEC validation:")
        for error in errors:
            print(f"- {error}")
        raise SystemExit(1)

    print("OK: results est une reconstruction complète et cohérente de raw.")


if __name__ == "__main__":
    main()
