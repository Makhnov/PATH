// Contrat d’intégration des algorithmes. Les métadonnées et la sélection sont générées depuis algorithms.json
use crate::{
    event::GpsEvent,
    format::Node,
    graph::{GNode, Graph},
    inspection::{Bench, BenchResult, InspectGraph},
};
use swap_cache::cache::CacheSingleton;

pub const WINDOW_SWEEP: [usize; 4] = [384, 512, 1024, 2048];
pub const DELTA_SWEEP: [u32; 4] = [0, 8, 16, 64];
// pub const DELTA_SWEEP: [u32; 6] = [0, 4, 8, 12, 16, 32];
pub const MAX_WINDOW_CAPACITY: usize = 4096;

pub fn validate_capacity(capacity: usize) -> Result<(), &'static str> {
    if capacity == 0 || capacity > MAX_WINDOW_CAPACITY {
        Err("--window attendu: entier de 1 à 4096, ou all")
    } else {
        Ok(())
    }
}

pub struct AlgorithmDescriptor {
    pub id: &'static str,
    pub command: &'static str,
    pub label: &'static str,
    pub short_label: &'static str,
    pub enabled: bool,
    pub policies: &'static [&'static str],
    pub states: &'static [&'static str],
    pub uses_heuristic: bool,
    pub specific_metrics: &'static [&'static str],
    pub parameters: &'static [(&'static str, u32)],
}

impl AlgorithmDescriptor {
    pub fn parameter(&self, key: &str) -> Option<u32> {
        self.parameters
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    }
    pub fn policy<'a>(&'a self, requested: Option<&'a str>) -> &'a str {
        requested
            .filter(|value| self.policies.contains(value))
            .unwrap_or(self.policies[0])
    }
    pub fn state<'a>(&'a self, requested: Option<&'a str>) -> &'a str {
        requested
            .filter(|value| self.states.contains(value))
            .unwrap_or(self.states.first().copied().unwrap_or(""))
    }
    pub fn validate(&self, options: RunOptions<'_>) -> Result<(), &'static str> {
        if !self.enabled {
            return Err("algorithme désactivé dans algorithms.json");
        }
        if !self.policies.contains(&options.policy) {
            return Err("politique incompatible avec cet algorithme");
        }
        if self.state(Some(options.state)) != options.state {
            return Err("stockage incompatible avec cet algorithme");
        }
        for (index, (key, _)) in options.parameters.iter().enumerate() {
            if self.parameter(key).is_none() {
                return Err("paramètre inconnu pour cet algorithme");
            }
            if options.parameters[..index]
                .iter()
                .any(|(prior, _)| prior == key)
            {
                return Err("paramètre dupliqué");
            }
        }
        if self.id == "astar_window" {
            if options
                .parameter("window")
                .is_some_and(|capacity| capacity as usize != WINDOW_CAPACITY)
            {
                return Err("capacité différente de la fenêtre compilée");
            }
            if options.parameter("max_batch") == Some(0) {
                return Err("un lot doit contenir au moins un candidat");
            }
        }
        if self.id == "astar_window"
            && options.policy == "spatial_tiebreak"
            && options.parameter("delta").is_some_and(|delta| delta != 0)
        {
            return Err("le départage spatial exige delta=0");
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct RunOptions<'a> {
    pub policy: &'a str,
    pub state: &'a str,
    pub parameters: &'a [(&'a str, u32)], // Paramètres qui doivent être validés par le registres avant le run
}

impl RunOptions<'_> {
    pub fn parameter(self, key: &str) -> Option<u32> {
        self.parameters
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    }
}

// Stocke les métriques spécifiques du run dans un tableau fixe
#[derive(Clone, Copy)]
pub struct SpecificMetrics {
    values: [(&'static str, u64); SPECIFIC_METRIC_CAPACITY],
    len: usize,
}

impl Default for SpecificMetrics {
    fn default() -> Self {
        Self {
            values: [("", 0); SPECIFIC_METRIC_CAPACITY],
            len: 0,
        }
    }
}

impl SpecificMetrics {
    pub fn from_values(values: &[(&'static str, u64)]) -> Self {
        assert!(values.len() <= SPECIFIC_METRIC_CAPACITY);
        let mut metrics = Self::default();
        metrics.values[..values.len()].copy_from_slice(values);
        metrics.len = values.len();
        metrics
    }
    pub fn values(&self) -> &[(&'static str, u64)] {
        &self.values[..self.len]
    }
    pub fn get(&self, key: &str) -> Option<u64> {
        self.values()
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    }
    pub fn validate(&self, declared: &[&str]) -> Result<(), &'static str> {
        for (index, (key, _)) in self.values().iter().enumerate() {
            if !declared.contains(key) {
                return Err("métrique spécifique non déclarée dans algorithms.json");
            }
            if self.values()[..index].iter().any(|(prior, _)| prior == key) {
                return Err("métrique spécifique dupliquée");
            }
        }
        Ok(())
    }
}

pub struct AlgorithmRun<C: CacheSingleton> {
    pub result: BenchResult<C>,
    pub metrics: SpecificMetrics,
}

impl<C: CacheSingleton> AlgorithmRun<C> {
    pub fn new(result: BenchResult<C>) -> Self {
        Self {
            result,
            metrics: SpecificMetrics::default(),
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/algorithm_registry.rs"));
