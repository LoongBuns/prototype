use alloc::vec::Vec;

use bitvec::vec::BitVec;
use protocol::ModuleMeta;

use crate::Error;

pub struct ModuleTransfer {
    meta: ModuleMeta,
    received_chunks: BitVec,
    chunks_buffer: Vec<u8>,
}

impl ModuleTransfer {
    pub fn new(meta: ModuleMeta) -> Self {
        let total_chunks = meta.total_chunks as usize;
        let total_size = meta.size as usize;

        Self {
            meta,
            received_chunks: BitVec::repeat(false, total_chunks),
            chunks_buffer: vec![0u8; total_size],
        }
    }

    pub fn name(&self) -> &str {
        self.meta.name.as_str()
    }

    pub fn binary(&self) -> Result<&[u8], Error> {
        if !self.received_chunks.all() {
            return Err(Error::InvalidChunk);
        }
        Ok(&self.chunks_buffer)
    }

    pub fn is_complete(&self) -> bool {
        self.received_chunks.all()
    }

    pub fn add_chunk(&mut self, index: u32, data: &[u8]) -> Result<(), Error> {
        let idx = index as usize;
        let total_chunks = self.meta.total_chunks as usize;
        let chunk_size = self.meta.chunk_size as usize;
        let total_size = self.meta.size as usize;

        if idx >= total_chunks || self.received_chunks[idx] {
            return Err(Error::InvalidChunk);
        }

        let expected_size = if idx == total_chunks - 1 {
            total_size - chunk_size * (total_chunks - 1)
        } else {
            chunk_size
        };

        if data.len() != expected_size {
            return Err(Error::InvalidChunk);
        }

        let start = idx * chunk_size;
        let end = start + data.len();
        if end > self.chunks_buffer.len() {
            return Err(Error::InvalidChunk);
        }

        self.chunks_buffer[start..end].copy_from_slice(data);
        self.received_chunks.set(idx, true);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn test_add() {
        let meta = ModuleMeta {
            name: "test".to_string(),
            size: (3 * 1024 + 512) as u64,
            chunk_size: 1024,
            total_chunks: 4,
        };
        let mut transfer = ModuleTransfer::new(meta);

        let data = vec![vec![0u8; 1024], vec![1u8; 1024], vec![2u8; 1024], vec![3u8; 512]];
        for (i, d) in data.iter().enumerate() {
            transfer.add_chunk(i as u32, d).unwrap();
        }

        let assembled = transfer.binary().unwrap();
        assert_eq!(assembled.len(), 3 * 1024 + 512);
        assert!(assembled[..1024].iter().all(|&b| b == 0));
        assert!(assembled[1024..2048].iter().all(|&b| b == 1));
        assert!(assembled[2048..3072].iter().all(|&b| b == 2));
        assert!(assembled[3072..].iter().all(|&b| b == 3));
    }

    #[test]
    fn test_out_of_order() {
        let meta = ModuleMeta {
            name: "test".to_string(),
            size: (2 * 1024 + 512) as u64,
            chunk_size: 1024,
            total_chunks: 3,
        };
        let mut transfer = ModuleTransfer::new(meta);

        transfer.add_chunk(2, &vec![2u8; 512]).unwrap();
        transfer.add_chunk(1, &vec![1u8; 1024]).unwrap();
        transfer.add_chunk(0, &vec![0u8; 1024]).unwrap();

        let assembled = transfer.binary().unwrap();
        assert_eq!(assembled.len(), 2 * 1024 + 512);
        assert_eq!(&assembled[0..1024], &vec![0u8; 1024][..]);
        assert_eq!(&assembled[1024..2048], &vec![1u8; 1024][..]);
        assert_eq!(&assembled[2048..], &vec![2u8; 512][..]);
    }

    #[test]
    fn test_invalid_chunk() {
        let meta = ModuleMeta {
            name: "test".to_string(),
            size: 1024,
            chunk_size: 1024,
            total_chunks: 1,
        };
        let mut transfer = ModuleTransfer::new(meta);

        assert!(transfer.add_chunk(0, &vec![0u8; 512]).is_err());

        transfer.add_chunk(0, &vec![0u8; 1024]).unwrap();
        assert!(transfer.add_chunk(0, &vec![0u8; 1024]).is_err());
    }
}
