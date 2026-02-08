use criterion::{black_box, criterion_group, criterion_main, Criterion};
use chip8_runtime::shared_memory::shared_memory::SharedMemory;

fn benchmark_mmap(c: &mut Criterion) {
    c.bench_function("mmap_single_page", |b| {
        let mut mem = SharedMemory::new().unwrap();
        b.iter(|| {
            let pages = mem.mmap(black_box(1)).unwrap();
            mem.munmap(&pages).unwrap();
        });
    });

    c.bench_function("mmap_10_pages", |b| {
        let mut mem = SharedMemory::new().unwrap();
        b.iter(|| {
            let pages = mem.mmap(black_box(10)).unwrap();
            mem.munmap(&pages).unwrap();
        });
    });

    c.bench_function("mmap_100_pages", |b| {
        let mut mem = SharedMemory::new().unwrap();
        b.iter(|| {
            let pages = mem.mmap(black_box(100)).unwrap();
            mem.munmap(&pages).unwrap();
        });
    });
}

fn benchmark_munmap(c: &mut Criterion) {
    c.bench_function("munmap_single_page", |b| {
        let mut mem = SharedMemory::new().unwrap();
        b.iter(|| {
            let pages = mem.mmap(1).unwrap();
            mem.munmap(black_box(&pages)).unwrap();
        });
    });

    c.bench_function("munmap_10_pages_contiguous", |b| {
        let mut mem = SharedMemory::new().unwrap();
        b.iter(|| {
            let pages = mem.mmap(10).unwrap();
            mem.munmap(black_box(&pages)).unwrap();
        });
    });

    c.bench_function("munmap_with_coalescing", |b| {
        let mut mem = SharedMemory::new().unwrap();
        b.iter(|| {
            // Allocate 3 regions
            let r1 = mem.mmap(2).unwrap();
            let r2 = mem.mmap(2).unwrap();
            let r3 = mem.mmap(2).unwrap();

            // Free them all (will coalesce)
            mem.munmap(&r1).unwrap();
            mem.munmap(&r2).unwrap();
            mem.munmap(black_box(&r3)).unwrap();

            // Clean up for next iteration
            let all = mem.mmap(6).unwrap();
            mem.munmap(&all).unwrap();
        });
    });
}

fn benchmark_free_list_reuse(c: &mut Criterion) {
    c.bench_function("mmap_from_free_list", |b| {
        let mut mem = SharedMemory::new().unwrap();
        // Pre-populate free_list
        let pages = mem.mmap(10).unwrap();
        mem.munmap(&pages).unwrap();

        b.iter(|| {
            let pages = mem.mmap(black_box(5)).unwrap();
            mem.munmap(&pages).unwrap();
        });
    });
}

criterion_group!(benches, benchmark_mmap, benchmark_munmap, benchmark_free_list_reuse);
criterion_main!(benches);
