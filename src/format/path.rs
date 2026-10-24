use crate::Result;
use crate::format::reader::{ByteReader, Read};

use super::Node;

use swap_cache::{cache::CacheSingleton, collections::RRBTree};

pub struct Path<C: CacheSingleton> {
    pub points: RRBTree<Node, C>,
    pub waypoints: RRBTree<u8, C>,
}

impl<C: CacheSingleton> Path<C> {
    pub async fn new<R: Read>(reader: &mut R) -> Result<Path<C>> {
        // Nombre de points qui constituent le chemin
        let points_number = reader.read_u16_le()? as usize;

        // println!("Nb points: {points_number}");
        let mut points = RRBTree::<Node, C>::new().await;

        for _ in 0..points_number {
            let x = reader.read_f64_le()?;
            let y = reader.read_f64_le()?;
            // println!("X:{x}, Y:{y}");
            points.push(Node::new(x, y)).await;
        }

        // Index des points qui sont des waypoints (bitmap)
        let wp_len = points_number.div_ceil(8);
        let mut waypoints = RRBTree::<u8, C>::new().await;
        for _ in 0..wp_len {
            let byte = reader.read_u8()?;
            waypoints.push(byte).await;
        }

        Ok(Path { points, waypoints })
    }
}
