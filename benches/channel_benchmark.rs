//! Channel performance benchmarks

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rust_routines::prelude::*;
use tokio::runtime::Runtime;

fn bench_channel_send_recv(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("channel_send_recv", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (tx, mut rx) = channel::<i32>();
                
                go!(move {
                    tx.send(black_box(42)).await.unwrap();
                });
                
                let value = rx.recv().await.unwrap();
                black_box(value);
            })
        })
    });
}

fn bench_channel_throughput(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("channel_throughput_1000", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (tx, mut rx) = channel::<i32>();
                
                let sender = go!(move {
                    for i in 0..1000 {
                        tx.send(black_box(i)).await.unwrap();
                    }
                });
                
                let receiver = go!(move {
                    let mut count = 0;
                    while count < 1000 {
                        let _value = rx.recv().await.unwrap();
                        count += 1;
                    }
                    black_box(count);
                });
                
                sender.join().await.unwrap();
                receiver.join().await.unwrap();
            })
        })
    });
}

fn bench_channel_multiple_senders(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("channel_multiple_senders", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (tx, mut rx) = channel::<i32>();
                
                let mut senders = Vec::new();
                for i in 0..10 {
                    let tx_clone = tx.clone();
                    let sender = go!(move {
                        for j in 0..100 {
                            tx_clone.send(black_box(i * 100 + j)).await.unwrap();
                        }
                    });
                    senders.push(sender);
                }
                
                let receiver = go!(move {
                    let mut count = 0;
                    while count < 1000 {
                        let _value = rx.recv().await.unwrap();
                        count += 1;
                    }
                    black_box(count);
                });
                
                for sender in senders {
                    sender.join().await.unwrap();
                }
                receiver.join().await.unwrap();
            })
        })
    });
}

criterion_group!(
    benches,
    bench_channel_send_recv,
    bench_channel_throughput,
    bench_channel_multiple_senders
);
criterion_main!(benches);
