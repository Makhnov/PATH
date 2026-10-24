use super::{Heights, Interests, Map, Path, streets};
use crate::format::reader::{ByteReader, Read};
use crate::{Result, result::error::EOF};
use swap_cache::{cache::CacheSingleton, collections::RRBTree};

pub struct Gps<C: CacheSingleton> {
    // TODO Étudier le préindexage des tuiles directement dans Map/Gps.
    pub maps: RRBTree<Map<C>, C>,

    pub path: Option<Path<C>>,

    pub interests: Option<Interests<C>>,

    // TODO Utiliser les hauteurs pour pondérer les métriques
    pub heights: Option<Heights<C>>,
}

impl<C: CacheSingleton> Gps<C> {
    pub async fn new<R: Read>(reader: &mut R) -> Result<Gps<C>> {
        let mut gps = Gps {
            maps: RRBTree::<Map<C>, C>::new().await,
            path: None,
            interests: None,
            heights: None,
        };

        // Les appels de benchmark réinitialisent les statistiques de swap-cache aux limites de leur mesure.
        gps.extract(reader).await?;

        Ok(gps)
    }

    pub async fn extract<R: Read>(&mut self, reader: &mut R) -> Result<()> {
        loop {
            let block_type = match reader.read_u8() {
                Ok(block_type) => block_type,
                Err(EOF) => return Ok(()),
                Err(error) => return Err(error),
            };

            self.read_block(reader, block_type).await?;
        }
    }

    async fn read_block<R: Read>(&mut self, reader: &mut R, block_type: u8) -> Result<()> {
        match block_type {
            0 => {
                // Bloc Map
                // TODO Après lecture Map, envisager la construction d’index dérivés sc
                let map = Map::<C>::new(reader).await?;
                self.maps.push(map).await;
            }
            1 => {
                // Les métadonnées Streets ne sont pas prises en charge actuellement, mais la longueur du bloc est préfixée
                // et le bloc doit être lu pour garder les blocs suivants alignés.
                streets::skip(reader)?;
            }
            2 => {
                // Bloc Path
                self.path = Some(Path::<C>::new(reader).await?);
            }
            3 => {
                // Bloc Interests
                self.interests = Some(Interests::<C>::new(reader).await?);
            }
            4 => {
                // Bloc Heights
                // Pour chaque noeud constitutif de ma trace principale une hauteur est associée.
                // La hauteur est un entier signé de 16 bits (i16) exprimé en mètres.
                let path_len = self
                    .path
                    .as_ref()
                    .ok_or("Heights block requires a preceding Path block")?
                    .points
                    .len();

                self.heights = Some(Heights::<C>::new(reader, path_len).await?);
            }
            _ => return Err("unknown GPS block type"),
        }

        Ok(())
    }
}
