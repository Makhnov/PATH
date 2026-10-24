use crate::{
    Result,
    format::reader::{ByteReader, Read},
};

use swap_cache::{cache::CacheSingleton, collections::RRBTree};

pub fn bytes_needed_for(value: usize) -> usize {
    if value <= 1 {
        1
    } else {
        (usize::BITS - value.leading_zeros()).div_ceil(8) as usize
    }
}

// Plus besoin de matcher avant de récupérer la collection ici
pub fn read_compressed_integer<R: Read>(reader: &mut R, bytes: usize) -> Result<usize> {
    match bytes {
        1 => reader.read_u8().map(|value| value as usize),
        2 => reader.read_u16_le().map(|value| value as usize),
        3 => reader.read_u24_le().map(|value| value as usize),
        4 => reader.read_u32_le().map(|value| value as usize),
        _ => Err("invalid compressed integer size"),
    }
}

pub async fn read_compressed_integers<R, C>(
    reader: &mut R,
    bytes: usize,
    n: usize,
) -> Result<RRBTree<usize, C>>
where
    R: Read,
    C: CacheSingleton,
{
    let mut out = RRBTree::<usize, C>::new().await;

    for _ in 0..n {
        let value = read_compressed_integer(reader, bytes)?;
        out.push(value).await;
    }

    Ok(out)
}
