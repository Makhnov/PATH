import csv
import os
from pathlib import Path

output_dir = Path(os.environ.get("BENCH_OUTPUT_DIR", "output"))
merged = output_dir / "all_runs.csv"

skip_exact = {
    "all_runs.csv",
    "all_runs_enriched.csv",
    "all_runs_latest.csv",
    "summary_by_algo.csv",
    "summary_by_case.csv",
    "summary_by_run.csv",
}

rows = []
fieldnames = []


def is_generated_file(path: Path) -> bool:
    name = path.name
    return (
        name in skip_exact
        or name.startswith("all_runs_enriched_")
        or name.startswith("summary_")
        or name.startswith("compare_")
        or name.endswith(".png")
        or name.endswith(".html")
        or name.endswith(".json")
        or name.endswith(".jsonl")
    )


def add_field(name: str) -> None:
    if name not in fieldnames:
        fieldnames.append(name)


assert output_dir.exists(), f"Dossier de sortie introuvable: {output_dir}"

for path in sorted(output_dir.iterdir()):
    if not path.is_file() or is_generated_file(path):
        continue

    text = path.read_text(encoding="utf-8").strip()
    if not text:
        continue

    lines = text.splitlines()
    if len(lines) < 2:
        continue

    reader = csv.DictReader(lines)
    if not reader.fieldnames:
        continue

    if "case_id" not in reader.fieldnames:
        continue

    for name in reader.fieldnames:
        add_field(name)

    for row in reader:
        if not row or row.get("case_id") == "case_id":
            continue
        rows.append(row)

assert rows, f"Aucune ligne de bench trouvée dans {output_dir}"

with merged.open("w", encoding="utf-8", newline="") as out:
    writer = csv.DictWriter(out, fieldnames=fieldnames)
    writer.writeheader()
    for row in rows:
        writer.writerow({name: row.get(name, "") for name in fieldnames})

print(f"OK: écrit {merged} ({len(rows)} lignes)")
