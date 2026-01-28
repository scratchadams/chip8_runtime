pub mod shared_memory {
    use std::io::{Error, ErrorKind};

    pub const PAGE_SIZE: usize = 0x1000;
    const PHYS_MEM_SIZE: usize = 0x100000;

    const PHYS_PAGE_COUNT: usize = PHYS_MEM_SIZE / PAGE_SIZE;

    /// Our initial strategy for memory allotment uses a page allocator over a
    /// shared physical memory arena. Each process can request multiple pages,
    /// which may map to non-contiguous physical locations.
    /// 
    /// phys_bitmap tracks availability per physical page; it
    /// does not yet support freeing.

    pub struct SharedMemory {
        pub phys_mem: Vec<u8>,
        phys_bitmap: Vec<bool>,
    }

    impl SharedMemory {
        pub fn new() -> Result<SharedMemory, std::io::Error> {
            Ok(
                SharedMemory {
                    phys_mem: vec![0; PHYS_MEM_SIZE],
                    phys_bitmap: vec![false; PHYS_PAGE_COUNT],
                }
            )
        }

        /// mmap returns a list of physical page bases for a process page table.
        /// The returned pages form a contiguous virtual range, but may map to
        /// non-contiguous physical locations.
        /// this allocator is first-fit and does not yet support freeing.
        pub fn mmap(&mut self, pages: u16) -> Result<Vec<u32>, Error> {
            if pages == 0 {
                return Err(Error::new(ErrorKind::InvalidInput, "page count must be > 0"));
            }

            // collect free pages first to avoid partial allocations.
            let mut free_indices: Vec<usize> = Vec::new();
            for (idx, used) in self.phys_bitmap.iter().enumerate() {
                if !*used {
                    free_indices.push(idx);
                    if free_indices.len() == pages as usize {
                        break;
                    }
                }
            }

            if free_indices.len() < pages as usize {
                return Err(Error::new(ErrorKind::OutOfMemory, "insufficient free pages"));
            }

            let mut allocated: Vec<u32> = Vec::with_capacity(pages as usize);
            for idx in free_indices {
                self.phys_bitmap[idx] = true;
                allocated.push((idx * PAGE_SIZE) as u32);
            }

            Ok(allocated)
        }


        /// write will be our primary function for writing data into memory
        /// it will take a mutable reference to the SharedMemory object of 
        /// the system, a virtual address to write data to a vector of 
        /// bytes which will be written at that address, and a length
        /// value which will indicate how many bytes will be written from 
        /// the data vector into memory.
        /// 
        /// We will need some checks here, such as len <= PAGE_SIZE and
        /// if len > data, then use len of data as write length
        /// 
        /// On complete, the write should return a Result that either contains
        /// the length of the write (how many bytes were written to memory)
        /// or an error value.
        /// 
        /// write clamps to data length and bounds-checks
        /// against physical memory size.
        pub fn write(&mut self, addr: usize, data: & Vec<u8>, len: usize) -> Result<(), Error> {
            let write_len = len.min(data.len());
            if write_len > PAGE_SIZE {
                return Err(Error::new(ErrorKind::Other, "write size must not exceed 0x1000 bytes"));
            }

            let end = addr
                .checked_add(write_len)
                .ok_or_else(|| Error::new(ErrorKind::Other, "overflow computing write range"))? as usize;
            if end > self.phys_mem.len() {
                return Err(Error::new(ErrorKind::InvalidInput, "write range out of bounds"));
            }

            self.phys_mem[addr..end].copy_from_slice(&data[..write_len]);

            //println!("Wrote {:X?} to physical address range [{:#X}..{:#X})", data, addr, end);

            Ok(())
        }

        // read clones a byte slice into a new Vec for callers.
        pub fn read(&mut self, addr: usize, len: usize) -> Result<Vec<u8>, Error> {
            let end = addr
                .checked_add(len)
                .ok_or_else(|| Error::new(ErrorKind::Other, "overflow computing read range"))? as usize;
            if end > self.phys_mem.len() {
                return Err(Error::new(ErrorKind::InvalidInput, "read range out of bounds"));
            }

            let mut data:Vec<u8> = Vec::with_capacity(len);
            data.extend_from_slice(&self.phys_mem[addr..end]);

            Ok(data)
        }
    }
}
