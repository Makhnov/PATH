"""Catalogue partagé des algorithmes pour les campagnes, la validation et le traitement des résultats.

Seuls algorithms.json et un adaptateur Rust sont nécessaires pour enregistrer un algorithme.
Les descripteurs désactivés restent connus lors de la lecture des résultats historiques.
"""
import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def load_registry(path=None):
    catalog = json.loads(Path(path or ROOT / "algorithms.json").read_text(encoding="utf-8"))
    if catalog.get("schema_version") != 1 or not catalog.get("algorithms"):
        raise ValueError("algorithms.json: schema/version invalide")
    names = set()
    for entry in catalog["algorithms"]:
        if not isinstance(entry.get("enabled"), bool):
            raise ValueError("enabled doit être booléen")
        identifiers = {entry["id"], entry["command"]}
        if names.intersection(identifiers):
            raise ValueError("identifiant dupliqué dans algorithms.json")
        names.update(identifiers)
        if not entry["policies"] or len(set(entry["policies"])) != len(entry["policies"]):
            raise ValueError("politiques vides ou dupliquées")
        if len(set(entry["states"])) != len(entry["states"]):
            raise ValueError("états dupliqués")
    return catalog["algorithms"]


ALGORITHMS = load_registry()


def descriptor(value):
    value = str(value or "").strip().lower()
    return next((entry for entry in ALGORITHMS if value in {entry["id"], entry["command"]}), None)


def normalize_algo(value):
    entry = descriptor(value)
    return entry["id"] if entry else str(value or "").strip().lower()


def state_for(row):
    entry = descriptor(row.get("algo"))
    if not entry or not (entry["states"] or entry.get("historical_states")):
        return ""
    default = entry["states"][0] if entry["states"] else ""
    return str(row.get("bucket_state") or default).strip().lower()


def policy_for(row):
    entry = descriptor(row.get("algo"))
    return str(row.get("exploration_policy") or (entry["policies"][0] if entry else "strict")).strip().lower()


def validate_options(row):
    entry = descriptor(row.get("algo"))
    if entry is None:
        raise ValueError(f"algorithme inconnu: {row.get('algo')}")
    policies = {*entry["policies"], *entry.get("historical_policies", [])}
    if policy_for(row) not in policies:
        raise ValueError(f"politique incompatible: {row.get('algo')}/{row.get('exploration_policy')}")
    state = state_for(row)
    states = {*entry["states"], *entry.get("historical_states", [])}
    if state and state not in states:
        raise ValueError(f"état incompatible: {row.get('algo')}/{state}")
    return entry


def variants(requested):
    if requested == "all":
        entries = [entry for entry in ALGORITHMS if entry["enabled"]]
    else:
        entry = descriptor(requested)
        if not entry or not entry["enabled"]:
            raise ValueError(f"algorithme inconnu ou désactivé: {requested}")
        entries = [entry]
    return [(entry["command"], policy, state) for entry in entries
            for state in entry["states"] or ["-"] for policy in entry["policies"]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("list", "variants"))
    parser.add_argument("algorithm", nargs="?", default="all")
    args = parser.parse_args()
    if args.action == "list":
        print(json.dumps(ALGORITHMS, ensure_ascii=False, indent=2))
    elif args.action == "variants":
        for variant in variants(args.algorithm):
            print(" ".join(variant))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError) as error:
        raise SystemExit(str(error))
