use crate::Result;
use crate::format::reader::{ByteReader, Read};

// Lit un bloc Streets sans le décoder.
// TODO: décoder et exposer Streets si prise en charge des métadonnées des rues
pub(super) fn skip<R: Read>(reader: &mut R) -> Result<()> {
    let full_size = reader.read_u32_le()?;
    let payload_size = full_size
        .checked_sub(size_of::<u32>() as u32)
        .ok_or("invalid Streets block size")? as usize;

    let mut remaining = payload_size;
    let mut scratch = [0u8; 64];
    while remaining != 0 {
        let chunk = remaining.min(scratch.len());
        reader.read_exact(&mut scratch[..chunk])?;
        remaining -= chunk;
    }

    Ok(())
}
