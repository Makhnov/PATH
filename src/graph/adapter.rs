#[cfg(feature = "inspect")]
use crate::inspection::{GpsInspection, GpsInspectors, InspectGraph};

use crate::Result;
use crate::format::{Gps, Map, Node};
use crate::graph::tile_index::{FALLBACK, TileIndexedGraph};
use crate::graph::{Edge, GNode, Graph, RawSegment, RawTile, Tile, TileOrder};

use swap_cache::{
    cache::CacheSingleton,
    cbox::CBoxedRef,
    iterator::{AIterator, IntoAIterator},
};

struct Scaling {
    long_scale: u32,
    lat_scale: u32,
    long_scale2: u32,
    lat_scale2: u32,
}

pub struct UGraph<'a, C: CacheSingleton> {
    #[cfg(feature = "inspect")]
    pub gps: &'a Gps<C>,
    pub map: CBoxedRef<'a, Map<C>, C>,
    scale: Scaling,
    order: TileOrder,
}

impl<'a, C: CacheSingleton> UGraph<'a, C> {
    pub async fn new(gps: &'a Gps<C>, order: TileOrder) -> Result<Self> {
        let map = gps.maps.get(0).await.ok_or("No Map block")?;
        let scale = Self::scaling(&map);

        Ok(Self {
            #[cfg(feature = "inspect")]
            gps,
            map,
            scale,
            order,
        })
    }

    pub async fn gnode(&self, long: f64, lat: f64) -> Result<Option<GNode>> {
        self.nearest_gnode(Node::new(long, lat)).await
    }

    fn scaling(map: &Map<C>) -> Scaling {
        // https://gis.stackexchange.com/questions/75528/understanding-terms-in-length-of-degree-formula
        let side = map.dimensions.side;
        let start_lat = map.dimensions.start_coordinates.1;
        let height = map.dimensions.grid_size.1 as f64;
        // let side = map.dimensions.side;

        // Les données ci dessous sont valables à la latitude moyenne de la carte
        let lat_mid_deg = start_lat + (height * side) / 2.0;
        let lat = lat_mid_deg.to_radians();

        // Constantes
        let m1 = 111132.95255;
        let m2 = -559.84957;
        let m3 = 1.17514;
        let m4 = -0.00230;
        let p1 = 111412.87733;
        let p2 = -93.50412;
        let p3 = 0.11774;

        // Mètres par degré de latitude
        let lat_scale = //*
        (m1)
        + (m2 * (2.0 * lat).cos())
        + (m3 * (4.0 * lat).cos())
        + (m4 * (6.0 * lat).cos());

        // Mètres par degré de longitude
        let long_scale = //*
            (p1 * lat.cos())
            + (p2 * (3.0 * lat).cos())
            + (p3 * (5.0 * lat).cos());

        // // Mètres par degré de latitude (Approx wikipedia latitude)
        // let lat_scale = 111_132.0;

        // // Mètres par degré de longitude
        // let long_scale = lat_scale * (1.0 - lat_mid_deg * lat_mid_deg / 8100.0);

        // Coût par mètre (i.e. à 1000 on travaille avec une précision au millimètres)
        let zoom = 10.0;

        // Une unité de grille vaur side / 255 degré
        let long_scale = long_scale * side / 255.0;
        let lat_scale = lat_scale * side / 255.0;

        // Unité de coût en centimères
        let long_scale = (long_scale * zoom) as u32;
        let lat_scale = (lat_scale * zoom) as u32;

        // Bornes u32 (Attention pour A* qui manipule les distances totales et non les distances locales)
        // assert!(long_scale < 180.0);
        // assert!(lat_scale < 180.0);

        // On tronque à la fin pour travailler tout du long en u32
        let long_scale2 = long_scale * long_scale;
        let lat_scale2 = lat_scale * lat_scale;

        Scaling {
            long_scale,
            lat_scale,
            long_scale2,
            lat_scale2,
        }
    }

    // Largeur de la grille !Doit être stockable dans un u8!
    fn width(&self) -> usize {
        self.map.dimensions.grid_size.0
    }

    // Hauteur de la grille !Doit être stockable dans un u8!
    fn height(&self) -> usize {
        self.map.dimensions.grid_size.1
    }

    // 2D => 1D
    pub fn linear_tile(&self, (xt, yt): Tile) -> RawTile {
        let xt = xt as usize;
        let yt = yt as usize;
        yt * self.width() + xt
    }

    // 1D => 2D
    pub fn cartesian_tile(&self, index: RawTile) -> Tile {
        let w = self.width();
        let xt = (index % w) as u8;
        let yt = (index / w) as u8;
        (xt, yt)
    }

    // Recherche dichotomique de la position d'une tuile dans les offsets stockés.
    async fn dichotomic_tile_position(&self, rtile: RawTile) -> Option<usize> {
        let target_rank = self.tile_rank(self.cartesian_tile(rtile));

        let stored_tiles = &self.map.offsets.stored_tiles;
        let mut left = 0;
        let mut right = stored_tiles.len();

        while left < right {
            let middle = left + (right - left) / 2;
            let candidate = *stored_tiles.get(middle).await?;
            let candidate_rank = self.tile_rank(self.cartesian_tile(candidate));

            match candidate_rank.cmp(&target_rank) {
                core::cmp::Ordering::Less => left = middle + 1,
                core::cmp::Ordering::Greater => right = middle,
                core::cmp::Ordering::Equal => return Some(middle),
            }
        }

        None
    }

    // Fin des segments d'une tuile à sa position dans les offsets stockés.
    async fn tile_segment_end(&self, index: usize) -> Result<usize> {
        self.map
            .offsets
            .stored_tiles_ends
            .get(index)
            .await
            .ok_or("No tile offset")
            .map(|end| *end)
    }

    fn unify(w: u8, h: u8, mut xt: u8, mut yt: u8, mut xl: u8, mut yl: u8) -> GNode {
        let topmost_tile = yt == h - 1;
        let rightmost_tile = xt == w - 1;

        // Node déplacé dans la tuile de droite
        if xl == 255 && !rightmost_tile {
            xt += 1;
            xl = 0;
        }

        // Node déplacé dans la tuile du dessus
        if yl == 255 && !topmost_tile {
            yt += 1;
            yl = 0;
        }

        // N.B. Pas de break, dans les if, techniquement un node peut être déplacé en diagonale (haut-droite)
        GNode::new((xt, yt), (xl, yl))
    }

    // Création des deux GNodes depuis un segment et sa tuile, on garantit l'absence de doublon
    pub fn gnodes(&self, rtile: RawTile, segment: RawSegment) -> (GNode, GNode) {
        let (xt, yt) = self.cartesian_tile(rtile);
        let [xl1, yl1, xl2, yl2] = segment;
        let w = self.width() as u8;
        let h = self.height() as u8;

        (
            Self::unify(w, h, xt, yt, xl1, yl1),
            Self::unify(w, h, xt, yt, xl2, yl2),
        )
    }

    // Calcul du coût pondéré d'une distance entre deux points d'une même tuile
    pub fn cost(&self, segment: RawSegment) -> u32 {
        // On calcule les différences locales sur les deux axes
        let dx = segment[0].abs_diff(segment[2]) as u32;
        let dy = segment[1].abs_diff(segment[3]) as u32;

        // Et leur carré
        let dx2 = dx * dx;
        let dy2 = dy * dy;

        // On multiplie ces carrés par les scaling précalculés et on additionne les deux produits
        let total = dx2 * self.scale.long_scale2 + dy2 * self.scale.lat_scale2;

        // On renvoie la racine
        total.isqrt()
    }

    // Création d'un gnode depuis un node
    fn to_gnode(&self, node: Node) -> GNode {
        // TODO: Gérer le None
        let Node { x, y } = node;
        let xi = self.map.dimensions.start_coordinates.0;
        let yi = self.map.dimensions.start_coordinates.1;
        let side = self.map.dimensions.side;

        let dx = x - xi;
        let dy = y - yi;

        let gx = dx / side;
        let gy = dy / side;

        let xt = gx as u8;
        let yt = gy as u8;

        let xi = ((gx - xt as f64) * 255.0) as u8;
        let yi = ((gy - yt as f64) * 255.0) as u8;

        let tile = (xt, yt);
        let index = (xi, yi);

        GNode::new(tile, index)
    }

    // Création d'un node depuis un GNode
    fn to_node(&self, gnode: GNode) -> Node {
        let (xt, yt) = gnode.tile();
        let xt = xt as f64;
        let yt = yt as f64;

        let (xi, yi) = gnode.index();
        let xi = xi as f64;
        let yi = yi as f64;

        let xs = self.map.dimensions.start_coordinates.0;
        let ys = self.map.dimensions.start_coordinates.1;
        let side = self.map.dimensions.side;

        let x = xs + (xt + xi / 255.0) * side;
        let y = ys + (yt + yi / 255.0) * side;

        Node::new(x, y)
    }

    // Rang Hilbert
    fn hside(&self) -> usize {
        self.width().max(self.height()).next_power_of_two()
    }

    // Renvoie un itérateur sur des arrêtes brutes non orientées (format binaire)
    pub async fn segments(&self, tile: Tile) -> Result<impl AIterator<Lend = RawSegment> + '_> {
        let rtile = self.linear_tile(tile);
        let (begin, end) = // *
        if let Some(stile) = self.dichotomic_tile_position(rtile).await {
            let begin = if let Some(previous) = stile.checked_sub(1) {
                self.tile_segment_end(previous).await?
            } else {
                0
            };

            let end = self.tile_segment_end(stile).await?;

            (begin, end)
        } else {
            (0, 0)
        };
        let len = end
            .checked_sub(begin)
            .ok_or("invalid tile segment bounds")?;

        Ok(self.map.data.iter().skip(begin).take(len).copied())
    }
}

impl<C: CacheSingleton> TileIndexedGraph<'_, '_, C> {
    // Renvoie un itérateur sur des arrêtes brutes non orientées mais indexées
    pub(super) async fn indexed_segments(
        &self,
        tile: Tile,
    ) -> Result<impl AIterator<Lend = RawSegment> + '_> {
        let (width, height) = self.source.map.dimensions.grid_size;
        let (begin, end) = if tile.0 as usize >= width || tile.1 as usize >= height {
            (0, 0)
        } else {
            let packed = *self
                .entries
                .get(self.source.linear_tile(tile))
                .await
                .expect("valid tile has an index entry");
            if packed == FALLBACK {
                self.dichotomic_segment_bounds(tile).await?
            } else {
                let begin = (packed >> 8) as usize;
                (begin, begin + (packed & 0xff) as usize)
            }
        };
        Ok(self
            .source
            .map
            .data
            .iter()
            .skip(begin)
            .take(end - begin)
            .copied())
    }
}

pub(super) fn edges<'a, C: CacheSingleton>(
    source: &'a UGraph<'_, C>,
    tile: Tile,
    gnode: GNode,
    segments: impl AIterator<Lend = RawSegment> + 'a,
) -> impl AIterator<Lend = Edge<GNode>> + 'a {
    let rtile = source.linear_tile(tile);
    let (xt, yt) = gnode.tile;
    let (xi, yi) = gnode.index;
    let x = if tile.0 == xt { xi } else { 255 };
    let y = if tile.1 == yt { yi } else { 255 };

    segments
        .filter(async move |segment| {
            (segment[0] == x && segment[1] == y) || (segment[2] == x && segment[3] == y)
        })
        .map(async move |segment| {
            let cost = source.cost(segment);
            let (node, neighbor) = source.gnodes(rtile, segment);

            Edge {
                node,
                neighbor,
                cost,
            }
        })
}

fn morton_rank((x, y): Tile) -> usize {
    let x = x as usize;
    let y = y as usize;
    let mut rank = 0usize;

    for bit in 0..u8::BITS as usize {
        rank |= ((x >> bit) & 1) << (2 * bit);
        rank |= ((y >> bit) & 1) << (2 * bit + 1);
    }

    rank
}

fn hilbert_rank((x, y): Tile, side: usize) -> usize {
    let mut x = x as usize;
    let mut y = y as usize;
    let mut rank = 0usize;
    let mut scale = side / 2;

    while scale != 0 {
        let rx = usize::from(x & scale != 0);
        let ry = usize::from(y & scale != 0);

        rank += scale * scale * ((3 * rx) ^ ry);

        if ry == 0 {
            if rx == 1 {
                x = side - 1 - x;
                y = side - 1 - y;
            }

            core::mem::swap(&mut x, &mut y);
        }

        scale /= 2;
    }

    rank
}

impl<'a, C: CacheSingleton> Graph for UGraph<'a, C> {
    type ULoc = GNode;
    type Loc = Node;

    fn tile_rank(&self, tile: Tile) -> usize {
        match self.order {
            TileOrder::RowMajor => self.linear_tile(tile),
            TileOrder::Morton => morton_rank(tile),
            TileOrder::Hilbert => hilbert_rank(tile, self.hside()),
        }
    }

    fn heuristic(&self, from: GNode, to: GNode) -> u32 {
        let (from_xt, from_yt) = from.tile();
        let (from_xi, from_yi) = from.index();

        let (to_xt, to_yt) = to.tile();
        let (to_xi, to_yi) = to.index();

        let from_x = from_xt as u32 * 255 + from_xi as u32;
        let from_y = from_yt as u32 * 255 + from_yi as u32;

        let to_x = to_xt as u32 * 255 + to_xi as u32;
        let to_y = to_yt as u32 * 255 + to_yi as u32;

        let dx = from_x.abs_diff(to_x);
        let dy = from_y.abs_diff(to_y);

        let sx = dx * self.scale.long_scale;
        let sy = dy * self.scale.lat_scale;

        sx.max(sy)
    }

    fn heuristile(&self, from: Tile, to: Tile) -> u32 {
        let dx = from.0.abs_diff(to.0) as u32;
        let dy = from.1.abs_diff(to.1) as u32;

        dx + dy
    }

    fn position(&self, gnode: GNode) -> Option<Node> {
        Some(self.to_node(gnode))
    }

    async fn segments(&self, tile: Tile) -> Result<impl AIterator<Lend = RawSegment> + '_> {
        UGraph::segments(self, tile).await
    }

    // Renvoie un itérateur sur des arrêtes uniques et orientées au format UGraph
    async fn edges(
        &self,
        tile: Tile,
        gnode: GNode,
    ) -> Result<impl AIterator<Lend = Edge<GNode>> + '_> {
        let segments = self.segments(tile).await?;
        Ok(edges(self, tile, gnode, segments))
    }

    // Le mode Dichotomic recherche les tuiles propriétaires, puis leurs segments.
    async fn neighbors(&self, gnode: GNode) -> Result<impl AIterator<Lend = Edge<GNode>> + '_> {
        let (xt, yt) = gnode.tile;
        let (xi, yi) = gnode.index;

        let mut voisins = [(0, 0); 4];
        let mut len = 0;

        voisins[len] = (xt, yt);
        len += 1;

        match (xi == 0, yi == 0) {
            (false, false) => {
                // rien
            }

            (false, true) => {
                // On ajoute les edges de la tuile du bas (si elle existe)
                if yt != 0 {
                    voisins[len] = (xt, yt - 1);
                    len += 1;
                }
            }

            (true, false) => {
                // On ajoute les edges de la tuile de gauche (si elle existe)
                if xt != 0 {
                    voisins[len] = (xt - 1, yt);
                    len += 1;
                }
            }

            (true, true) => {
                // On ajoute les edges de la tuile du bas (si elle existe) ...
                if yt != 0 {
                    voisins[len] = (xt, yt - 1);
                    len += 1;
                }
                // celles de la tuile de gauche (si elle existe) ...
                if xt != 0 {
                    voisins[len] = (xt - 1, yt);
                    len += 1;
                }
                // et celles de la tuiles en bas à gauche (si elle existe)
                if yt != 0 && xt != 0 {
                    voisins[len] = (xt - 1, yt - 1);
                    len += 1;
                }
            }
        }

        let mut edge_iters = [None, None, None, None];
        for i in 0..len {
            edge_iters[i] = Some(self.edges(voisins[i], gnode).await?);
        }
        Ok(edge_iters
            .into_iter()
            .flatten()
            .to_async()
            // TODO Ecrire un filter_map pour AIIterator dans la lib swap cache
            .flat_map(async move |edges| {
                // Moins verbeux sans déstructurer l'arrête
                edges.flat_map(async move |mut edge| {
                    // Si le noeud testé est le "voisin" de cet arrrête, on l'inverse
                    if edge.neighbor == gnode {
                        core::mem::swap(&mut edge.node, &mut edge.neighbor);
                    }
                    // Si il existe mais côté "node" on conserve l'arrête en l'état
                    (edge.node == gnode).then_some(edge).into_iter().to_async()
                })
            }))
    }

    async fn nearest_gnode(&self, node: Node) -> Result<Option<GNode>> {
        // On transforme notre loc en gnode
        let gnode = self.to_gnode(node);
        let (xt, yt) = gnode.tile();
        let (xi, yi) = gnode.index();

        let w = self.width() as u8;
        let h = self.height() as u8;

        let nearest = self
            .segments((xt, yt))
            .await?
            .fold(
                None::<((u8, u8), u32)>,
                async move |best, [x1, y1, x2, y2]| {
                    let cand1 = ((x1, y1), self.cost([xi, yi, x1, y1]));
                    let cand2 = ((x2, y2), self.cost([xi, yi, x2, y2]));

                    best.into_iter()
                        .chain([cand1, cand2])
                        .min_by_key(|&(_, cost)| cost)
                },
            )
            .await
            .map(move |((xl, yl), _)| Self::unify(w, h, xt, yt, xl, yl));
        if nearest.is_some() {
            return Ok(nearest);
        }

        // Quand on rencontre une tuile vide, on cherche dans les autres tuiles stockées
        let dimensions = &self.map.dimensions;
        let qx = (node.x - dimensions.start_coordinates.0) / dimensions.side * 255.0;
        let qy = (node.y - dimensions.start_coordinates.1) / dimensions.side * 255.0;
        let distance = |x: f64, y: f64| {
            let dx = (x - qx) * self.scale.long_scale as f64;
            let dy = (y - qy) * self.scale.lat_scale as f64;
            dx * dx + dy * dy
        };
        let mut best = None::<(GNode, f64)>;
        let mut tiles = self.map.offsets.stored_tiles.iter().copied();

        while let Some(rtile) = tiles.next().await {
            let (tx, ty) = self.cartesian_tile(rtile);
            let ox = tx as f64 * 255.0;
            let oy = ty as f64 * 255.0;

            // Si le meilleur point de la tuile est moins bon que notre résultat actuel on skip
            let bx = qx.clamp(ox, ox + 255.0);
            let by = qy.clamp(oy, oy + 255.0);
            if best.is_some_and(|(_, cost)| distance(bx, by) >= cost) {
                continue;
            }

            best = self
                .segments((tx, ty))
                .await?
                .fold(best, async move |best, [x1, y1, x2, y2]| {
                    let candidates = [(x1, y1), (x2, y2)].map(|(x, y)| {
                        (
                            Self::unify(w, h, tx, ty, x, y),
                            distance(ox + x as f64, oy + y as f64),
                        )
                    });
                    best.into_iter()
                        .chain(candidates)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                })
                .await;
        }

        Ok(best.map(|(node, _)| node))
    }
}

#[cfg(feature = "inspect")]
impl<'a, C> InspectGraph for UGraph<'a, C>
where
    C: CacheSingleton,
{
    async fn inspect_gps(&self, inspectors: &mut GpsInspectors) -> GpsInspection {
        let GpsInspectors {
            gps_maps,
            map_stored_tiles,
            map_stored_ends,
            map_segments,
            path_points,
            path_waypoints,
            interest_stored_tiles,
            interest_stored_ends,
            interest_data,
            heights,
            ..
        } = inspectors;

        let mut map_stored_tiles_total = 0usize;
        let mut map_stored_ends_total = 0usize;
        let mut map_segments_total = 0usize;
        let mut map_index = 0usize;

        self.gps
            .maps
            .inspect_cache(async |map, rank| {
                let current_map = map_index;
                map_index += 1;
                if let Some(rank) = rank {
                    gps_maps.current.insert(current_map);
                    gps_maps.observe_rank(rank);
                }

                let mut tile_index = 0usize;
                map.offsets
                    .stored_tiles
                    .inspect_cache(async |_, rank| {
                        let entry = (current_map, tile_index);
                        tile_index += 1;
                        map_stored_tiles_total += 1;

                        if let Some(rank) = rank {
                            map_stored_tiles.current.insert(entry);
                            map_stored_tiles.observe_rank(rank);
                        }
                    })
                    .await;

                let mut end_index = 0usize;
                map.offsets
                    .stored_tiles_ends
                    .inspect_cache(async |_, rank| {
                        let entry = (current_map, end_index);
                        end_index += 1;
                        map_stored_ends_total += 1;

                        if let Some(rank) = rank {
                            map_stored_ends.current.insert(entry);
                            map_stored_ends.observe_rank(rank);
                        }
                    })
                    .await;

                let mut segment_index = 0usize;
                map.data
                    .inspect_cache(async |_, rank| {
                        let entry = (current_map, segment_index);
                        segment_index += 1;
                        map_segments_total += 1;

                        if let Some(rank) = rank {
                            map_segments.current.insert(entry);
                            map_segments.observe_rank(rank);
                        }
                    })
                    .await;
            })
            .await;

        gps_maps.save(map_index);
        map_stored_tiles.save(map_stored_tiles_total);
        map_stored_ends.save(map_stored_ends_total);
        map_segments.save(map_segments_total);

        let mut path_points_total = 0usize;
        let mut path_waypoints_total = 0usize;

        if let Some(path) = &self.gps.path {
            let mut point_index = 0usize;
            path.points
                .inspect_cache(async |_, rank| {
                    let entry = point_index;
                    point_index += 1;
                    path_points_total += 1;

                    if let Some(rank) = rank {
                        path_points.current.insert(entry);
                        path_points.observe_rank(rank);
                    }
                })
                .await;

            let mut waypoint_index = 0usize;
            path.waypoints
                .inspect_cache(async |_, rank| {
                    let entry = waypoint_index;
                    waypoint_index += 1;
                    path_waypoints_total += 1;

                    if let Some(rank) = rank {
                        path_waypoints.current.insert(entry);
                        path_waypoints.observe_rank(rank);
                    }
                })
                .await;
        }

        path_points.save(path_points_total);
        path_waypoints.save(path_waypoints_total);

        let mut interest_stored_tiles_total = 0usize;
        let mut interest_stored_ends_total = 0usize;
        let mut interest_data_total = 0usize;

        if let Some(interests) = &self.gps.interests {
            let mut tile_index = 0usize;
            interests
                .offsets
                .stored_tiles
                .inspect_cache(async |_, rank| {
                    let entry = tile_index;
                    tile_index += 1;
                    interest_stored_tiles_total += 1;

                    if let Some(rank) = rank {
                        interest_stored_tiles.current.insert(entry);
                        interest_stored_tiles.observe_rank(rank);
                    }
                })
                .await;

            let mut end_index = 0usize;
            interests
                .offsets
                .stored_tiles_ends
                .inspect_cache(async |_, rank| {
                    let entry = end_index;
                    end_index += 1;
                    interest_stored_ends_total += 1;

                    if let Some(rank) = rank {
                        interest_stored_ends.current.insert(entry);
                        interest_stored_ends.observe_rank(rank);
                    }
                })
                .await;

            let mut interest_index = 0usize;
            interests
                .data
                .inspect_cache(async |_, rank| {
                    let entry = interest_index;
                    interest_index += 1;
                    interest_data_total += 1;

                    if let Some(rank) = rank {
                        interest_data.current.insert(entry);
                        interest_data.observe_rank(rank);
                    }
                })
                .await;
        }

        interest_stored_tiles.save(interest_stored_tiles_total);
        interest_stored_ends.save(interest_stored_ends_total);
        interest_data.save(interest_data_total);

        let mut heights_total = 0usize;

        if let Some(values) = &self.gps.heights {
            let mut height_index = 0usize;
            values
                .heights
                .inspect_cache(async |_, rank| {
                    let entry = height_index;
                    height_index += 1;
                    heights_total += 1;

                    if let Some(rank) = rank {
                        heights.current.insert(entry);
                        heights.observe_rank(rank);
                    }
                })
                .await;
        }

        heights.save(heights_total);

        inspectors.snapshot()
    }
}
