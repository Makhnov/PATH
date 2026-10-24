pub mod gps;
pub mod heights;
pub mod interests;
pub mod map;
pub mod node;
pub mod path;
pub mod reader;
pub mod streets;
pub mod utils;

use core::ops::Deref;

use crate::Result;
use crate::format::reader::{ByteReader, Read};
use crate::format::utils::bytes_needed_for;
use crate::format::utils::read_compressed_integers;

use swap_cache::{cache::CacheSingleton, collections::RRBTree};

pub use self::gps::Gps;
pub use self::heights::Heights;
pub use self::interests::Interests;
pub use self::map::Map;
pub use self::node::Node;
pub use self::path::Path;

// use self::streets::Streets;

const _COLORS: [&str; 5] = ["yellow", "red", "blue", "cyan", "green"];

// Dimensions utilisées par [`TilesOffsets::new`] pour connaître le nombre total de tuiles
#[derive(Debug)]
pub struct Dimensions {
    // Première tuile affichée : `(tile_x_min, tile_y_min)`.
    pub first_tile: (usize, usize),

    // Taille de la grille obtenue : `(largeur, hauteur)`.
    pub grid_size: (usize, usize),

    // Coordonnées GPS du coin bas gauche `(longitude min, latitude min)` de la première tuile affichée.
    pub start_coordinates: (f64, f64),

    // Taille réelle d’un côté de tuile (valeurs gps).
    pub side: f64,
}

// Index des tuiles non vides et positions des données afférentes
pub struct TilesOffsets<C: CacheSingleton> {
    // Taille d’une entrée (en octet). 3 pour les [`Interests`], 4 pour les [`Map`])
    pub entry_size: usize,

    // Vecteur d'indices des tuiles qui contiennent des données
    // Encodés sur `tile_index_size` octets (cf. [`TilesOffsets::new`])
    pub stored_tiles: RRBTree<usize, C>,

    // Offsets de fin (cumulés) du nombre d'entrée pour chacune des tuiles occupées
    // Encodés sur `offset_index_size` octets (cf. [`TilesOffsets::new`]) Même `len()` que [`stored_tiles`](Self::stored_tiles)
    pub stored_tiles_ends: RRBTree<usize, C>,
}

impl Dimensions {
    pub fn new<R: Read>(reader: &mut R) -> Result<Dimensions> {
        // Corrdonnées de la première (bas gauche) tuile qui contient des entrées
        let first_tile_x = reader.read_u32_le()? as usize;
        let first_tile_y = reader.read_u32_le()? as usize;
        // println!("Coords 1st_tile: ({};{})", first_tile_x, first_tile_y);

        // Nombre de tuiles en largeur dans la grille (long)
        let grid_size_h = reader.read_u32_le()? as usize;
        // println!("size_h = {}", grid_size_h);
        // Nombre de tuiles en hauteur dans la grille (lat)
        let grid_size_v = reader.read_u32_le()? as usize;
        // println!("size_v = {}", grid_size_v);

        // Coodonnées du point visible plus extrème (bas gauche de la carte)
        let coords_x = reader.read_f64_le()?;
        let coords_y = reader.read_f64_le()?;
        // println!("Coodonnées: ({};{})", coords_x, coords_y);

        // Taille du côté d'une tuile (carrée) en valeur gps
        let side = reader.read_f64_le()?;
        // println!("Side: {}", side);

        Ok(Dimensions {
            first_tile: (first_tile_x, first_tile_y),
            grid_size: (grid_size_h, grid_size_v),
            start_coordinates: (coords_x, coords_y),
            side,
        })
    }
}

// imaginons que j'obtiens un x et un y pour deux points, qui soient les représatations entiere sur une grille complete.
impl<C: CacheSingleton> TilesOffsets<C> {
    pub async fn new<R: Read>(reader: &mut R, dimensions: &Dimensions) -> Result<TilesOffsets<C>> {
        Self::new_inner(reader, dimensions).await
    }

    pub async fn new_map<R: Read>(
        reader: &mut R,
        dimensions: &Dimensions,
    ) -> Result<TilesOffsets<C>> {
        Self::new_inner(reader, dimensions).await
    }

    async fn new_inner<R: Read>(
        reader: &mut R,
        dimensions: &Dimensions,
    ) -> Result<TilesOffsets<C>> {
        // Octets nécessaires pour encoder les offsets eq. nombre d'entrées répertoriées (Intérêts, Maps, etc.)
        let offset_index_size = reader.read_u8()? as usize;
        if !(1..=4).contains(&offset_index_size) {
            return Err("invalid compressed offset size");
        }
        // println!("Taille d'offset: {offset_index_size}");

        // Octets nécessaires pour encoder une entrée. Diffère selon l'objet concerné (Intérêt, Map, etc.)
        let entry_size = reader.read_u8()? as usize;
        // println!("Taille d'une entrée: {entry_size}");

        // Nombre de tuiles réellemnt utilisées pour stocker les données
        let tiles_len = reader.read_u32_le()? as usize;
        // println!("Nombre de tuiles effectives: {tiles_len}");
        //
        // Taille de la grille et taille de l'encodage nécessaire pour le plus gros index
        let grid_size = dimensions.grid_size.0 * dimensions.grid_size.1;
        let tile_index_size = bytes_needed_for(grid_size);
        if !(1..=4).contains(&tile_index_size) {
            return Err("invalid compressed tile index size");
        }

        // Vecteur d'indexation des tuiles effectives i.e. [4, 19, 452] : Les Cinquième, Vingtième, ... tuiles sont occupées
        let stored_tiles =
            read_compressed_integers::<R, C>(reader, tile_index_size, tiles_len).await?;

        // Vecteur d'offset des tuiles (cumulatif) i.e. [3, 5, 11] : La tuile vingt contient les entrées 3..5
        let stored_tiles_ends =
            read_compressed_integers::<R, C>(reader, offset_index_size, tiles_len).await?;

        Ok(TilesOffsets {
            entry_size,
            stored_tiles,
            stored_tiles_ends,
        })
    }

    async fn end_offset(&self) -> Result<usize> {
        let len = self.stored_tiles_ends.len();

        if len == 0 {
            return Ok(0);
        }

        self.stored_tiles_ends
            .get(len - 1)
            .await
            .ok_or("missing tile offset")
            .map(|last| last.deref() * self.entry_size)
    }
}
