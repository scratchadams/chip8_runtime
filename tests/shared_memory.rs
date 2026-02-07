use chip8_runtime::shared_memory::shared_memory::SharedMemory;

const PHYS_MEM_SIZE: usize = 0x100000;
const PAGE_SIZE: usize = 0x1000;

#[test]
fn mmap_failure_does_not_leak_pages() {
    let mut mem = SharedMemory::new().unwrap();
    assert!(mem.mmap(257).is_err());
    let pages = mem.mmap(256).unwrap();
    assert_eq!(pages.len(), 256);
}

#[test]
fn write_and_read_bounds_checked() {
    let mut mem = SharedMemory::new().unwrap();
    let data = vec![0xAA];

    assert!(mem.write(PHYS_MEM_SIZE, &data, data.len()).is_err());
    assert!(mem.read(PHYS_MEM_SIZE, 1).is_err());

    let last = PHYS_MEM_SIZE - 1;
    assert!(mem.write(last, &data, data.len()).is_ok());
    let read = mem.read(last, 1).unwrap();
    assert_eq!(read[0], 0xAA);
}

#[test]
fn munmap_frees_pages_for_reuse() {
    let mut mem = SharedMemory::new().unwrap();

    // Allocate 10 pages
    let pages1 = mem.mmap(10).unwrap();
    assert_eq!(pages1.len(), 10);

    // Free them
    assert!(mem.munmap(&pages1).is_ok());

    // Allocate 10 pages again - should reuse the freed pages
    let pages2 = mem.mmap(10).unwrap();
    assert_eq!(pages2.len(), 10);

    // Verify we got the same pages back (may be in different order)
    let mut pages1_sorted = pages1.clone();
    let mut pages2_sorted = pages2.clone();
    pages1_sorted.sort();
    pages2_sorted.sort();
    assert_eq!(pages1_sorted, pages2_sorted);
}

#[test]
fn munmap_coalesces_adjacent_regions() {
    let mut mem = SharedMemory::new().unwrap();

    // Allocate 3 separate regions of 2 pages each
    let region1 = mem.mmap(2).unwrap();  // pages 0-1
    let region2 = mem.mmap(2).unwrap();  // pages 2-3
    let region3 = mem.mmap(2).unwrap();  // pages 4-5

    // Free all three regions (will be coalesced into one 6-page region)
    assert!(mem.munmap(&region1).is_ok());
    assert!(mem.munmap(&region2).is_ok());
    assert!(mem.munmap(&region3).is_ok());

    // Allocate 6 pages - should get them from the coalesced region
    let pages = mem.mmap(6).unwrap();
    assert_eq!(pages.len(), 6);

    // Verify we got the first 6 pages (they should have been coalesced)
    for (i, &page) in pages.iter().enumerate() {
        let expected = (i * PAGE_SIZE) as u32;
        assert!(pages.contains(&expected),
            "Expected page at address {:#X} to be in allocated pages", expected);
    }
}

#[test]
fn munmap_handles_non_contiguous_pages() {
    let mut mem = SharedMemory::new().unwrap();

    // Allocate 5 pages
    let pages1 = mem.mmap(5).unwrap();

    // Allocate 5 more pages (non-contiguous to first allocation)
    let pages2 = mem.mmap(5).unwrap();

    // Free the first set
    assert!(mem.munmap(&pages1).is_ok());

    // Allocate 3 pages - should come from freed pages
    let pages3 = mem.mmap(3).unwrap();
    assert_eq!(pages3.len(), 3);

    // Free the second and third sets
    assert!(mem.munmap(&pages2).is_ok());
    assert!(mem.munmap(&pages3).is_ok());

    // Should be able to allocate all 10 pages again
    let pages4 = mem.mmap(10).unwrap();
    assert_eq!(pages4.len(), 10);
}

#[test]
fn munmap_empty_page_table_succeeds() {
    let mut mem = SharedMemory::new().unwrap();

    // munmap with empty page table should succeed
    assert!(mem.munmap(&[]).is_ok());
}

#[test]
fn munmap_rejects_invalid_page_indices() {
    let mut mem = SharedMemory::new().unwrap();

    // Try to free a page beyond physical memory size
    let invalid_addr = PHYS_MEM_SIZE as u32 + PAGE_SIZE as u32;
    assert!(mem.munmap(&[invalid_addr]).is_err());
}

#[test]
fn free_list_prioritized_over_bitmap_scan() {
    let mut mem = SharedMemory::new().unwrap();

    // Allocate and free the first 10 pages
    let pages1 = mem.mmap(10).unwrap();
    assert!(mem.munmap(&pages1).is_ok());

    // Allocate 5 pages - should come from free_list (first 5 of the freed pages)
    let pages2 = mem.mmap(5).unwrap();
    assert_eq!(pages2.len(), 5);

    // Verify these are from the freed pages (pages 0-4)
    for &page in &pages2 {
        assert!(pages1.contains(&page),
            "Page {:#X} should be from the previously freed pages", page);
    }
}
