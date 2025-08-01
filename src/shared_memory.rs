pub mod shared_memory {
    use std::io::{Error, ErrorKind};
    use std::fs;

    const PAGE_SIZE: usize = 0x1000;
    const PHYS_MEM_SIZE: usize = 0x100000;

    macro_rules! set_bit {
        ($value:expr, $bit:expr) => {
            $value |= 1 << $bit;
        };
    }

    macro_rules! clear_bit  {
        ($value:expr, $bit:expr) => {
            $value &= !(1 << $bit);
        };
    }

    macro_rules! toggle_bit {
        ($value:expr, $bit:expr) => {
            $value ^= 1 << $bit;
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

    pub struct SharedMemory {
        phys_mem: Vec<u8>,
        page_table_entries: Vec<Vec<u32>>,
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
        pub fn write(&mut self, addr: u32, data: Vec<u8>) -> Result<(), Error> {
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
                .checked_add(data.len())
                .ok_or_else(|| Error::new(ErrorKind::Other, "overflow computing write range"))?;

            if end > self.phys_mem.len() {
                return Err(Error::new(ErrorKind::Other, "write exceeds physical memory"));
            }

            self.phys_mem[phy_addr..end].copy_from_slice(&data);

            println!("Wrote {:X?} to physical address range [{:#X}..{:#X})", data, phy_addr, end);

            Ok(())
        }

        pub fn read(&mut self, addr: u32, data: &mut Vec<u8>, len: usize) -> Result<(), Error> {
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
                .checked_add(len)
                .ok_or_else(|| Error::new(ErrorKind::Other, "overflow computing write range"))?;

            if end > self.phys_mem.len() {
                return Err(Error::new(ErrorKind::Other, "write exceeds physical memory"));
            }

            data.extend_from_slice(&self.phys_mem[phy_addr..end]);

            Ok(())
        }

        pub fn load_program(&mut self, addr: u32, filename: String) -> Result<(), Error> {
            let program_text = fs::read(filename)?;
            
            let _ = self.write(addr, program_text)?;

            Ok(())
        }


    }
}