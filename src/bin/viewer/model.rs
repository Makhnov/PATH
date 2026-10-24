use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ViewSnapshot {
    pub version: u32,
    pub meta: ViewMeta,
    pub viewport: ViewViewport,
    pub layers: ViewLayers,
    pub metrics: ViewMetrics,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewMeta {
    pub filename: String,
    pub map_count: usize,
    pub has_interests: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewViewport {
    // Largeur de la carte (en unité de grille)
    pub width: f64,
    // Hauteur de la carte
    pub height: f64,
    // Longitude bas gauche
    pub lon_min: f64,
    // Latitude bas gauche
    pub lat_min: f64,
    // Largeur d'une tuile en degré
    pub tile_side: f64,
    // Colonnes
    pub tiles_x: usize,
    // Lignes
    pub tiles_y: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewLayers {
    pub segments: Vec<ViewSegment>,
    pub interests: Vec<ViewInterest>,
    pub nodes: Vec<ViewNode>,
    pub path: Vec<ViewPathPoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewSegment {
    pub id: u32,
    pub tile: [u8; 2],
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub cost: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewInterest {
    pub id: u32,
    pub kind: u8,
    pub tile: [u8; 2],
    pub index: [u8; 2],
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewNode {
    pub id: u32,
    pub label: String,
    pub role: ViewNodeRole,
    pub tile: [u8; 2],
    pub index: [u8; 2],
    pub x: f64,
    pub y: f64,
    pub lon: f64,
    pub lat: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewNodeRole {
    Start,
    End,
    Debug,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewPathPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewMetrics {
    pub segments: usize,
    pub interests: usize,
    pub nodes: usize,
    pub path_points: usize,
    pub straight_line_meters: Option<f64>,
    pub grid_distance: Option<f64>,
}
