use alloc::string::String;

use bitvec::vec::BitVec;
use protocol::ModuleInfo;

use super::cache::ModuleCache;
use crate::Error;

pub struct ModuleTransfer {
    name: String,
    size: usize,
    chunk_size: usize,
    total_chunks: usize,
    received: BitVec,
}

impl ModuleTransfer {
    pub fn new(meta: &ModuleInfo) -> Self {
        let total_chunks = meta.total_chunks as usize;

        Self {
            name: meta.name.clone(),
            size: meta.size as usize,
            chunk_size: meta.chunk_size as usize,
            total_chunks: meta.total_chunks as usize,
            received: BitVec::repeat(false, total_chunks),
        }
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn is_complete(&self) -> bool {
        self.received.all()
    }

    pub fn add_chunk(
        &mut self,
        cache: &mut ModuleCache,
        index: usize,
        data: &[u8],
    ) -> Result<(), Error> {
        if index >= self.total_chunks {
            return Err(Error::InvalidChunkIndex(index, self.total_chunks));
        }
        if self.received[index] {
            return Err(Error::DuplicateChunk(index));
        }

        let expected_size = if index == self.total_chunks - 1 {
            self.size - self.chunk_size * (self.total_chunks - 1)
        } else {
            self.chunk_size
        };

        if data.len() == expected_size {
            cache.put_slice(&self.name, index * self.chunk_size, data)?;
            self.received.set(index, true);

            log::debug!(
                "Received chunk {} ({}B) for '{}' [{}/{}]",
                index,
                data.len(),
                self.name,
                self.received.count_ones(),
                self.total_chunks,
            );
            Ok(())
        } else {
            Err(Error::InvalidChunkSize(expected_size, data.len()))
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use super::*;

    #[test]
    fn test_add() {
        let meta = ModuleInfo {
            name: String::from("test"),
            size: (3 * 1024 + 512) as u64,
            chunk_size: 1024,
            total_chunks: 4,
        };
        let mut cache = ModuleCache::new(4096);
        let mut transfer = ModuleTransfer::new(&meta);

        cache.put(&meta.name, meta.size as usize).unwrap();
        let data = [
            vec![0u8; 1024],
            vec![1u8; 1024],
            vec![2u8; 1024],
            vec![3u8; 512],
        ];
        for (i, d) in data.iter().enumerate() {
            transfer.add_chunk(&mut cache, i, d).unwrap();
        }

        let assembled = cache.get("test").unwrap();
        assert_eq!(assembled.len(), 3 * 1024 + 512);
        assert!(assembled[..1024].iter().all(|&b| b == 0));
        assert!(assembled[1024..2048].iter().all(|&b| b == 1));
        assert!(assembled[2048..3072].iter().all(|&b| b == 2));
        assert!(assembled[3072..].iter().all(|&b| b == 3));
    }

    #[test]
    fn test_out_of_order() {
        let meta = ModuleInfo {
            name: String::from("test"),
            size: (2 * 1024 + 512) as u64,
            chunk_size: 1024,
            total_chunks: 3,
        };
        let mut cache = ModuleCache::new(4096);
        let mut transfer = ModuleTransfer::new(&meta);

        cache.put(&meta.name, meta.size as usize).unwrap();
        transfer.add_chunk(&mut cache, 2, &vec![2u8; 512]).unwrap();
        transfer.add_chunk(&mut cache, 1, &vec![1u8; 1024]).unwrap();
        transfer.add_chunk(&mut cache, 0, &vec![0u8; 1024]).unwrap();

        let assembled = cache.get("test").unwrap();
        assert_eq!(assembled.len(), 2 * 1024 + 512);
        assert_eq!(&assembled[0..1024], &vec![0u8; 1024][..]);
        assert_eq!(&assembled[1024..2048], &vec![1u8; 1024][..]);
        assert_eq!(&assembled[2048..], &vec![2u8; 512][..]);
    }

    #[test]
    fn test_invalid_chunk() {
        let meta = ModuleInfo {
            name: String::from("test"),
            size: 1024,
            chunk_size: 1024,
            total_chunks: 1,
        };
        let mut cache = ModuleCache::new(4096);
        let mut transfer = ModuleTransfer::new(&meta);

        cache.put(&meta.name, meta.size as usize).unwrap();
        assert!(transfer.add_chunk(&mut cache, 0, &vec![0u8; 512]).is_err());

        transfer.add_chunk(&mut cache, 0, &vec![0u8; 1024]).unwrap();
        assert!(transfer.add_chunk(&mut cache, 0, &vec![0u8; 1024]).is_err());
    }
}
