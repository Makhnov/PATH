// Aucune adaptation nécessaire, A* utilise les insertions et les extractions standards
use crate::graph::GNode;

#[derive(Debug, Clone, Copy, Hash)]

// La file de A* est constitué d'une paire contenant le noeud candidat (GNode = 4 * u8) et de son prédécesseur (GNode)
// Les couts (f, g) sont stockés dans la clef (2 * u32)
pub(crate) struct Step {
    // clef                 // 8
    pub(crate) node: GNode, // 4
    pub(crate) prev: GNode, // 4
} // 16 octets
