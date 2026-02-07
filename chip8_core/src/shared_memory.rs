pub mod shared_memory {
    use crate::device::device::{MemoryAllocator, AllocError};

    #[cfg(feature = "std")]
    use std::io::{Error, ErrorKind};

    #[cfg(not(feature = "std"))]
    use alloc::{vec, vec::Vec};

    #[cfg(not(feature = "std"))]
    #[derive(Debug, Clone)]
    pub struct Error {
        message: &'static str,
    }

    #[cfg(not(feature = "std"))]
    #[derive(Debug, Clone, Copy)]
    pub enum ErrorKind {
        InvalidInput,
        OutOfMemory,
        Other,
    }

    #[cfg(not(feature = "std"))]
    impl Error {
        pub fn new(_kind: ErrorKind, message: &'static str) -> Self {
            Error { message }
        }
    }

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
        free_list: Vec<(usize, usize)>, // (base_page_index, count)
    }

    impl SharedMemory {
        pub fn new() -> Result<SharedMemory, Error> {
            Ok(
                SharedMemory {
                    phys_mem: vec![0; PHYS_MEM_SIZE],
                    phys_bitmap: vec![false; PHYS_PAGE_COUNT],
                    free_list: Vec::new(),
                }
            )
        }

        /// mmap returns a list of physical page bases for a process page table.
        /// The returned pages form a contiguous virtual range, but may map to
        /// non-contiguous physical locations.
        /// First-fit strategy: checks free_list for coalesced regions before scanning bitmap.
        pub fn mmap(&mut self, pages: u16) -> Result<Vec<u32>, Error> {
            if pages == 0 {
                return Err(Error::new(ErrorKind::InvalidInput, "page count must be > 0"));
            }

            let needed = pages as usize;
            let mut allocated: Vec<u32> = Vec::with_capacity(needed);

            // First, try to satisfy allocation from free_list
            let mut i = 0;
            while i < self.free_list.len() && allocated.len() < needed {
                let (base, count) = self.free_list[i];
                let take = (needed - allocated.len()).min(count);

                // Allocate from this free region
                for offset in 0..take {
                    let idx = base + offset;
                    self.phys_bitmap[idx] = true;
                    allocated.push((idx * PAGE_SIZE) as u32);
                }

                if take == count {
                    // Consumed entire region, remove it
                    self.free_list.remove(i);
                } else {
                    // Partial consumption, update region
                    self.free_list[i] = (base + take, count - take);
                    i += 1;
                }
            }

            // If free_list didn't satisfy the allocation, scan bitmap for remaining pages
            if allocated.len() < needed {
                // Collect free indices first to avoid borrow checker issues
                let mut free_indices: Vec<usize> = Vec::new();
                for (idx, used) in self.phys_bitmap.iter().enumerate() {
                    if !*used {
                        free_indices.push(idx);
                        if free_indices.len() >= needed - allocated.len() {
                            break;
                        }
                    }
                }

                // Now mark them as used and add to allocated
                for idx in free_indices {
                    self.phys_bitmap[idx] = true;
                    allocated.push((idx * PAGE_SIZE) as u32);
                    if allocated.len() == needed {
                        break;
                    }
                }
            }

            if allocated.len() < needed {
                // Allocation failed, rollback both bitmap AND free_list
                // Use munmap to restore allocator state (handles both bitmap and free_list)
                let _ = self.munmap(&allocated); // Best-effort cleanup, ignore errors
                return Err(Error::new(ErrorKind::OutOfMemory, "insufficient free pages"));
            }

            Ok(allocated)
        }

        /// Free pages back to the allocator, coalescing adjacent free regions.
        /// First-fit strategy: maintains free_list sorted by base index.
        ///
        /// Atomicity: Validates ALL indices before mutating state to ensure
        /// partial failures don't corrupt allocator invariants.
        /// Double-free protection: Deduplicates indices to prevent overlapping free ranges.
        pub fn munmap(&mut self, page_table: &[u32]) -> Result<(), Error> {
            if page_table.is_empty() {
                return Ok(());
            }

            // Convert physical addresses to page indices
            let mut indices: Vec<usize> = page_table
                .iter()
                .map(|&addr| (addr as usize) / PAGE_SIZE)
                .collect();
            indices.sort_unstable();

            // Deduplicate to prevent double-free (overlapping free ranges)
            indices.dedup();

            // Validate ALL indices BEFORE mutating state (atomicity guarantee)
            for &idx in &indices {
                if idx >= PHYS_PAGE_COUNT {
                    return Err(Error::new(ErrorKind::InvalidInput, "page index out of range"));
                }
            }

            // Now safe to mutate: mark pages as free in bitmap
            for &idx in &indices {
                self.phys_bitmap[idx] = false;
            }

            // Add freed regions to free_list with coalescing
            let mut i = 0;
            while i < indices.len() {
                let base = indices[i];
                let mut count = 1;

                // Count contiguous pages
                while i + count < indices.len() && indices[i + count] == base + count {
                    count += 1;
                }

                // Insert into free_list, maintaining sorted order and coalescing
                self.insert_and_coalesce(base, count);

                i += count;
            }

            Ok(())
        }

        /// Insert a free region into free_list, coalescing with adjacent regions.
        fn insert_and_coalesce(&mut self, base: usize, count: usize) {
            let end = base + count;

            // Find insertion point and check for coalescence opportunities
            let mut insert_idx = self.free_list.len();
            let mut coalesce_with_prev = false;
            let mut coalesce_with_next = false;

            for (idx, &(free_base, free_count)) in self.free_list.iter().enumerate() {
                let free_end = free_base + free_count;

                // Check if new region is adjacent to this one
                if free_end == base {
                    // Coalesce with previous region
                    insert_idx = idx;
                    coalesce_with_prev = true;
                } else if end == free_base {
                    // Coalesce with next region
                    if coalesce_with_prev {
                        // Coalesce all three: prev + new + next
                        coalesce_with_next = true;
                        break;
                    } else {
                        insert_idx = idx;
                        coalesce_with_next = true;
                        break;
                    }
                } else if free_base > end && insert_idx == self.free_list.len() {
                    // Found insertion point (maintain sorted order)
                    insert_idx = idx;
                    break;
                }
            }

            if coalesce_with_prev && coalesce_with_next {
                // Merge prev + new + next
                let (prev_base, prev_count) = self.free_list[insert_idx];
                let (_, next_count) = self.free_list[insert_idx + 1];
                self.free_list[insert_idx] = (prev_base, prev_count + count + next_count);
                self.free_list.remove(insert_idx + 1);
            } else if coalesce_with_prev {
                // Merge with previous
                let (prev_base, prev_count) = self.free_list[insert_idx];
                self.free_list[insert_idx] = (prev_base, prev_count + count);
            } else if coalesce_with_next {
                // Merge with next
                let (_next_base, next_count) = self.free_list[insert_idx];
                self.free_list[insert_idx] = (base, count + next_count);
            } else {
                // No coalescing, just insert
                self.free_list.insert(insert_idx, (base, count));
            }
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

    /// Implement MemoryAllocator trait for SharedMemory.
    /// This allows SharedMemory to be used polymorphically with other allocators.
    impl MemoryAllocator for SharedMemory {
        fn mmap(&mut self, pages: u16) -> Result<Vec<u32>, AllocError> {
            // Delegate to existing mmap implementation, converting error types
            SharedMemory::mmap(self, pages).map_err(|_| AllocError::OutOfMemory)
        }

        fn munmap(&mut self, page_table: &[u32]) -> Result<(), AllocError> {
            // Delegate to existing munmap implementation, converting error types
            SharedMemory::munmap(self, page_table).map_err(|_| AllocError::Other)
        }

        fn write(&mut self, addr: usize, data: &[u8]) -> Result<(), AllocError> {
            // Convert slice to Vec for existing write signature
            let data_vec = data.to_vec();
            SharedMemory::write(self, addr, &data_vec, data.len())
                .map_err(|_| AllocError::Other)
        }

        fn read(&mut self, addr: usize, len: usize) -> Result<Vec<u8>, AllocError> {
            SharedMemory::read(self, addr, len).map_err(|_| AllocError::Other)
        }
    }
}
