#!/usr/bin/env python3
"""Créer un adaptateur d'algorithme désactivé et son entrée dans le registre, sans écraser de fichiers."""
import argparse
import copy
import json
from pathlib import Path
import re

from algorithm_registry import ROOT, load_registry

TEMPLATE = '''// Adaptateur du pipeline pour {name}. Activer son entrée dans algorithms.json après implémentation.
use crate::{{
    algo::registry::{{AlgorithmRun, RunOptions}},
    event::GpsEvent,
    format::Node,
    graph::{{GNode, Graph}},
    inspection::{{Bench, InspectGraph}},
}};
use swap_cache::cache::CacheSingleton;

pub async fn run<G, C, F, B>(
    _graph: &G, _start: Node, _end: Node, _options: RunOptions<'_>,
    _emit: F, _bench: &mut B,
) -> AlgorithmRun<C>
where
    G: Graph<ULoc = GNode, Loc = Node> + InspectGraph,
    C: CacheSingleton,
    F: FnMut(GpsEvent<GNode>),
    B: Bench,
{{
    // Renvoyer AlgorithmRun::new(your_search(...).await), ou joindre les métriques
    // déclarées avec SpecificMetrics::from_values(...). Réutiliser Bench pour les
    // compteurs et événements standards. Ce squelette reste désactivé tant qu'il n'est pas prêt.
    todo!("implement {name}")
}}
'''


def scaffold(name, label=None, short_label=None, root=ROOT):
    if not re.fullmatch(r"[a-z][a-z0-9]*(?:_[a-z0-9]+)*", name):
        raise ValueError("nom attendu : identifiant snake_case en minuscules")
    root = Path(root)
    path = root / "algorithms.json"
    catalog = json.loads(path.read_text(encoding="utf-8"))
    entries = load_registry(path)
    if any(name in {entry["id"], entry["command"]} for entry in entries):
        raise ValueError("cet identifiant existe déjà")
    adapter = root / "src/algo/adapters" / f"{name}.rs"
    if adapter.exists():
        raise ValueError("adaptateur déjà présent : aucun fichier écrasé")
    entry = copy.deepcopy(entries[0])
    entry.update(id=name, command=name, label=label or name,
                 short_label=short_label or name, enabled=False,
                 adapter=adapter.relative_to(root).as_posix(), policies=["strict"], states=[],
                 counter_validation="functional",
                 specific_metrics=[], collections=[], structure_note="", ui_model="standard", uses_heuristic=True)
    entry.pop("parameters", None)
    entry.pop("metric_labels", None)
    catalog["algorithms"].append(entry)
    adapter.parent.mkdir(parents=True, exist_ok=True)
    with adapter.open("x", encoding="utf-8") as stream:
        stream.write(TEMPLATE.format(name=name))
    try:
        path.write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    except OSError:
        adapter.unlink()
        raise
    return adapter


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("name")
    parser.add_argument("--label")
    parser.add_argument("--short-label")
    args = parser.parse_args()
    try:
        adapter = scaffold(args.name, args.label, args.short_label)
    except (ValueError, OSError) as error:
        parser.error(str(error))
    print(f"Créé : {adapter.relative_to(ROOT)} + entrée désactivée dans algorithms.json")
    print("Implémenter run(), tester, puis passer enabled à true. Voir docs/algorithms.md.")


if __name__ == "__main__":
    main()
