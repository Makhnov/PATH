#!/usr/bin/env python3
import csv
import os
from collections import defaultdict
from pathlib import Path
from publish import bucket_state
from algorithm_registry import ALGORITHMS, normalize_algo, validate_options

output_dir = Path(os.environ.get("BENCH_OUTPUT_DIR", "output"))
path = output_dir / "all_runs.csv"
rows = list(csv.DictReader(path.open(encoding="utf-8")))

assert rows, "CSV vide"

VALID_MODES = {"perf", "inspect"}
VALID_ALGOS = {entry["id"] for entry in ALGORITHMS}
EXPERIMENTAL_ALGOS = {entry["id"] for entry in ALGORITHMS if entry["counter_validation"] == "functional"}
CORE_FIELDS = ["found", "cost", "path_len"]


def as_int(row, key):
    return int(float(row.get(key, "0") or 0))


def row_mode(row):
    return (row.get("mode") or "perf").strip().lower()


def row_config(row):
    return (
        row.get("graph_storage", "tile_dicho"),
        row.get("neighbor_access", "tile_segments"),
        row.get("exploration_policy", "strict"),
        bucket_state(row),
        row.get("heuristic", "standard"),
    )


by_scenario_mode = defaultdict(dict)
by_scenario_algo = defaultdict(dict)
functional_rows = []

for row in rows:
    case_id = row.get("case_id", "")
    file = row.get("file", "")
    mode = row_mode(row)
    algo = normalize_algo(row.get("algo", ""))

    assert case_id, f"case_id manquant: {row}"
    assert file, f"file manquant pour {case_id} {mode} {algo}"
    assert mode in VALID_MODES, f"mode invalide: {mode} ({case_id} {file} {algo})"
    assert algo in VALID_ALGOS, f"algo invalide: {algo} ({case_id} {file} {mode})"
    validate_options(row)
    assert row.get("found") in {"true", "1"}, f"chemin non trouvé: {case_id} {file} {mode} {algo}"

    if mode == "inspect" and algo not in EXPERIMENTAL_ALGOS:
        heap_pushes = as_int(row, "heap_pushes")
        heap_pops = as_int(row, "heap_pops")
        nodes_skipped = as_int(row, "nodes_skipped")
        nodes_visited = as_int(row, "nodes_visited")
        neighbors_skipped = as_int(row, "neighbors_skipped")
        neighbors_visited = as_int(row, "neighbors_visited")
        edges_visited = as_int(row, "edges_visited")

        assert heap_pops == nodes_visited + nodes_skipped, (
            f"heap_pops != nodes_visited + nodes_skipped: {case_id} {file} {algo}"
        )
        assert neighbors_visited + neighbors_skipped == edges_visited, (
            f"neighbors_visited + neighbors_skipped != edges_visited: {case_id} {file} {algo}"
        )
        assert neighbors_visited + 1 == heap_pushes, (
            f"neighbors_visited + 1 != heap_pushes: {case_id} {file} {algo}"
        )

        if validate_options(row)["counter_validation"] == "countdown":
            assert as_int(row, "countdown_scans") == nodes_visited
            if bucket_state(row) == "decoupled":
                assert as_int(row, "countdown_done_checks") == edges_visited
                assert as_int(row, "countdown_done_inserts") <= nodes_visited
                assert as_int(row, "countdown_done_decs") + as_int(row, "countdown_done_removes") <= neighbors_skipped
            else:
                assert as_int(row, "countdown_done_checks") == heap_pops + edges_visited
                assert as_int(row, "countdown_done_inserts") == nodes_visited
                assert as_int(row, "countdown_done_decs") + as_int(row, "countdown_done_removes") == (
                    nodes_skipped + neighbors_skipped
                )

    config = row_config(row)
    by_scenario_mode[(case_id, file, *config, mode)][algo] = row
    by_scenario_algo[(case_id, file, *config, algo)][mode] = row
    functional_rows.append((row, algo, mode))

# Vérification différentielle : chaque variante de recherche doit concorder avec A*
# pour les mêmes scénario et représentation des données. La politique et l'état sont
# volontairement exclus de cette clé : ce sont les dimensions testées, pas les conditions de référence.
def functional_key(row, mode):
    return (
        row.get("case_id", ""),
        row.get("file", ""),
        row.get("swap_profile", ""),
        row.get("layout", ""),
        row.get("graph_storage", "tile_dicho"),
        row.get("neighbor_access", "tile_segments"),
        row.get("heuristic", "standard"),
        mode,
    )

baselines = {
    functional_key(row, mode): row
    for row, algo, mode in functional_rows
    if algo == "astar_lazy"
}
for row, algo, mode in functional_rows:
    if algo == "astar_lazy":
        continue
    baseline = baselines.get(functional_key(row, mode))
    if baseline is None:
        continue
    assert row.get("found") == baseline.get("found"), (
        f"found différent de A* pour {algo} {row.get('case_id')} {row.get('file')} {mode}"
    )
    assert row.get("cost") == baseline.get("cost"), (
        f"coût différent de A* pour {algo} {row.get('case_id')} {row.get('file')} {mode}: "
        f"{algo}={row.get('cost')} A*={baseline.get('cost')}"
    )

# Perf et inspect doivent décrire exactement le même résultat algorithmique.
for key, modes in sorted(by_scenario_algo.items()):
    if not {"perf", "inspect"}.issubset(modes):
        continue
    perf = modes["perf"]
    inspect = modes["inspect"]
    for field in CORE_FIELDS:
        assert perf.get(field) == inspect.get(field), (
            f"{field} différent entre perf/inspect pour {key}: "
            f"perf={perf.get(field)} inspect={inspect.get(field)}"
        )

scenario_count = len({(r.get("case_id", ""), r.get("file", ""), *row_config(r)) for r in rows})
print(
    f"OK: {len(rows)} runs validés, {scenario_count} scénarios/configs, "
    f"modes={sorted({row_mode(r) for r in rows})}, "
    f"algos={sorted({normalize_algo(r.get('algo', '')) for r in rows})}."
)
