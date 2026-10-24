#!/usr/bin/env python3
import argparse
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path.cwd()
PROFILES = {"256_64", "256_128", "256_256"}
GENERATED_NAMES = {
    "all_runs.csv",
    "all_runs_enriched.csv",
    "all_runs_latest.csv",
    "summary_by_algo.csv",
    "summary_by_case.csv",
    "summary_by_run.csv",
    "publish_manifest.json",
}


def profile_dirs(raw_root: Path) -> list[Path]:
    return sorted(
        path
        for path in raw_root.rglob("*")
        if path.is_dir() and path.name in PROFILES
    )


def is_generated(path: Path) -> bool:
    name = path.name
    return (
        name in GENERATED_NAMES
        or name.startswith("all_runs_enriched_")
        or name.startswith("summary_")
        or name.startswith("compare_")
        or name.endswith((".json", ".jsonl", ".md", ".png", ".html"))
    )


def has_raw_files(directory: Path) -> bool:
    return any(path.is_file() and not is_generated(path) for path in directory.iterdir())


def run_script(script: str, directory: Path, env: dict[str, str]) -> str:
    result = subprocess.run(
        [sys.executable, script],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if result.returncode != 0:
        raise RuntimeError(f"{script} a échoué pour {directory}:\n{result.stdout}")
    return result.stdout


def compile_directory(directory: Path) -> tuple[Path, str]:
    env = os.environ.copy()
    env["BENCH_OUTPUT_DIR"] = str(directory.resolve())
    log = [f"=== {directory} ==="]

    if has_raw_files(directory):
        log.append(run_script("scripts/merge.py", directory, env).rstrip())
    elif (directory / "all_runs.csv").is_file():
        log.append("Aucun fichier brut détecté, réutilisation de all_runs.csv")
    else:
        raise RuntimeError(f"aucun fichier brut ni all_runs.csv dans {directory}")

    log.append(run_script("scripts/check.py", directory, env).rstrip())
    log.append(run_script("scripts/compile.py", directory, env).rstrip())

    expected = directory / f"all_runs_enriched_{directory.name}.csv"
    if not expected.is_file():
        raise RuntimeError(f"fichier enrichi attendu absent: {expected}")

    return directory, "\n".join(part for part in log if part)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw-root", default="bench/raw")
    parser.add_argument(
        "--jobs",
        type=int,
        default=max(1, int(os.environ.get("BENCH_REBUILD_JOBS", "2"))),
    )
    parser.add_argument("--offset", type=int, default=0)
    parser.add_argument("--limit", type=int, default=0)
    args = parser.parse_args()

    raw_root = Path(args.raw_root)
    if not raw_root.is_absolute():
        raw_root = ROOT / raw_root
    raw_root = raw_root.resolve()
    all_directories = profile_dirs(raw_root)
    directories = all_directories[max(0, args.offset):]
    if args.limit > 0:
        directories = directories[:args.limit]
    if not directories:
        raise SystemExit(f"Aucun dossier de profil sous {raw_root}")

    jobs = max(1, min(args.jobs, len(directories)))
    print(
        f"Recompilation de {len(directories)} dossier(s), jobs={jobs}, "
        f"offset={args.offset}, total={len(all_directories)}",
        flush=True,
    )
    failures = []
    completed = 0

    with ThreadPoolExecutor(max_workers=jobs) as executor:
        futures = {executor.submit(compile_directory, directory): directory for directory in directories}
        for future in as_completed(futures):
            directory = futures[future]
            try:
                _, log = future.result()
            except Exception as error:
                failures.append((directory, str(error)))
                print(f"ERREUR: {directory}\n{error}", file=sys.stderr)
                continue
            completed += 1
            print(f"OK [{completed}/{len(directories)}] {directory}", flush=True)
            if os.environ.get("BENCH_REBUILD_VERBOSE") == "1":
                print(log)

    if failures:
        print(f"ÉCHEC: {len(failures)} dossier(s) non recompilé(s).", file=sys.stderr)
        raise SystemExit(1)

    print(f"OK: {completed} dossier(s) raw entièrement recompilés.", flush=True)


if __name__ == "__main__":
    main()
