use crate::Result;
use crate::format::reader::{ByteReader, Read};

use swap_cache::{cache::CacheSingleton, collections::RRBTree};

pub struct Heights<C: CacheSingleton> {
    pub heights: RRBTree<i16, C>,
}

impl<C: CacheSingleton> Heights<C> {
    pub async fn new<R: Read>(reader: &mut R, len: usize) -> Result<Heights<C>> {
        let mut heights = RRBTree::<i16, C>::new().await;

        for _ in 0..len {
            heights.push(reader.read_i16_le()?).await;
        }

        Ok(Heights { heights })
    }
}
