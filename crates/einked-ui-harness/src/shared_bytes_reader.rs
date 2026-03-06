use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

pub struct SharedBytesReader {
    bytes: Arc<[u8]>,
    pos: usize,
}

impl SharedBytesReader {
    pub fn new(bytes: Arc<[u8]>) -> Self {
        Self { bytes, pos: 0 }
    }
}

impl Read for SharedBytesReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.bytes.len().saturating_sub(self.pos);
        if remaining == 0 {
            return Ok(0);
        }
        let n = remaining.min(buf.len());
        buf[..n].copy_from_slice(&self.bytes[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Seek for SharedBytesReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let len = self.bytes.len() as i128;
        let current = self.pos as i128;
        let next = match pos {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::Current(offset) => current + i128::from(offset),
            SeekFrom::End(offset) => len + i128::from(offset),
        };
        if next < 0 || next > len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek out of bounds",
            ));
        }
        self.pos = next as usize;
        Ok(self.pos as u64)
    }
}
