use crate::{Result, result::error::EOF};

pub trait Read {
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()>;
}

pub struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &[u8]) -> Reader<'_> {
        Reader { data, offset: 0 }
    }

    pub fn ftell(&self) -> usize {
        self.offset
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl Read for Reader<'_> {
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let end = self.offset.checked_add(buf.len()).ok_or(EOF)?;

        if end > self.len() {
            return Err(EOF);
        }

        buf.copy_from_slice(&self.data[self.offset..end]);
        self.offset = end;

        Ok(())
    }
}

pub trait ByteReader: Read {
    fn read_u8(&mut self) -> Result<u8> {
        let mut buf = [0u8; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn read_u16_le(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_i16_le(&mut self) -> Result<i16> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)?;
        Ok(i16::from_le_bytes(buf))
    }

    fn read_u24_le(&mut self) -> Result<u32> {
        let mut buf = [0u8; 3];
        self.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes([buf[0], buf[1], buf[2], 0]))
    }

    fn read_u32_le(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_f64_le(&mut self) -> Result<f64> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(f64::from_le_bytes(buf))
    }
}

impl<R: Read> ByteReader for R {}
