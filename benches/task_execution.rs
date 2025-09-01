use std::thread;

use criterion::{Criterion, criterion_group, criterion_main};
use scoped_thread_pool_std::ThreadPool;

const N_THREADS: usize = 16;
const USE_THREAD_POOL: bool = true;

fn bench_thread_pool(c: &mut Criterion) {
    let thread_pool = ThreadPool::new(N_THREADS);
    c.bench_function("thread_pool", |b| {
        b.iter(|| {
            if USE_THREAD_POOL {
                thread_pool.spawn(|| {
                    let _ = 1 + 1;
                }).join();
            } else {
                thread::spawn(|| {
                    let _ = 1 + 1;
                }).join().unwrap();
            }
        })
    });
}

criterion_group!(benches, bench_thread_pool);
criterion_main!(benches);
