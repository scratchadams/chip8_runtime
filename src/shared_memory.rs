pub mod shared_memory {
    use std::io::{Error, ErrorKind};

    pub const PAGE_SIZE: usize = 0x1000;
    const PHYS_MEM_SIZE: usize = 0x100000;

    macro_rules! set_bit {
        ($value:expr, $bit:expr) => {
            $value |= 1 << $bit;
        };
    }


    macro_rules! is_bit_set {
        ($value:expr, $bit:expr) => {
            ($value & (1 << $bit)) !=0
        };
    }

    /// Our initial strategy for memory allotment will be 1-page per-process
    /// since the existing chip8 emulator allocates 0x1000 bytes of useable 
    /// memory for a process. Therefore our 'shared memory' will be able to 
    /// hold 256 (0x100) processes at a time.
    /// 
    /// each bit in phys_bitmap corresponds to a page of physical memory
    /// page allocators can consult the bitmap to determine which pages
    /// are available to be mapped to a virtual address
    /// Codex generated: phys_bitmap is a simple 1-bit-per-page allocator; it
    /// does not yet support freeing or multi-page reservations.

    pub struct SharedMemory {
        pub phys_mem: Vec<u8>,
        pub page_table_entries: Vec<Vec<u32>>,
        phys_bitmap: u128,
    }

    impl SharedMemory {
        pub fn new() -> Result<SharedMemory, std::io::Error> {
            Ok(
                SharedMemory {
                    phys_mem: vec![0; PHYS_MEM_SIZE],
                    page_table_entries: Vec::new(),
                    phys_bitmap: 0,
                }
            )
        }

        /// mmap will serve as the primary function for returning virtual memory mappings
        /// of PAGE_SIZE. 
        /// 
        /// Takes a number of pages as input and returns the virtual address at the 
        /// that points to the start of the first page. All returned pages will 
        /// be contiguous in regards to the virtual address space however may map
        ///  to non-contiguous 'physical' memory locations if any of the pages fail 
        /// to allocate, an error will be returned
        /// 
        /// Codex generated: current implementation only supports 1 page and
        /// does not yet handle the "no pages available" case.
        pub fn mmap(&mut self, pages: u8) -> Result<u32, Error> {
            let phys_page_count = PHYS_MEM_SIZE / PAGE_SIZE;
            let mut vaddr = 0x0;

            if pages > 1 {
                return Err(Error::new(ErrorKind::AddrInUse, "Only 1 page for now..."));
            }

            for page in 0..phys_page_count {
                if !is_bit_set!(self.phys_bitmap, page) {
                    set_bit!(self.phys_bitmap, page);
                    println!("bitmap: {:0128b}", self.phys_bitmap);

                    vaddr = self.vmmap(page).unwrap();

                    println!("virtual address 0x{:04x}", vaddr);
                    break;
                }
            }

        Ok(vaddr)
        }

        /// vmmap takes a SharedMemory object and an available page,
        /// updates the associated page table entry and returns a
        /// virtual address
        /// Codex generated: the virtual address encodes the pgd/pte index.
        fn vmmap(&mut self, page: usize) -> Result<u32, Error> {
            if page > 0xff {
                return Err(Error::new(ErrorKind::AddrNotAvailable, "Page value too large."));
            }

            let page_table_index = page / 0x10;
            let page_entry_index = page % 0x10;

            if page_entry_index == 0 {
                self.page_table_entries.push(Vec::new());
                self.page_table_entries[page_table_index].push(page as u32 * 0x1000);

                return Ok(self.page_table_entries[page_table_index][page_entry_index]);
            }
            self.page_table_entries[page_table_index]
                .push(page as u32 * 0x1000);


            let vaddr = (page_table_index << 16) | (page_entry_index << 12);

            Ok(vaddr as u32)
            //Ok(mem.page_table_entries[page_table_index][page_entry_index])
        }
        /// vaddr_to_pte should take a vaddr and return a mutable slice
        /// reference to the memory associated with it
        /// Codex generated: this returns the physical page base, not a slice.
        pub fn vaddr_to_pte(&mut self, addr: u32) -> Result<usize, Error> {
            let pgd_idx = (addr >> 16) as usize;
            let pte_idx = ((addr >> 12) & 0x0F) as usize;

            let pte = self
                .page_table_entries
                .get(pgd_idx)
                .ok_or_else(|| Error::new(ErrorKind::Other, "PGD index out of bounds"))?;

            let phys_page = pte
                .get(pte_idx)
                .ok_or_else(|| Error::new(ErrorKind::Other, "PTE index out of bounds"))?;

            let phy_addr = *phys_page as usize;

            let end = phy_addr
                .checked_add(PAGE_SIZE)
                .ok_or_else(|| Error::new(ErrorKind::Other, "overflow computing write range"))?;

            if end > self.phys_mem.len() {
                return Err(Error::new(ErrorKind::Other, "write exceeds physical memory"));
            }

            Ok(phy_addr)
            //Ok(self.phys_mem[phy_addr..end].as_mut())

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
        /// Codex generated: current implementation ignores `len` and writes
        /// all bytes from `data` starting at `addr`.
        pub fn write(&mut self, addr: usize, data: & Vec<u8>, len: usize) -> Result<(), Error> {
            let write_len = len.min(data.len());
            if write_len > 0x1000 {
                return Err(Error::new(ErrorKind::Other, "write size must not exceed 0x1000 bytes"));
            }

            let end = addr
                .checked_add(write_len)
                .ok_or_else(|| Error::new(ErrorKind::Other, "overflow computing write range"))? as usize;

            self.phys_mem[addr..end].copy_from_slice(&data[..write_len]);

            //println!("Wrote {:X?} to physical address range [{:#X}..{:#X})", data, addr, end);

            Ok(())
        }

        // Codex generated: read clones a byte slice into a new Vec for callers.
        pub fn read(&mut self, addr: usize, len: usize) -> Result<Vec<u8>, Error> {
            let mut data:Vec<u8> = Vec::new();
            let end = addr + len as usize;

            data.extend_from_slice(&self.phys_mem[addr..end]);

            Ok(data)
        }
    }
}
