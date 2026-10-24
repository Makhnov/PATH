use serde_json::Value;
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn string(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("algorithms.json: {key} manquant"))
        .to_owned()
}
fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("algorithms.json: {key} doit être une liste"))
        .iter()
        .map(|v| v.as_str().expect("valeur textuelle attendue").to_owned())
        .collect()
}
fn identifier(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_lowercase())
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
fn main() {
    println!("cargo:rerun-if-changed=algorithms.json");
    println!("cargo:rerun-if-env-changed=BENCH_WINDOW_CAPACITY");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let raw = fs::read_to_string(root.join("algorithms.json")).expect("algorithms.json absent");
    let catalog: Value = serde_json::from_str(&raw).expect("algorithms.json invalide");
    assert_eq!(catalog["schema_version"], 1, "version de registre inconnue");
    let entries = catalog["algorithms"]
        .as_array()
        .expect("liste algorithms manquante");
    assert!(!entries.is_empty(), "registre vide");
    let default_window = entries
        .iter()
        .find(|entry| entry["id"] == "astar_window")
        .and_then(|entry| entry["parameters"]["window"].as_u64())
        .unwrap_or(64) as usize;
    let window_capacity: usize = env::var("BENCH_WINDOW_CAPACITY")
        .map(|value| {
            value
                .parse()
                .expect("BENCH_WINDOW_CAPACITY doit être un entier")
        })
        .unwrap_or(default_window);
    assert!(
        (1..=4096).contains(&window_capacity),
        "BENCH_WINDOW_CAPACITY attendu: 1..=4096"
    );
    let mut names = BTreeSet::new();
    let mut variants = Vec::new();
    let mut descriptors = Vec::new();
    let mut modules = String::new();
    let mut dispatch = String::new();
    let mut metric_names = Vec::new();
    let mut capacity = 1;
    for entry in entries {
        let id = string(entry, "id");
        let command = string(entry, "command");
        assert!(
            identifier(&id) && identifier(&command),
            "id/commande doivent être des identifiants Rust"
        );
        let enabled = entry["enabled"]
            .as_bool()
            .expect("enabled doit être booléen");
        let mut entry_names = BTreeSet::new();
        for name in [&id, &command] {
            assert!(
                name == &name.trim().to_ascii_lowercase() && !name.is_empty(),
                "identifiant invalide"
            );
            if entry_names.insert(name) {
                assert!(names.insert(name.clone()), "identifiant dupliqué: {name}");
            }
        }
        let policies = strings(entry, "policies");
        let states = strings(entry, "states");
        assert!(!policies.is_empty(), "politique par défaut manquante");
        for list in [&policies, &states] {
            assert_eq!(
                list.iter().collect::<BTreeSet<_>>().len(),
                list.len(),
                "options dupliquées"
            );
            assert!(list.iter().all(|s| identifier(s)), "option invalide");
        }
        let specific = strings(entry, "specific_metrics");
        assert!(specific.iter().all(|s| identifier(s)), "métrique invalide");
        assert_eq!(
            specific.iter().collect::<BTreeSet<_>>().len(),
            specific.len(),
            "métriques dupliquées"
        );
        capacity = capacity.max(specific.len());
        for name in &specific {
            if !metric_names.contains(name) {
                metric_names.push(name.clone());
            }
        }
        let variant: String = id
            .split('_')
            .map(|part| {
                let mut chars = part.chars();
                format!(
                    "{}{}",
                    chars.next().unwrap().to_ascii_uppercase(),
                    chars.as_str()
                )
            })
            .collect();
        variants.push(variant.clone());
        println!("cargo:rustc-check-cfg=cfg(algorithm_{id})");
        if enabled {
            println!("cargo:rustc-cfg=algorithm_{id}");
            let adapter = string(entry, "adapter");
            let path = PathBuf::from(&adapter);
            assert!(
                path.components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
                    && adapter.ends_with(".rs"),
                "chemin adaptateur invalide"
            );
            assert!(root.join(&path).is_file(), "adaptateur absent: {adapter}");
            println!("cargo:rerun-if-changed={adapter}");
            modules += &format!("#[path = {:?}] mod adapter_{id};\n", root.join(path));
            dispatch += &format!(
                "Self::{variant} => Ok(adapter_{id}::run::<G,C,F,B>(graph,start,end,options,emit,bench).await),\n"
            );
        } else {
            dispatch += &format!("Self::{variant} => Err(\"algorithme désactivé\"),\n");
        }
        let parameters: Vec<(String, u32)> = entry
            .get("parameters")
            .and_then(Value::as_object)
            .map(|object| {
                object
                    .iter()
                    .map(|(name, value)| {
                        assert!(identifier(name), "paramètre invalide");
                        (
                            name.clone(),
                            u32::try_from(value.as_u64().expect("paramètre entier attendu"))
                                .expect("paramètre trop grand"),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        descriptors.push(format!("AlgorithmDescriptor {{ id: {id:?}, command: {command:?}, label: {:?}, short_label: {:?}, enabled: {enabled}, policies: &{policies:?}, states: &{states:?}, uses_heuristic: {}, specific_metrics: &{specific:?}, parameters: &{parameters:?} }}", string(entry,"label"), string(entry,"short_label"), entry["uses_heuristic"].as_bool().expect("uses_heuristic booléen attendu")));
    }
    let descriptor_arms: String = variants
        .iter()
        .enumerate()
        .map(|(i, v)| format!("Self::{v} => &DESCRIPTORS[{i}],\n"))
        .collect();
    let algorithms = variants
        .iter()
        .map(|v| format!("Self::{v}"))
        .collect::<Vec<_>>()
        .join(",");
    let output = format!(
        r#"
pub const CATALOG_JSON: &str = {raw:?};
pub const WINDOW_CAPACITY: usize = {window_capacity};
pub const SPECIFIC_METRIC_CAPACITY: usize = {capacity};
pub const SPECIFIC_METRIC_NAMES: &[&str] = &{metric_names:?};
pub const DESCRIPTORS: &[AlgorithmDescriptor] = &[{}];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {{ {} }}
{modules}
impl Algorithm {{
    pub const ALL: &'static [Self] = &[{algorithms}];
    pub fn descriptor(self) -> &'static AlgorithmDescriptor {{ match self {{ {descriptor_arms} }} }}
    pub fn parse(value: &str) -> Option<Self> {{
        let value = value.trim();
        Self::ALL.iter().copied().find(|algo| {{
            let d=algo.descriptor(); d.enabled &&
                (d.id.eq_ignore_ascii_case(value) || d.command.eq_ignore_ascii_case(value))
        }})
    }}
    pub fn csv_name(self) -> &'static str {{ self.descriptor().id }}
    pub fn short_name(self) -> &'static str {{ self.descriptor().command }}
    pub fn backend_name(self) -> &'static str {{ self.descriptor().command }}
    pub fn name(self) -> &'static str {{ self.descriptor().command }}
    pub async fn run<G,C,F,B>(self, graph:&G, start:Node, end:Node, options:RunOptions<'_>, emit:F, bench:&mut B) -> Result<AlgorithmRun<C>, &'static str>
    where G:Graph<ULoc=GNode,Loc=Node>+InspectGraph, C:CacheSingleton, F:FnMut(GpsEvent<GNode>), B:Bench {{
        self.descriptor().validate(options)?;
        match self {{ {dispatch} }}
    }}
}}
"#,
        descriptors.join(",\n"),
        variants.join(",")
    );
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("algorithm_registry.rs"),
        output,
    )
    .unwrap();
}
