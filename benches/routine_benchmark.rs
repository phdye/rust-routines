//! Routine performance benchmarks

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rust_routines::prelude::*;
use tokio::runtime::Runtime;

fn bench_routine_spawn(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("routine_spawn", |b| {
        b.iter(|| {
            rt.block_on(async {
                let handle = go!(async {
                    black_box(42)
                });
                
                let result = handle.join().await.unwrap();
                black_box(result);
            })
        })
    });
}

fn bench_routine_spawn_many(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("routine_spawn_1000", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut handles = Vec::new();
                
                for i in 0..1000 {
                    let handle = go!(async move {
                        black_box(i)
                    });
                    handles.push(handle);
                }
                
                let mut results = Vec::new();
                for handle in handles {
                    let result = handle.join().await.unwrap();
                    results.push(result);
                }
                
                black_box(results);
            })
        })
    });
}

fn bench_routine_yield(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("routine_yield_now", |b| {
        b.iter(|| {
            rt.block_on(async {
                for _i in 0..100 {
                    yield_now().await;
                }
                black_box(());
            })
        })
    });
}

fn bench_routine_computation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("routine_computation", |b| {
        b.iter(|| {
            rt.block_on(async {
                let handle = go!(async {
                    // Simulate some computation
                    let mut sum = 0;
                    for i in 0..1000 {
                        sum += i;
                        if i % 100 == 0 {
                            yield_now().await;
                        }
                    }
                    black_box(sum)
                });
                
                let result = handle.join().await.unwrap();
                black_box(result);
            })
        })
    });
}

fn bench_routine_concurrent_computation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("routine_concurrent_computation", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut handles = Vec::new();
                
                for _i in 0..10 {
                    let handle = go!(async {
                        let mut sum = 0;
                        for j in 0..1000 {
                            sum += j;
                            if j % 100 == 0 {
                                yield_now().await;
                            }
                        }
                        black_box(sum)
                    });
                    handles.push(handle);
                }
                
                let mut results = Vec::new();
                for handle in handles {
                    let result = handle.join().await.unwrap();
                    results.push(result);
                }
                
                black_box(results);
            })
        })
    });
}

criterion_group!(
    benches,
    bench_routine_spawn,
    bench_routine_spawn_many,
    bench_routine_yield,
    bench_routine_computation,
    bench_routine_concurrent_computation
);
criterion_main!(benches);
