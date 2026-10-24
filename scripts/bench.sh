#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

RAW_ROOT="${BENCH_RAW_ROOT:-bench/raw}"
RESULTS_ROOT="${BENCH_RESULTS_ROOT:-bench/results}"
CATALOG="${BENCH_CATALOG_PATH:-bench/catalog.csv}"
SCENARIOS="${BENCH_SCENARIOS_PATH:-bench/scenarios.csv}"

usage() {
    cat <<'EOF_USAGE'
Usage:
  scripts/bench.sh run ALGO [options]
      Lance un benchmark et sauvegarde son résultat brut dans bench/raw.
      Sans --profile, lance automatiquement 128 Ko puis 256 Ko.
      perf utilise toujours la latence flash simulée.
      inspect utilise toujours yield, sans latence simulée.

  scripts/bench.sh campaign ALGO|all [options]
      Lance toute la campagne de l'algorithme: 5 trajets x 3 layouts,
      perf + inspect, puis reconstruit bench/results.
      Sans --profile, lance 128 Ko et 256 Ko.
      astar_bucket lance automatiquement les 6 variantes :
      coupled/decoupled × strict/tile_aware/spatial_tiebreak.
      astar_countdown lance les 4 variantes :
      coupled/decoupled × strict/spatial_tiebreak.
      window lance les 2 politiques : spatial_priority/spatial_tiebreak, état coupled.
      Chaque variante compare tile_dicho et tile_index, en deux workers indépendants.
      all lance les algorithmes actifs déclarés dans algorithms.json.
      Pour désactiver un algorithme, passer son champ enabled à false.

  scripts/bench.sh build
  scripts/bench.sh rebuild
      Reconstruit entièrement bench/results et bench/catalog.csv depuis bench/raw.
      Si bench/raw est vide, supprime les résultats dérivés.

  scripts/bench.sh viewer [GPS] [PORT] [LAYOUT]
      Ouvre le viewer sur les résultats reconstruits.

Options de `campaign`:
  --profile 128k|256k
  --inspect-stride N
  --graph-storage both|tile_dicho|tile_index (défaut: both)
  --jobs 1|2 (défaut: 2, parallélisme entre représentations)
EOF_USAGE
}

profile_feature() {
    case "$1" in
        256_128|128|128k) echo "cache-128k" ;;
        256_256|256|256k) echo "cache-256k" ;;
        *) echo "Profil inconnu: $1" >&2; exit 1 ;;
    esac
}

profile_name() {
    case "$1" in
        256_128|128|128k) echo "256_128" ;;
        256_256|256|256k) echo "256_256" ;;
        *) echo "Profil inconnu: $1" >&2; exit 1 ;;
    esac
}

file_for_layout() {
    case "$1" in
        normal) echo "grenoble_large.gps" ;;
        hilbert) echo "grenoble_large_hilbert.gps" ;;
        zcurve) echo "grenoble_large_zcurve.gps" ;;
        *) echo "Layout inconnu: $1" >&2; exit 1 ;;
    esac
}

campaign_variants() {
    python3 "$SCRIPT_DIR/algorithm_registry.py" variants "$1"
}

campaign_progress() {
    local completed="$1" total="$2" started="$3" filled percent
    [ -t 1 ] || [ "$completed" -eq "$total" ] || return 0
    filled=$((total ? completed * 30 / total : 30))
    percent=$((total ? completed * 100 / total : 100))
    local full="██████████████████████████████" empty="░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░"
    if [ -t 1 ]; then printf '\r\033[K'; fi
    printf 'Campagne [%s%s] %3d %% · %d/%d · %d s' \
        "${full:0:filled}" "${empty:filled}" "$percent" "$completed" "$total" "$((SECONDS - started))"
    if [ ! -t 1 ] || [ "$completed" -eq "$total" ]; then printf '\n'; fi
}

build_worker() {
    local profile="$1" mode="$2" latency="$3"
    local feature profile_id target_dir
    feature="$(profile_feature "$profile")" || return $?
    profile_id="$(profile_name "$profile")" || return $?
    target_dir="${BENCH_WORKER_TARGET_ROOT:-$ROOT/target/bench-workers}/${mode}-${profile_id}-${latency}"

    local features="bench-worker,${mode},${feature}"
    if [ "$latency" = "latency" ]; then
        features+=",latency"
    fi

    echo "=== build worker: mode=$mode profile=$profile_id latency=$latency ===" >&2
    CARGO_TARGET_DIR="$target_dir" \
        cargo build --quiet --release --no-default-features --bin bench-bin --features "$features" || return $?
    echo "$target_dir/release/bench-bin"
}

build_results() {
    local raw_abs results_abs catalog_abs staging catalog_tmp stamp backup_results backup_catalog
    raw_abs="$(mkdir -p "$RAW_ROOT" && cd "$RAW_ROOT" && pwd)"
    results_abs="$ROOT/$RESULTS_ROOT"
    catalog_abs="$ROOT/$CATALOG"
    if [[ "$RESULTS_ROOT" = /* ]]; then results_abs="$RESULTS_ROOT"; fi
    if [[ "$CATALOG" = /* ]]; then catalog_abs="$CATALOG"; fi

    if ! find "$raw_abs" -type d \( -name 256_128 -o -name 256_256 \) -print -quit | grep -q .; then
        rm -rf "$results_abs"
        rm -f "$catalog_abs"
        echo "OK: aucune donnée brute, résultats dérivés supprimés."
        return 0
    fi

    stamp="$(date +%Y%m%d_%H%M%S)"
    staging="${results_abs}.build.$$"
    catalog_tmp="${catalog_abs}.build.$$"
    backup_results="${results_abs}.backup.${stamp}"
    backup_catalog="${catalog_abs}.backup.${stamp}"
    rm -rf "$staging" "$catalog_tmp"
    mkdir -p "$staging" "$(dirname "$catalog_abs")"

    echo "=== compile raw ==="
    python3 scripts/recompile_raw.py --raw-root "$raw_abs"

    echo "=== rebuild results from raw ==="
    BENCH_RESULTS_ROOT="$staging" \
    BENCH_RESULTS_PUBLIC_ROOT="$RESULTS_ROOT" \
        python3 scripts/rebuild_publish.py --raw-root "$raw_abs"

    echo "=== catalog + exports ==="
    python3 scripts/export_results.py "$staging/stats"
    BENCH_CATALOG_PATH="$catalog_tmp" \
    BENCH_CATALOG_PUBLIC_STATS_ROOT="$RESULTS_ROOT/stats" \
        python3 scripts/catalog.py "$staging/stats"

    echo "=== validate derived results ==="
    python3 scripts/validate_results.py \
        --raw-root "$raw_abs" \
        --results-root "$staging" \
        --public-results-root "$RESULTS_ROOT" \
        --catalog "$catalog_tmp"

    if [ -e "$results_abs" ]; then mv "$results_abs" "$backup_results"; fi
    if [ -e "$catalog_abs" ]; then mv "$catalog_abs" "$backup_catalog"; fi

    mv "$staging" "$results_abs"
    mv "$catalog_tmp" "$catalog_abs"

    if ! python3 scripts/validate_results.py \
        --raw-root "$raw_abs" \
        --results-root "$results_abs" \
        --public-results-root "$RESULTS_ROOT" \
        --catalog "$catalog_abs"; then
        echo "Erreur: validation finale échouée, restauration." >&2
        rm -rf "$results_abs" "$catalog_abs"
        [ -e "$backup_results" ] && mv "$backup_results" "$results_abs"
        [ -e "$backup_catalog" ] && mv "$backup_catalog" "$catalog_abs"
        exit 1
    fi

    rm -rf "$backup_results" "$backup_catalog"
    echo "OK: $RESULTS_ROOT reconstruit depuis $RAW_ROOT"
}

replace_previous_runs() {
    local run_id="$1"
    python3 scripts/deduplicate_raw.py --raw-root "$RAW_ROOT" --prefer-run-id "$run_id"
}

run_campaign() {
    [ "$#" -gt 0 ] || { echo "ALGO manquant" >&2; usage; exit 1; }
    local requested_algo="$1"
    shift

    local profiles="256_128 256_256"
    local stride="${BENCH_INSPECT_STRIDE:-100}"
    local graph_storages="tile_dicho tile_index" jobs="${BENCH_JOBS:-2}"

    while [ "$#" -gt 0 ]; do
        case "$1" in
            --profile|--ram)
                shift
                profiles="$(profile_name "${1:?profil manquant}")" || return $?
                ;;
            --inspect-stride|--inspect-stride)
                shift
                stride="${1:?stride manquant}"
                ;;
            --graph-storage|--graph)
                shift
                case "${1:?représentation manquante}" in
                    both) graph_storages="tile_dicho tile_index" ;;
                    tile_dicho) graph_storages="tile_dicho" ;;
                    tile_index) graph_storages="tile_index" ;;
                    *) echo "Représentation attendue: both | tile_dicho | tile_index" >&2; return 1 ;;
                esac
                ;;
            --jobs) shift; jobs="${1:?nombre de workers manquant}" ;;
            --help|-h)
                usage
                exit 0
                ;;
            *)
                echo "Option inconnue pour campaign: $1" >&2
                usage
                exit 1
                ;;
        esac
        shift
    done

    [ -f "$SCENARIOS" ] || { echo "Scénarios introuvables: $SCENARIOS" >&2; exit 1; }
    [[ "$jobs" = 1 || "$jobs" = 2 ]] || { echo "--jobs attendu: 1 | 2" >&2; return 1; }
    [[ "$stride" =~ ^[1-9][0-9]*$ ]] || { echo "Stride invalide: $stride" >&2; exit 1; }

    local variants
    variants="$(campaign_variants "$requested_algo")" || return $?

    local run_id="${BENCH_RUN_ID:-campaign_$(date +%Y%m%d_%H%M%S)_$$}"
    run_id="${run_id//[^A-Za-z0-9_.-]/_}"
    local run_root="$RAW_ROOT/$run_id"
    local worker_log="$run_root/workers.log"
    mkdir -p "$run_root"

    echo "=== campaign $run_id ==="
    echo "algo=$requested_algo profiles=[$profiles] stride=$stride"
    echo "modes: perf=latency, inspect=yield; graph=[$graph_storages] jobs=$jobs"

    local -a profile_list graph_list variant_list
    read -r -a profile_list <<< "$profiles"
    read -r -a graph_list <<< "$graph_storages"
    mapfile -t variant_list <<< "$variants"
    local scenario_count total completed=0 started
    scenario_count="$(awk -F, '{sub(/\r$/, "", $1); if ($1 != "" && $1 != "name") n++} END {print n+0}' "$SCENARIOS")"
    total=$((${#profile_list[@]} * ${#graph_list[@]} * ${#variant_list[@]} * scenario_count * 3 * 2))

    local profile profile_id out_dir name start_lon start_lat end_lon end_lat
    local layout file mode binary algo policy state_mode graph_storage pid batch_status
    local -a worker_pids=()
    local -A binaries=()
    for profile in $profiles; do
        binaries["$profile,perf"]="$(build_worker "$profile" perf latency)" || return $?
        binaries["$profile,inspect"]="$(build_worker "$profile" inspect yield)" || return $?
    done
    started=$SECONDS
    campaign_progress "$completed" "$total" "$started"

    for profile in $profiles; do
        profile_id="$(profile_name "$profile")" || return $?
        out_dir="$run_root/$profile_id"
        mkdir -p "$out_dir"

        while IFS=, read -r name start_lon start_lat end_lon end_lat || [ -n "${name:-}" ]; do
            name="${name//$'\r'/}"
            [ -z "$name" ] && continue
            [ "$name" = "name" ] && continue

            for layout in normal hilbert zcurve; do
                file="$(file_for_layout "$layout")" || return $?
                for mode in perf inspect; do
                    while read -r algo policy state_mode; do
                        [ -n "$algo" ] || continue
                        binary="${binaries["$profile_id,$mode"]}"
                        worker_pids=()
                        batch_status=0
                        for graph_storage in $graph_storages; do
                            echo "=== $profile_id $mode $name $layout $algo/$policy/$state_mode $graph_storage ===" >> "$worker_log"
                            BENCH_RUN_ID="$run_id" BENCH_GRAPH_STORAGE="$graph_storage" BENCH_NEIGHBOR_ACCESS=tile_segments \
                            BENCH_EXPLORATION_POLICY="$policy" BENCH_BUCKET_STATE="${state_mode/-/}" \
                                "$binary" --worker --algo "$algo" --layout "$layout" --file "$file" \
                                --case "$name" --from "$start_lon" "$start_lat" --to "$end_lon" "$end_lat" \
                                --inspect-stride "$stride" --save-dir "$out_dir" --saved-output >> "$worker_log" 2>&1 &
                            worker_pids+=("$!")
                            if [ "$jobs" = 1 ]; then
                                if wait "${worker_pids[0]}"; then
                                    completed=$((completed + 1))
                                    campaign_progress "$completed" "$total" "$started"
                                else batch_status=$?; fi
                                worker_pids=()
                                [ "$batch_status" = 0 ] || break
                            fi
                        done
                        # Toujours attendre les deux workers, même si l'un échoue.
                        for pid in "${worker_pids[@]}"; do
                            if wait "$pid"; then
                                completed=$((completed + 1))
                                campaign_progress "$completed" "$total" "$started"
                            else batch_status=$?; fi
                        done
                        if [ "$batch_status" != 0 ]; then
                            printf '\nErreur de benchmark, journal : %s\n' "$worker_log" >&2
                            tail -n 40 "$worker_log" >&2
                            return "$batch_status"
                        fi
                    done <<< "$variants"
                done
            done
        done < "$SCENARIOS"
    done

    replace_previous_runs "$run_id"
    echo "OK: raw=$run_root"
    build_results
}

cmd="${1:-help}"
[ "$#" -gt 0 ] && shift || true
case "$cmd" in
    run)
        [ "$#" -gt 0 ] || { echo "ALGO manquant" >&2; usage; exit 1; }
        run_id="${BENCH_RUN_ID:-single_$(date +%Y%m%d_%H%M%S)_$$}"
        run_id="${run_id//[^A-Za-z0-9_.-]/_}"
        BENCH_RUN_ID="$run_id" cargo run --release --bin bench-bin -- "$@" --save
        replace_previous_runs "$run_id"
        ;;
    campaign)
        run_campaign "$@"
        ;;
    build|rebuild)
        [ "$#" -eq 0 ] || { echo "$cmd ne prend pas d'argument" >&2; exit 1; }
        build_results
        ;;
    viewer)
        cargo run --release --bin viewer-bin -- "$@"
        ;;
    help|--help|-h)
        usage
        ;;
    *)
        echo "Commande inconnue: $cmd" >&2
        usage
        exit 1
        ;;
esac
