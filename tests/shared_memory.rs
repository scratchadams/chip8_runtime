use chip8_runtime::shared_memory::shared_memory::SharedMemory;

const PHYS_MEM_SIZE: usize = 0x100000;

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
