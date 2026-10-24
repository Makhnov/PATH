// Helper qui prépare et construit les données du graphe pour afficher la carte dans le viewer
use super::model::{
    ViewInterest, ViewLayers, ViewMeta, ViewMetrics, ViewNode, ViewNodeRole, ViewPathPoint,
    ViewSegment, ViewSnapshot, ViewViewport,
};

use lib_gps::{
    format::{Dimensions, Gps, Interests, Node},
    graph::{GNode, Graph, Tile, UGraph},
};

use swap_cache::{cache::CacheSingleton, iterator::AIterator};

use std::io::{Error, ErrorKind, Result};

#[derive(Debug, Clone)]
pub struct SnapshotOptions {
    pub filename: String,
    pub start: Option<Node>,
    pub end: Option<Node>,
    pub max_segments: Option<usize>,
    pub max_interests: Option<usize>,
}

impl SnapshotOptions {
    pub fn new(filename: impl Into<String>) -> Self {
        Self {
            filename: filename.into(),
            start: None,
            end: None,
            max_segments: None,
            max_interests: None,
        }
    }

    pub fn _with_start(mut self, node: Node) -> Self {
        self.start = Some(node);
        self
    }

    pub fn _with_end(mut self, node: Node) -> Self {
        self.end = Some(node);
        self
    }
}

pub async fn build_snapshot<C: CacheSingleton>(
    gps: &Gps<C>,
    graph: &UGraph<'_, C>,
    options: SnapshotOptions,
) -> Result<ViewSnapshot> {
    let map = gps
        .maps
        .get(0)
        .await
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "aucune map dans le fichier GPS"))?;

    let map_dim = &map.dimensions;
    let mut segments = Vec::new();
    let mut segment_id = 0u32;

    let mut stored_tiles = map.offsets.stored_tiles.iter().copied();

    while let Some(rtile) = stored_tiles.next().await {
        let tile = graph.cartesian_tile(rtile);

        let mut segment_iter = graph
            .segments(tile)
            .await
            .map_err(|_| Error::new(ErrorKind::InvalidData, "échec de lecture des segments"))?;

        while let Some(segment) = segment_iter.next().await {
            if options
                .max_segments
                .is_some_and(|max| segments.len() >= max)
            {
                break;
            }

            let [x1, y1, x2, y2] = segment;
            let (gx1, gy1) = position(map_dim, map_dim, tile, (x1, y1));
            let (gx2, gy2) = position(map_dim, map_dim, tile, (x2, y2));

            segments.push(ViewSegment {
                id: segment_id,
                tile: [tile.0, tile.1],
                x1: gx1,
                y1: gy1,
                x2: gx2,
                y2: gy2,
                cost: graph.cost(segment),
            });

            segment_id += 1;
        }

        if options
            .max_segments
            .is_some_and(|max| segments.len() >= max)
        {
            break;
        }
    }

    let interests = if let Some(interests) = gps.interests.as_ref() {
        view_interests(interests, map_dim, options.max_interests).await
    } else {
        Vec::new()
    };

    let mut nodes = Vec::new();
    if let Some(start) = options.start {
        let gnode = graph
            .nearest_gnode(start)
            .await
            .map_err(|_| Error::new(ErrorKind::InvalidData, "échec nearest_gnode départ"))?;

        if let Some(gnode) = gnode {
            nodes.push(view_node(map_dim, gnode, "Départ", ViewNodeRole::Start));
        }
    }

    if let Some(end) = options.end {
        let gnode = graph
            .nearest_gnode(end)
            .await
            .map_err(|_| Error::new(ErrorKind::InvalidData, "échec nearest_gnode arrivée"))?;

        if let Some(gnode) = gnode {
            nodes.push(view_node(map_dim, gnode, "Arrivée", ViewNodeRole::End));
        }
    }

    let straight_line_meters = match (options.start, options.end) {
        (Some(start), Some(end)) => Some(distance(start, end)),
        _ => None,
    };

    let grid_distance = match (
        nodes.iter().find(|n| matches!(n.role, ViewNodeRole::Start)),
        nodes.iter().find(|n| matches!(n.role, ViewNodeRole::End)),
    ) {
        (Some(a), Some(b)) => Some(((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()),
        _ => None,
    };

    let path: Vec<ViewPathPoint> = Vec::new();

    let metrics = ViewMetrics {
        segments: segments.len(),
        interests: interests.len(),
        nodes: nodes.len(),
        path_points: path.len(),
        straight_line_meters,
        grid_distance,
    };

    Ok(ViewSnapshot {
        version: 1,
        meta: ViewMeta {
            filename: options.filename,
            map_count: gps.maps.len(),
            has_interests: gps.interests.is_some(),
        },
        viewport: ViewViewport {
            width: map_dim.grid_size.0 as f64 * 255.0,
            height: map_dim.grid_size.1 as f64 * 255.0,
            lon_min: map_dim.start_coordinates.0,
            lat_min: map_dim.start_coordinates.1,
            tile_side: map_dim.side,
            tiles_x: map_dim.grid_size.0,
            tiles_y: map_dim.grid_size.1,
        },
        layers: ViewLayers {
            segments,
            interests,
            nodes,
            path,
        },
        metrics,
    })
}

pub async fn build_view_node<C: CacheSingleton>(
    gps: &Gps<C>,
    graph: &UGraph<'_, C>,
    lon: f64,
    lat: f64,
    label: &str,
    role: ViewNodeRole,
) -> Result<ViewNode> {
    let map =
        gps.maps.get(0).await.ok_or_else(|| {
            Error::new(ErrorKind::InvalidData, "Pas de 'Map' dans le fichier .gps")
        })?;

    let node = Node::new(lon, lat);

    let gnode = graph
        .nearest_gnode(node)
        .await
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "échec nearest_gnode"))?
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Il n'y a pas de GNode à proximité"))?;

    Ok(view_node(&map.dimensions, gnode, label, role))
}

async fn view_interests<C: CacheSingleton>(
    interests: &Interests<C>,
    map_dim: &Dimensions,
    max_interests: Option<usize>,
) -> Vec<ViewInterest> {
    let mut out = Vec::new();
    let mut id = 0u32;
    let dim = &interests.dimensions;

    let mut stored_tiles = interests.offsets.stored_tiles.iter().copied().enumerate();

    while let Some((stored_index, rtile)) = stored_tiles.next().await {
        let begin = if let Some(previous) = stored_index.checked_sub(1) {
            match interests.offsets.stored_tiles_ends.get(previous).await {
                Some(value) => *value,
                None => return out,
            }
        } else {
            0
        };

        let end = match interests.offsets.stored_tiles_ends.get(stored_index).await {
            Some(value) => *value,
            None => return out,
        };

        let tile = cartesian_tile(dim, rtile);

        let mut entry = begin;
        while entry < end {
            if max_interests.is_some_and(|max| out.len() >= max) {
                return out;
            }

            let Some(raw_interest) = interests.data.get(entry).await else {
                return out;
            };

            let [kind, lx, ly] = *raw_interest;
            let (x, y) = position(map_dim, dim, tile, (lx, ly));

            out.push(ViewInterest {
                id,
                kind,
                tile: [tile.0, tile.1],
                index: [lx, ly],
                x,
                y,
            });

            id += 1;
            entry += 1;
        }
    }

    out
}

fn view_node(map_dim: &Dimensions, gnode: GNode, label: &str, role: ViewNodeRole) -> ViewNode {
    let (xt, yt) = gnode.tile();
    let (ix, iy) = gnode.index();
    let lon = map_dim.start_coordinates.0 + (xt as f64 + ix as f64 / 255.0) * map_dim.side;
    let lat = map_dim.start_coordinates.1 + (yt as f64 + iy as f64 / 255.0) * map_dim.side;

    ViewNode {
        id: gnode.id(),
        label: label.to_string(),
        role,
        tile: [xt, yt],
        index: [ix, iy],
        x: xt as f64 * 255.0 + ix as f64,
        y: yt as f64 * 255.0 + iy as f64,
        lon,
        lat,
    }
}

fn cartesian_tile(dim: &Dimensions, index: usize) -> Tile {
    let width = dim.grid_size.0;
    ((index % width) as u8, (index / width) as u8)
}

fn position(map_dim: &Dimensions, dim: &Dimensions, tile: Tile, index: (u8, u8)) -> (f64, f64) {
    let lon = dim.start_coordinates.0 + (tile.0 as f64 + index.0 as f64 / 255.0) * dim.side;
    let lat = dim.start_coordinates.1 + (tile.1 as f64 + index.1 as f64 / 255.0) * dim.side;

    let gx = ((lon - map_dim.start_coordinates.0) / map_dim.side) * 255.0;
    let gy = ((lat - map_dim.start_coordinates.1) / map_dim.side) * 255.0;

    (gx, gy)
}

fn distance(a: Node, b: Node) -> f64 {
    let earth_radius = 6_371_000.0_f64;
    let lat1 = a.y.to_radians();
    let lat2 = b.y.to_radians();
    let dlat = (b.y - a.y).to_radians();
    let dlon = (b.x - a.x).to_radians();

    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);

    2.0 * earth_radius * h.sqrt().atan2((1.0 - h).sqrt())
}
