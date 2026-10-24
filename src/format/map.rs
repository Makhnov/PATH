use super::{Dimensions, TilesOffsets};
use crate::Result;
use crate::format::reader::{ByteReader, Read};
use crate::graph::RawSegment;
use core::mem::size_of;

use swap_cache::{cache::CacheSingleton, collections::RRBTree};

pub struct Map<C: CacheSingleton> {
    pub color: [u8; 3],
    pub dimensions: Dimensions,
    pub offsets: TilesOffsets<C>,
    pub data: RRBTree<RawSegment, C>,
}

impl<C: CacheSingleton> Map<C> {
    pub async fn new<R: Read>(reader: &mut R) -> Result<Map<C>> {
        // Couleur du tracé (rouge = chemin, noir = background, etc.)
        let color = [reader.read_u8()?, reader.read_u8()?, reader.read_u8()?];

        // Format de la carte affichée
        let dimensions = Dimensions::new(reader)?;

        // Position des points du tracé en fonction des dimensions ci-dessus
        let offsets = TilesOffsets::<C>::new_map(reader, &dimensions).await?;
        let expected_entry_size = size_of::<RawSegment>();
        if offsets.entry_size != expected_entry_size {
            return Err("invalid Map entry size");
        }

        // Position du dernier offset (cumulatif, donc == nombre de points dans le tracé) converti en octet, et buffer adéquat
        let end = offsets.end_offset().await?;
        let mut data = RRBTree::<RawSegment, C>::new().await;

        // On récupère toutes nos données dans le buffer qu'on vient de créer, à ce stade notre bloc "0" est entièrement extrait
        for _ in 0..(end / expected_entry_size) {
            let mut segment = [0u8; 4];
            reader.read_exact(&mut segment)?;
            data.push(segment).await;
        }

        Ok(Map {
            color,
            dimensions,
            offsets,
            data,
        })
    }
}
