use super::{Dimensions, TilesOffsets};
use crate::Result;
use crate::format::reader::Read;
use core::mem::size_of;

use swap_cache::{cache::CacheSingleton, collections::RRBTree};

pub type RawInterest = [u8; 3];

// Bloc des points d'intérêt de la zone du chemin principal stocké dans le fichier GPS.
pub struct Interests<C: CacheSingleton> {
    // Dimensions de la grille qui `crop` (large) la zone à afficher
    pub dimensions: Dimensions,

    // Index des tuiles qui contiennent des point d'intérêts pour retrouver les données associées
    pub offsets: TilesOffsets<C>,

    // Données binaires des intérêts. Chaque entrée est encodée sur `offsets.entry_size` (3) octets : Type, x, y    
    pub data: RRBTree<RawInterest, C>,
}

impl<C: CacheSingleton> Interests<C> {
    pub async fn new<R: Read>(reader: &mut R) -> Result<Interests<C>> {
        // Format de la carte affichée
        let dimensions = Dimensions::new(reader)?;

        // Position des points d'intérêts en fonction en fonction des dimensions ci-dessus
        let offsets = TilesOffsets::<C>::new(reader, &dimensions).await?;
        let expected_entry_size = size_of::<RawInterest>();
        if offsets.entry_size != expected_entry_size {
            return Err("invalid Interests entry size");
        }

        // Position du dernier offset (cumulatif, donc == nombre de points d'intérêt stockés) converti en octet, et buffer adéquat
        let end = offsets.end_offset().await?;
        let mut data = RRBTree::<RawInterest, C>::new().await;

        // On récupère toutes nos données dans le buffer qu'on vient de créer, à ce stade notre bloc "3" est entièrement extrait
        for _ in 0..(end / expected_entry_size) {
            let mut interest = [0u8; 3];
            reader.read_exact(&mut interest)?;
            data.push(interest).await;
        }

        Ok(Interests {
            dimensions,
            offsets,
            data,
        })
    }
}
