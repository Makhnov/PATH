#![cfg_attr(feature = "bench-worker", feature(impl_trait_in_assoc_type))]
#![recursion_limit = "256"]

#[cfg(feature = "bench-worker")]
mod worker;

#[cfg(not(feature = "bench-worker"))]
mod cli {
    use chrono::Local;
    use lib_gps::graph::tile_index::GraphStorage;
    use std::env;
    use std::fs;
    use std::io::{Error, ErrorKind, Result};
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitStatus, Stdio};

    const DEFAULT_SCENARIOS: &str = "bench/scenarios.csv";
    const DEFAULT_CASE: &str = "case_001";

    use lib_gps::algo::registry::{
        Algorithm, DELTA_SWEEP, RunOptions, WINDOW_SWEEP, validate_capacity,
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Selection<T> {
        One(T),
        All,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct WindowTuning {
        window: usize,
        delta: u32,
        batch: u32,
    }
    #[derive(Clone, Copy)]
    enum Mode {
        Perf,
        Inspect,
    }

    impl Mode {
        fn parse(value: &str) -> Result<Self> {
            match value.trim().to_ascii_lowercase().as_str() {
                "perf" | "performance" => Ok(Self::Perf),
                "inspect" | "instrument" => Ok(Self::Inspect),
                _ => Err(invalid(format!(
                    "mode inconnu: {value} (attendu: perf | inspect)"
                ))),
            }
        }

        fn feature(self) -> &'static str {
            match self {
                Self::Perf => "perf",
                Self::Inspect => "inspect",
            }
        }

        fn latency(self) -> bool {
            matches!(self, Self::Perf)
        }
    }

    #[derive(Clone, Copy)]
    enum Profile {
        Ram128,
        Ram256,
    }

    impl Profile {
        fn parse(value: &str) -> Result<Self> {
            match value.trim().to_ascii_lowercase().as_str() {
                "128" | "128k" | "128ko" | "256_128" | "cache-128k" => Ok(Self::Ram128),
                "256" | "256k" | "256ko" | "256_256" | "cache-256k" => Ok(Self::Ram256),
                _ => Err(invalid(format!(
                    "profil inconnu: {value} (attendu: 128k | 256k)"
                ))),
            }
        }

        fn feature(self) -> &'static str {
            match self {
                Self::Ram128 => "cache-128k",
                Self::Ram256 => "cache-256k",
            }
        }

        fn name(self) -> &'static str {
            match self {
                Self::Ram128 => "256_128",
                Self::Ram256 => "256_256",
            }
        }
    }

    #[derive(Clone, Copy)]
    enum Layout {
        Base,
        Hilbert,
        Zcurve,
    }

    impl Layout {
        fn parse(value: &str) -> Result<Self> {
            match value.trim().to_ascii_lowercase().as_str() {
                "base" | "normal" | "row" | "row-major" => Ok(Self::Base),
                "hilbert" => Ok(Self::Hilbert),
                "zcurve" | "z-curve" | "z_curve" => Ok(Self::Zcurve),
                _ => Err(invalid(format!(
                    "layout inconnu: {value} (attendu: normal | hilbert | zcurve)"
                ))),
            }
        }

        fn name(self) -> &'static str {
            match self {
                Self::Base => "normal",
                Self::Hilbert => "hilbert",
                Self::Zcurve => "zcurve",
            }
        }

        fn gps_filename(self) -> &'static str {
            match self {
                Self::Base => "grenoble_large.gps",
                Self::Hilbert => "grenoble_large_hilbert.gps",
                Self::Zcurve => "grenoble_large_zcurve.gps",
            }
        }
    }

    #[derive(Clone)]
    struct Scenario {
        name: String,
        file: String,
        start_lon: f64,
        start_lat: f64,
        end_lon: f64,
        end_lat: f64,
    }

    struct Request {
        algorithm: Algorithm,
        mode: Mode,
        policy: String,
        profile: Option<Profile>,
        layout: Layout,
        case_id: String,
        file: Option<String>,
        start: Option<(f64, f64)>,
        end: Option<(f64, f64)>,
        scenarios: PathBuf,
        save: bool,
        inspect_stride: u64,
        bucket_state: String,
        graph_storage: GraphStorage,
        window: Option<Selection<usize>>,
        delta: Option<Selection<u32>>,
        batch: Option<u32>,
    }

    impl Request {
        fn parse() -> Result<Self> {
            Self::parse_args(env::args().skip(1))
        }

        fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Self> {
            let Some(first) = args.next() else {
                print_help();
                return Err(invalid("algorithme manquant"));
            };

            if matches!(first.as_str(), "--help" | "-h" | "help") {
                print_help();
                std::process::exit(0);
            }

            if first == "--list-algorithms" {
                println!("{}", lib_gps::algo::registry::CATALOG_JSON);
                std::process::exit(0);
            }
            let algorithm = Algorithm::parse(&first)
                .ok_or_else(|| invalid(format!("algorithme inconnu ou désactivé: {first}")))?;
            let mut request = Self {
                algorithm,
                mode: Mode::Perf,
                policy: algorithm.descriptor().policy(None).to_string(),
                profile: None,
                layout: Layout::Base,
                case_id: DEFAULT_CASE.to_string(),
                file: None,
                start: None,
                end: None,
                scenarios: PathBuf::from(DEFAULT_SCENARIOS),
                save: false,
                inspect_stride: 100,
                bucket_state: algorithm.descriptor().state(None).to_string(),
                graph_storage: GraphStorage::parse(
                    &env::var("BENCH_GRAPH_STORAGE").unwrap_or_else(|_| "tile_dicho".into()),
                )
                .ok_or_else(|| invalid("BENCH_GRAPH_STORAGE attendu: tile_dicho | tile_index"))?,
                window: None,
                delta: None,
                batch: None,
            };

            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--graph-storage" | "--graph" => {
                        request.graph_storage = GraphStorage::parse(&next(&mut args, &arg)?)
                            .ok_or_else(|| {
                                invalid("--graph-storage attendu: tile_dicho | tile_index")
                            })?;
                    }
                    "--neighbor-access" => {
                        if next(&mut args, &arg)? != "tile_segments" {
                            return Err(invalid("--neighbor-access attendu: tile_segments"));
                        }
                    }
                    "--mode" => request.mode = Mode::parse(&next(&mut args, "--mode")?)?,
                    "--policy" => {
                        request.policy = next(&mut args, "--policy")?
                            .trim()
                            .to_lowercase()
                            .replace('-', "_");
                        if !request
                            .algorithm
                            .descriptor()
                            .policies
                            .contains(&request.policy.as_str())
                            && request.policy == "spatial"
                        {
                            request.policy = "spatial_tiebreak".into();
                        }
                        if !request
                            .algorithm
                            .descriptor()
                            .policies
                            .contains(&request.policy.as_str())
                            && request.policy == "tileaware"
                        {
                            request.policy = "tile_aware".into();
                        }
                    }
                    "--state" => {
                        request.bucket_state = next(&mut args, &arg)?.trim().to_lowercase();
                        if !request
                            .algorithm
                            .descriptor()
                            .states
                            .contains(&request.bucket_state.as_str())
                        {
                            request.bucket_state = match request.bucket_state.as_str() {
                                "couple" | "couplé" => "coupled".into(),
                                "decouple" | "découplé" => "decoupled".into(),
                                _ => request.bucket_state,
                            };
                        }
                    }
                    "--profile" | "--ram" => {
                        request.profile = Some(Profile::parse(&next(&mut args, &arg)?)?)
                    }
                    "--layout" => request.layout = Layout::parse(&next(&mut args, "--layout")?)?,
                    "--case" => request.case_id = next(&mut args, "--case")?,
                    "--file" => request.file = Some(next(&mut args, "--file")?),
                    "--from" => request.start = Some(read_point(&mut args, "--from")?),
                    "--to" => request.end = Some(read_point(&mut args, "--to")?),
                    "--save" => request.save = true,
                    "--window" => {
                        let value = next(&mut args, &arg)?;
                        request.window = Some(if value.eq_ignore_ascii_case("all") {
                            Selection::All
                        } else {
                            let window = value.parse::<usize>().map_err(|_| {
                                invalid("--window attendu: un entier positif | all")
                            })?;
                            validate_capacity(window).map_err(invalid)?;
                            Selection::One(window)
                        });
                    }
                    "--delta" => {
                        let value = next(&mut args, &arg)?;
                        request.delta = Some(if value.eq_ignore_ascii_case("all") {
                            Selection::All
                        } else {
                            Selection::One(value.parse::<u32>().map_err(|_| {
                                invalid("--delta attendu: un entier positif ou nul | all")
                            })?)
                        });
                    }
                    "--batch" | "--max-batch" => {
                        let batch = next(&mut args, &arg)?.parse::<u32>().map_err(|_| {
                            invalid("--batch attendu: un entier strictement positif")
                        })?;
                        if batch == 0 {
                            return Err(invalid("--batch doit être supérieur à zéro"));
                        }
                        request.batch = Some(batch);
                    }
                    "--inspect-stride" => {
                        request.inspect_stride = next(&mut args, &arg)?
                            .parse::<u64>()
                            .map_err(|_| invalid(format!("{arg} doit être un entier positif")))?
                            .max(1)
                    }
                    "--help" | "-h" => {
                        print_help();
                        std::process::exit(0);
                    }
                    value if !value.starts_with('-') && request.case_id == DEFAULT_CASE => {
                        request.case_id = value.to_string();
                    }
                    _ => return Err(invalid(format!("argument inconnu: {arg}"))),
                }
            }

            if request.start.is_some() != request.end.is_some() {
                return Err(invalid("--from et --to doivent être fournis ensemble"));
            }
            request
                .algorithm
                .descriptor()
                .validate(RunOptions {
                    policy: &request.policy,
                    state: &request.bucket_state,
                    parameters: &[],
                })
                .map_err(invalid)?;

            if request.tuning_requested() {
                if request.algorithm.descriptor().id != "astar_window" {
                    return Err(invalid(
                        "--window, --delta et --batch sont réservés à A* Window",
                    ));
                }
                if request.save {
                    return Err(invalid(
                        "--window, --delta et --batch lancent un essai sans sauvegarde : retirez --save",
                    ));
                }
                if request.policy == "spatial_tiebreak"
                    && request
                        .tuning_values()
                        .iter()
                        .any(|tuning| tuning.delta > 0)
                {
                    return Err(invalid(
                        "--delta > 0 et --delta all nécessitent --policy spatial_priority ; le départage spatial utilise delta=0",
                    ));
                }
            }

            Ok(request)
        }

        fn tuning_requested(&self) -> bool {
            self.window.is_some() || self.delta.is_some() || self.batch.is_some()
        }

        fn default_window(&self) -> usize {
            self.algorithm
                .descriptor()
                .parameter("window")
                .unwrap_or(64) as usize
        }

        fn tuning_values(&self) -> Vec<WindowTuning> {
            let windows = match self.window {
                Some(Selection::All) => WINDOW_SWEEP.to_vec(),
                Some(Selection::One(window)) => vec![window],
                None => vec![self.default_window()],
            };
            let deltas = match self.delta {
                Some(Selection::All) => DELTA_SWEEP.to_vec(),
                Some(Selection::One(delta)) => vec![delta],
                None if self.policy == "spatial_tiebreak" => vec![0],
                None => vec![self.algorithm.descriptor().parameter("delta").unwrap_or(16)],
            };
            let batch = self.batch.unwrap_or_else(|| {
                self.algorithm
                    .descriptor()
                    .parameter("max_batch")
                    .unwrap_or(32)
            });
            windows
                .into_iter()
                .flat_map(|window| {
                    deltas.iter().copied().map(move |delta| WindowTuning {
                        window,
                        delta,
                        batch,
                    })
                })
                .collect()
        }

        fn resolve(&self) -> Result<Scenario> {
            if let (Some(start), Some(end)) = (self.start, self.end) {
                return Ok(Scenario {
                    name: self.case_id.clone(),
                    file: self
                        .file
                        .clone()
                        .unwrap_or_else(|| self.layout.gps_filename().to_string()),
                    start_lon: start.0,
                    start_lat: start.1,
                    end_lon: end.0,
                    end_lat: end.1,
                });
            }

            let mut scenario = load_scenarios(&self.scenarios)?
                .into_iter()
                .find(|scenario| scenario.name == self.case_id)
                .ok_or_else(|| {
                    invalid(format!(
                        "scénario introuvable: case={} dans {}",
                        self.case_id,
                        self.scenarios.display()
                    ))
                })?;
            scenario.file = self
                .file
                .clone()
                .unwrap_or_else(|| self.layout.gps_filename().to_string());
            Ok(scenario)
        }
    }

    pub fn run() -> Result<()> {
        let request = Request::parse()?;
        let scenario = request.resolve()?;
        let run_id = make_run_id();
        let profiles = request.profile.map_or_else(
            || vec![Profile::Ram128, Profile::Ram256],
            |profile| vec![profile],
        );
        let tunings = if request.tuning_requested() {
            request.tuning_values().into_iter().map(Some).collect()
        } else {
            vec![None]
        };
        let summarize = tunings.len() > 1;
        if summarize {
            println!(
                "Essais A* Window : {} combinaison(s) par profil, sans sauvegarde.",
                tunings.len()
            );
            let mut header = format!(
                "{:<8} {:>6} {:>5} {:>5} {:>11} {:>8} {:>8} {:>10} {:>8}",
                "Profil",
                "Window",
                "Delta",
                "Batch",
                "Durée(ms)",
                "Coût",
                "SWAP(bl)",
                "Pops>min",
                "Marge f"
            );
            if matches!(request.mode, Mode::Inspect) {
                header.push_str(&format!(" {:>11} {:>11}", "Lect. flash", "Écr. flash"));
            }
            println!("{header}");
        }

        for profile in profiles {
            let save_dir = request.save.then(|| {
                let root = env::var("BENCH_RAW_ROOT").unwrap_or_else(|_| "bench/raw".to_string());
                PathBuf::from(root).join(&run_id).join(profile.name())
            });

            if let Some(dir) = &save_dir {
                fs::create_dir_all(dir)?;
            }

            for tuning in &tunings {
                if let Some(report) = launch_worker(
                    &request,
                    &scenario,
                    profile,
                    save_dir.as_deref(),
                    &run_id,
                    *tuning,
                    summarize,
                )? {
                    print_tuning_row(profile, request.mode, &report)?;
                }
            }
        }
        Ok(())
    }

    fn launch_worker(
        request: &Request,
        scenario: &Scenario,
        profile: Profile,
        save_dir: Option<&Path>,
        run_id: &str,
        tuning: Option<WindowTuning>,
        summarize: bool,
    ) -> Result<Option<serde_json::Value>> {
        let mut features = vec!["bench-worker", request.mode.feature(), profile.feature()];
        if request.mode.latency() {
            features.push("latency");
        }

        let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let worker_root = env::var_os("BENCH_WORKER_TARGET_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target").join("bench-workers"));
        let worker_target = worker_root.join(format!(
            "{}-{}-{}",
            request.mode.feature(),
            profile.name(),
            if request.mode.latency() {
                "latency"
            } else {
                "yield"
            }
        ));

        let worker_target = match tuning {
            Some(tuning) if tuning.window != request.default_window() => worker_target
                .with_file_name(format!(
                    "{}-window{}",
                    worker_target.file_name().unwrap().to_string_lossy(),
                    tuning.window
                )),
            _ => worker_target,
        };
        let mut command = Command::new(cargo);
        command
            .arg("run")
            .arg("--quiet")
            .arg("--release")
            .arg("--no-default-features")
            .arg("--features")
            .arg(features.join(","))
            .arg("--bin")
            .arg("bench-bin")
            .env("CARGO_TARGET_DIR", worker_target)
            .arg("--")
            .arg("--worker")
            .arg("--algo")
            .arg(request.algorithm.name())
            .arg("--file")
            .arg(&scenario.file)
            .arg("--layout")
            .arg(request.layout.name())
            .arg("--case")
            .arg(&scenario.name)
            .arg("--from")
            .arg(scenario.start_lon.to_string())
            .arg(scenario.start_lat.to_string())
            .arg("--to")
            .arg(scenario.end_lon.to_string())
            .arg(scenario.end_lat.to_string())
            .arg("--inspect-stride")
            .arg(request.inspect_stride.to_string())
            .env("BENCH_RUN_ID", run_id)
            .env("BENCH_GRAPH_STORAGE", request.graph_storage.as_str())
            .env("BENCH_NEIGHBOR_ACCESS", "tile_segments")
            .env("BENCH_EXPLORATION_POLICY", &request.policy)
            .env("BENCH_BUCKET_STATE", &request.bucket_state);

        if let Some(dir) = save_dir {
            command.arg("--save-dir").arg(dir).arg("--saved-output");
        }

        if let Some(tuning) = tuning {
            command
                .env("BENCH_WINDOW_CAPACITY", tuning.window.to_string())
                .arg("--window")
                .arg(tuning.window.to_string())
                .arg("--delta")
                .arg(tuning.delta.to_string())
                .arg("--batch")
                .arg(tuning.batch.to_string());
        } else {
            // Garder les exécutants ordinaires à la capacité par défaut du registre,
            // même si le processus parent utilise une capacité Window personnalisée.
            command.env(
                "BENCH_WINDOW_CAPACITY",
                request.default_window().to_string(),
            );
        }

        if summarize {
            command.arg("--tuning-output").stderr(Stdio::inherit());
            let output = command.output()?;
            ensure_success(output.status, "exécution du benchmark")?;
            let report = serde_json::from_slice::<serde_json::Value>(&output.stdout)
                .map_err(|err| Error::other(format!("résumé Window invalide : {err}")))?;
            let expected = tuning.ok_or_else(|| Error::other("réglages Window manquants"))?;
            if report.get("window").and_then(serde_json::Value::as_u64)
                != Some(expected.window as u64)
                || report.get("delta").and_then(serde_json::Value::as_u64)
                    != Some(expected.delta as u64)
                || report.get("batch").and_then(serde_json::Value::as_u64)
                    != Some(expected.batch as u64)
            {
                return Err(Error::other(
                    "les réglages du worker diffèrent de ceux demandés",
                ));
            }
            Ok(Some(report))
        } else {
            ensure_success(command.status()?, "exécution du benchmark")?;
            Ok(None)
        }
    }

    fn print_tuning_row(profile: Profile, mode: Mode, report: &serde_json::Value) -> Result<()> {
        let number = |key: &str| {
            report
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| Error::other(format!("champ {key} absent du résumé Window")))
        };
        let optional_number = |key: &str| {
            report
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .map_or_else(|| "—".into(), |value| value.to_string())
        };
        let cost = if report.get("found").and_then(serde_json::Value::as_bool) == Some(true) {
            number("cost")?.to_string()
        } else {
            "absent".into()
        };
        let mut row = format!(
            "{:<8} {:>6} {:>5} {:>5} {:>11.3} {:>8} {:>8} {:>10} {:>8}",
            profile.name(),
            number("window")?,
            number("delta")?,
            number("batch")?,
            number("search_us")? as f64 / 1_000.0,
            cost,
            number("swap_cache_blocks")?,
            number("relaxed_pops")?,
            number("max_f_slack")?,
        );
        if matches!(mode, Mode::Inspect) {
            row.push_str(&format!(
                " {:>11} {:>11}",
                optional_number("flash_reads"),
                optional_number("flash_writes")
            ));
        }
        println!("{row}");
        Ok(())
    }

    fn load_scenarios(path: &Path) -> Result<Vec<Scenario>> {
        let text = fs::read_to_string(path)?;
        let mut scenarios = Vec::new();

        for (index, line) in text.lines().enumerate() {
            let line = line.trim_end_matches('\r').trim();
            if line.is_empty() || index == 0 && line.starts_with("name,") {
                continue;
            }

            let cols: Vec<_> = line.split(',').map(str::trim).collect();
            if cols.len() != 5 {
                return Err(invalid(format!(
                    "ligne scénario invalide {}:{}",
                    path.display(),
                    index + 1
                )));
            }

            scenarios.push(Scenario {
                name: cols[0].to_string(),
                file: String::new(),
                start_lon: parse_f64(cols[1], "start_lon")?,
                start_lat: parse_f64(cols[2], "start_lat")?,
                end_lon: parse_f64(cols[3], "end_lon")?,
                end_lat: parse_f64(cols[4], "end_lat")?,
            });
        }

        Ok(scenarios)
    }

    fn read_point(args: &mut impl Iterator<Item = String>, option: &str) -> Result<(f64, f64)> {
        let lon = parse_f64(&next(args, option)?, "longitude")?;
        let lat = parse_f64(&next(args, option)?, "latitude")?;
        Ok((lon, lat))
    }

    fn next(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String> {
        args.next()
            .ok_or_else(|| invalid(format!("valeur manquante après {option}")))
    }

    fn parse_f64(value: &str, label: &str) -> Result<f64> {
        value
            .parse::<f64>()
            .map_err(|_| invalid(format!("{label} invalide: {value}")))
    }

    fn make_run_id() -> String {
        env::var("BENCH_RUN_ID").unwrap_or_else(|_| {
            format!(
                "single_{}_{}",
                Local::now().format("%Y%m%d_%H%M%S_%3f"),
                std::process::id()
            )
        })
    }

    fn ensure_success(status: ExitStatus, label: &str) -> Result<()> {
        if status.success() {
            Ok(())
        } else {
            Err(Error::other(format!(
                "{label} échouée avec le statut {status}"
            )))
        }
    }

    fn invalid(message: impl Into<String>) -> Error {
        Error::new(ErrorKind::InvalidInput, message.into())
    }

    fn print_help() {
        println!("Algorithmes actifs (algorithms.json) :");
        for algo in Algorithm::ALL
            .iter()
            .filter(|algo| algo.descriptor().enabled)
        {
            let d = algo.descriptor();
            println!(
                "  {} : {} ; policies={:?}, states={:?}",
                d.command, d.label, d.policies, d.states
            );
        }
        println!(
            "\
Usage:
  cargo run --release --bin bench-bin -- ALGO [options]

Exemples:
  cargo run --release --bin bench-bin -- astar
  cargo run --release --bin bench-bin -- dijkstra --case case_003 --layout hilbert
  cargo run --release --bin bench-bin -- astar_bucket --state coupled --policy strict --mode inspect
  cargo run --release --bin bench-bin -- astar_bucket --state decoupled --policy spatial-tiebreak --mode inspect
  cargo run --release --bin bench-bin -- astar_window --profile 256_128 --layout hilbert --case case_003 --window 256 --delta 8
  cargo run --release --bin bench-bin -- astar_window --profile 256_128 --layout hilbert --case case_003 --window all --delta all
  cargo run --release --bin bench-bin -- astar --mode inspect --save

Options:
  --case ID                         case_001 par défaut
  --layout normal|hilbert|zcurve   normal par défaut
  --file FICHIER                    surcharge le fichier du layout
  --from LON LAT / --to LON LAT    coordonnées personnalisées
  --graph-storage tile_dicho|tile_index
                                    Dichotomic ou Indexed
  --neighbor-access tile_segments      parcours des segments des tuiles
  --mode perf|inspect         perf par défaut
  --policy ID                      politique compatible, selon la liste ci-dessus
  --state ID                       stockage compatible, selon la liste ci-dessus
  --list-algorithms                 affiche le registre complet
  --profile 128k|256k              sans option: lance les deux profils
  --window N|all                    capacité A* Window, essai sans sauvegarde
  --delta N|all                     marge f de Priorité spatiale, 0 autorisé
  --batch N                        plafond du lot A* Window, entier > 0 (défaut: registre)
  --save                            sauvegarde uniquement dans bench/raw
  --inspect-stride N          snapshot cache toutes les N actions (100 par défaut)

Sémantique des modes:
  perf                              latence flash simulée toujours active
  inspect                      yield toujours actif, sans latence simulée

Pour une campagne complète et la reconstruction des résultats:
  scripts/bench.sh campaign ALGO
  scripts/bench.sh build
"
        );
        println!(
            "Réglages A* Window : --window all = {WINDOW_SWEEP:?} ; --delta all = {DELTA_SWEEP:?}.\nUne fenêtre non standard nécessite une compilation spécialisée, réutilisée pour ses deltas et lots.\n--window, --delta ou --batch sont incompatibles avec --save ; les essais ne produisent aucun CSV ni replay."
        );
    }
}

#[cfg(not(feature = "bench-worker"))]
fn main() {
    if let Err(err) = cli::run() {
        eprintln!("Erreur: {err}");
        std::process::exit(1);
    }
}

#[cfg(feature = "bench-worker")]
#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    let _ = spawner;

    let _ = env_logger::builder()
        .format_timestamp(None)
        .parse_filters("info")
        .try_init();

    if let Err(err) = worker::run().await {
        eprintln!("Erreur: {err}");
        std::process::exit(1);
    }

    std::process::exit(0);
}
