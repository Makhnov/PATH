// Aucune adaptation nécessaire, Dijkstra utilise les insertions et les extractions standards
use crate::graph::GNode;

#[derive(Debug, Clone, Copy, Hash)]

// La file de Dijkstra est constituée d'une paire contenant le noeud candidat (GNode = 4 * u8) et de son prédécesseur (GNode + 1 * u8 pour l'option)
// Les couts (g) sont stockés dans la clef (1 * u32)
pub(crate) struct Step {
    // clef                         // 4
    pub(crate) node: GNode,         // 4
    pub(crate) prev: Option<GNode>, // 5
} // 13 octets
