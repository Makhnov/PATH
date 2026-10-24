#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountdownStateMode {
    // La file conserve ensemble le coût et le prédécesseur, comme dans Countdown legacy
    Coupled,
    // Ici les coûts actifs et les prédécesseurs sont conservés dans des arbres séparés
    Decoupled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountdownPolicy {
    Strict,
    SpatialTieBreak,
}
