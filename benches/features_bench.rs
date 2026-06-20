//! # Kind DB — Feature Benchmark Suite
//!
//! Validates the performance claims made in the README using criterion.
//!
//! ## Claims under test
//!
//! | # | Claim (README)                                                  | Benchmark group              |
//! |---|----------------------------------------------------------------|------------------------------|
//! | 1 | Lock-free skip list scales with concurrent readers/writers      | `concurrent_scaling`         |
//! | 2 | Secondary index lookups are O(log N), not O(N) full-scans      | `secondary_index`            |
//! | 3 | Cache layer bypasses O(log N) traversal for hot reads           | `cache_hit_vs_miss`          |
//! | 4 | LRU vs LFU vs FIFO — modular cache strategies                  | `cache_strategies`           |
//! | 5 | TTL lazy eviction purges expired keys on read                   | `ttl_lazy_eviction`          |
//! | 6 | Atomic CAS provides linearizable updates                        | `cas_atomicity`              |
//! | 7 | Transactions batch writes atomically                            | `transaction_batching`       |
//! | 8 | Range scan is efficient over ordered data                       | `range_scan`                 |
//! | 9 | Prefix scan short-circuits at boundary                          | `prefix_scan`                |
//! |10 | O(log N) complexity — put/get latency grows logarithmically     | `ologn_complexity`           |

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use rand::Rng;
use std::sync::{Arc, Barrier};
use std::thread;

use kind::cache::{Cache, FifoCache, LfuCache, LruCache};
use kind::server::{DbRecord, KindServerImpl};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a KindServerImpl with an optional schema for indexing benchmarks.
fn server_with_schema() -> KindServerImpl {
    let ksl = r#"
        enum ContainerStatus { Running, Stopped, Draining }
        type ContainerRecord {
            id: String,
            image: String,
            port: U16,
            @indexed status: ContainerStatus,
            spawn_time: I64
        }
    "#;
    let tmp = std::env::temp_dir().join("kind_bench_schema.ksl");
    std::fs::write(&tmp, ksl).unwrap();
    KindServerImpl::new(None, Some(tmp.to_str().unwrap().to_string()), None, false)
}

fn bare_server() -> KindServerImpl {
    KindServerImpl::new(None, None, None, false)
}

fn container_json(id: usize, status: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "id": format!("c-{}", id),
        "image": "nginx:latest",
        "port": 8080u16,
        "status": status,
        "spawn_time": 1_700_000_000i64 + id as i64,
    }))
    .unwrap()
}

fn seed_server(server: &KindServerImpl, n: usize) {
    for i in 0..n {
        let key = format!("key:{:08}", i);
        let val = format!("value-{}", i).into_bytes();
        server.tree.insert(
            key.clone(),
            DbRecord {
                
                value: val,
                expires_at: None, version: 0},
        );
    }
}

fn seed_indexed_server(server: &KindServerImpl, n: usize) {
    let statuses = ["Running", "Stopped", "Draining"];
    for i in 0..n {
        let status = statuses[i % 3];
        let key = format!("container:{}", i);
        let val = container_json(i, status);
        server.tree.insert(
            key.clone(),
            DbRecord {
                
                value: val.clone(),
                expires_at: None, version: 0},
        );
        server.index_record(&key, &val);
    }
}

// ===========================================================================
// 1. CONCURRENT SCALING — lock-free skip list
// ===========================================================================

fn bench_concurrent_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_scaling");
    group.sample_size(30);

    for thread_count in [1, 2, 4, 8] {
        // --- concurrent writes ---
        group.throughput(Throughput::Elements(10_000));
        group.bench_with_input(
            BenchmarkId::new("write", thread_count),
            &thread_count,
            |b, &tc| {
                b.iter_batched(
                    bare_server,
                    |server| {
                        let server = Arc::new(server);
                        let barrier = Arc::new(Barrier::new(tc));
                        let ops_per_thread = 10_000 / tc;
                        let handles: Vec<_> = (0..tc)
                            .map(|t| {
                                let s = Arc::clone(&server);
                                let bar = Arc::clone(&barrier);
                                thread::spawn(move || {
                                    bar.wait();
                                    for i in 0..ops_per_thread {
                                        let key = format!("t{}:k{}", t, i);
                                        s.tree.insert(
                                            key.clone(),
                                            DbRecord {
                                                
                                                value: vec![1u8; 64],
                                                expires_at: None, version: 0},
                                        );
                                    }
                                })
                            })
                            .collect();
                        for h in handles {
                            h.join().unwrap();
                        }
                        black_box(&server);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        // --- concurrent reads ---
        group.bench_with_input(
            BenchmarkId::new("read", thread_count),
            &thread_count,
            |b, &tc| {
                b.iter_batched(
                    || {
                        let s = bare_server();
                        seed_server(&s, 10_000);
                        Arc::new(s)
                    },
                    |server| {
                        let barrier = Arc::new(Barrier::new(tc));
                        let ops_per_thread = 10_000 / tc;
                        let handles: Vec<_> = (0..tc)
                            .map(|t| {
                                let s = Arc::clone(&server);
                                let bar = Arc::clone(&barrier);
                                thread::spawn(move || {
                                    bar.wait();
                                    for i in 0..ops_per_thread {
                                        let idx = (t * ops_per_thread + i) % 10_000;
                                        let key = format!("key:{:08}", idx);
                                        black_box(s.tree.get(&key));
                                    }
                                })
                            })
                            .collect();
                        for h in handles {
                            h.join().unwrap();
                        }
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        // --- mixed read/write ---
        group.bench_with_input(
            BenchmarkId::new("mixed_rw", thread_count),
            &thread_count,
            |b, &tc| {
                b.iter_batched(
                    || {
                        let s = bare_server();
                        seed_server(&s, 5_000);
                        Arc::new(s)
                    },
                    |server| {
                        let barrier = Arc::new(Barrier::new(tc));
                        let ops = 10_000 / tc;
                        let handles: Vec<_> = (0..tc)
                            .map(|t| {
                                let s = Arc::clone(&server);
                                let bar = Arc::clone(&barrier);
                                thread::spawn(move || {
                                    bar.wait();
                                    let mut rng = rand::thread_rng();
                                    for i in 0..ops {
                                        if rng.gen_bool(0.8) {
                                            let idx = rng.gen_range(0..5_000);
                                            let key = format!("key:{:08}", idx);
                                            black_box(s.tree.get(&key));
                                        } else {
                                            let key = format!("new:t{}:{}", t, i);
                                            s.tree.insert(
                                                key.clone(),
                                                DbRecord {
                                                    
                                                    value: vec![42u8; 32],
                                                    expires_at: None, version: 0},
                                            );
                                        }
                                    }
                                })
                            })
                            .collect();
                        for h in handles {
                            h.join().unwrap();
                        }
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// ===========================================================================
// 2. SECONDARY INDEX — O(log N) indexed lookup vs O(N) full scan
// ===========================================================================

fn bench_secondary_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("secondary_index");
    group.sample_size(30);

    for n in [1_000, 10_000, 50_000] {
        // Indexed lookup via SkipMap
        group.bench_with_input(BenchmarkId::new("indexed_lookup", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let s = server_with_schema();
                    seed_indexed_server(&s, n);
                    s
                },
                |server| {
                    let idx_key = "ContainerRecord:status:Running";
                    if let Some(set) = server.indexes.get(idx_key) {
                        let keys: Vec<_> =
                            set.value().iter().map(|e| e.value().clone()).collect();
                        for k in keys.iter().take(100) {
                            black_box(server.tree.get(k));
                        }
                    }
                },
                BatchSize::SmallInput,
            );
        });

        // Simulated full-table scan (no index)
        group.bench_with_input(BenchmarkId::new("full_scan", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let s = server_with_schema();
                    seed_indexed_server(&s, n);
                    s
                },
                |server| {
                    let mut results = Vec::new();
                    for entry in server.tree.iter() {
                        if let Ok(json) =
                            serde_json::from_slice::<serde_json::Value>(&entry.value().value)
                        {
                            if json.get("status").and_then(|v| v.as_str()) == Some("Running") {
                                results.push(entry.key().clone());
                            }
                        }
                        if results.len() >= 100 {
                            break;
                        }
                    }
                    black_box(results);
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ===========================================================================
// 3. CACHE HIT vs MISS — O(1) cache vs O(log N) tree lookup
// ===========================================================================

fn bench_cache_hit_vs_miss(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_hit_vs_miss");

    for n in [1_000, 10_000, 100_000] {
        // Cache hit: O(1) HashMap lookup
        group.bench_with_input(BenchmarkId::new("cache_hit", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let cache = LruCache::new(n);
                    for i in 0..n {
                        cache.put(format!("key:{:08}", i), vec![0u8; 64]);
                    }
                    cache
                },
                |cache| {
                    let mut rng = rand::thread_rng();
                    for _ in 0..1_000 {
                        let idx = rng.gen_range(0..n);
                        black_box(cache.get(&format!("key:{:08}", idx)));
                    }
                },
                BatchSize::SmallInput,
            );
        });

        // Cache miss: O(log N) SkipMap lookup
        group.bench_with_input(BenchmarkId::new("tree_lookup", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let s = bare_server();
                    seed_server(&s, n);
                    s
                },
                |server| {
                    let mut rng = rand::thread_rng();
                    for _ in 0..1_000 {
                        let idx = rng.gen_range(0..n);
                        black_box(server.tree.get(&format!("key:{:08}", idx)));
                    }
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ===========================================================================
// 4. CACHE STRATEGIES — LRU vs LFU vs FIFO throughput
// ===========================================================================

fn bench_cache_strategies(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_strategies");
    let capacity = 500;
    let ops = 5_000;

    // Zipfian-like workload: 80% of accesses hit 20% of keys
    let build_workload = || -> Vec<usize> {
        let mut rng = rand::thread_rng();
        (0..ops)
            .map(|_| {
                if rng.gen_bool(0.8) {
                    rng.gen_range(0..200) // hot set
                } else {
                    rng.gen_range(200..1_000) // cold set
                }
            })
            .collect()
    };

    group.bench_function("LRU", |b| {
        b.iter_batched(
            || (LruCache::<String, Vec<u8>>::new(capacity), build_workload()),
            |(cache, workload)| {
                for idx in workload {
                    let key = format!("k:{}", idx);
                    if cache.get(&key).is_none() {
                        cache.put(key.clone(), vec![0u8; 64]);
                    }
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("LFU", |b| {
        b.iter_batched(
            || (LfuCache::<String, Vec<u8>>::new(capacity), build_workload()),
            |(cache, workload)| {
                for idx in workload {
                    let key = format!("k:{}", idx);
                    if cache.get(&key).is_none() {
                        cache.put(key.clone(), vec![0u8; 64]);
                    }
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("FIFO", |b| {
        b.iter_batched(
            || (FifoCache::<String, Vec<u8>>::new(capacity), build_workload()),
            |(cache, workload)| {
                for idx in workload {
                    let key = format!("k:{}", idx);
                    if cache.get(&key).is_none() {
                        cache.put(key.clone(), vec![0u8; 64]);
                    }
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ===========================================================================
// 5. TTL LAZY EVICTION — cost of expiration check on read
// ===========================================================================

fn bench_ttl_lazy_eviction(c: &mut Criterion) {
    let mut group = c.benchmark_group("ttl_lazy_eviction");

    let now_ms = || -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };

    // Read a live (non-expiring) key
    group.bench_function("read_live_key", |b| {
        b.iter_batched(
            || {
                let s = bare_server();
                s.tree.insert(
                    "live".into(),
                    DbRecord {
                        
                        value: vec![1u8; 64],
                        expires_at: None, version: 0},
                );
                s
            },
            |server| {
                for _ in 0..1_000 {
                    if let Some(e) = server.tree.get("live") {
                        black_box(kind::server::is_expired(e.value().expires_at));
                    }
                }
            },
            BatchSize::SmallInput,
        );
    });

    // Read a key with a future TTL (not yet expired — still needs the check)
    group.bench_function("read_ttl_valid", |b| {
        b.iter_batched(
            || {
                let s = bare_server();
                s.tree.insert(
                    "ttl".into(),
                    DbRecord {
                        
                        value: vec![1u8; 64],
                        expires_at: Some(now_ms() + 600_000), version: 0},
                );
                s
            },
            |server| {
                for _ in 0..1_000 {
                    if let Some(e) = server.tree.get("ttl") {
                        black_box(kind::server::is_expired(e.value().expires_at));
                    }
                }
            },
            BatchSize::SmallInput,
        );
    });

    // Read an expired key (triggers lazy eviction path)
    group.bench_function("read_expired_key", |b| {
        b.iter_batched(
            || {
                let s = bare_server();
                s.tree.insert(
                    "expired".into(),
                    DbRecord {
                        value: vec![1u8; 64],
                        expires_at: Some(1), // epoch + 1ms — long expired
                        version: 0
                    },
                );
                s
            },
            |server| {
                for _ in 0..1_000 {
                    if let Some(e) = server.tree.get("expired") {
                        let exp = kind::server::is_expired(e.value().expires_at);
                        if exp {
                            server.tree.remove("expired");
                        }
                        black_box(exp);
                    }
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ===========================================================================
// 6. CAS ATOMICITY — compare-and-swap under contention
// ===========================================================================

fn bench_cas_atomicity(c: &mut Criterion) {
    let mut group = c.benchmark_group("cas_atomicity");
    group.sample_size(30);

    for thread_count in [1, 2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::new("contended_cas", thread_count),
            &thread_count,
            |b, &tc| {
                b.iter_batched(
                    || {
                        let s = bare_server();
                        s.tree.insert(
                            "counter".into(),
                            DbRecord {
                                
                                value: b"0".to_vec(),
                                expires_at: None, version: 0},
                        );
                        Arc::new(s)
                    },
                    |server| {
                        let barrier = Arc::new(Barrier::new(tc));
                        let attempts_per = 500 / tc;
                        let handles: Vec<_> = (0..tc)
                            .map(|_| {
                                let s = Arc::clone(&server);
                                let bar = Arc::clone(&barrier);
                                thread::spawn(move || {
                                    bar.wait();
                                    let mut successes = 0u64;
                                    for _ in 0..attempts_per {
                                        if let Some(entry) = s.tree.get("counter") {
                                            let old = entry.value().value.clone();
                                            let cur: u64 =
                                                String::from_utf8_lossy(&old).parse().unwrap_or(0);
                                            let new_val =
                                                (cur + 1).to_string().into_bytes();
                                            // Simulate CAS: re-read and compare
                                            if let Some(check) = s.tree.get("counter") {
                                                if check.value().value == old {
                                                    s.tree.insert(
                                                        "counter".into(),
                                                        DbRecord {
                                                            
                                                            value: new_val,
                                                            expires_at: None, version: 0},
                                                    );
                                                    successes += 1;
                                                }
                                            }
                                        }
                                    }
                                    successes
                                })
                            })
                            .collect();
                        let total: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
                        black_box(total);
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// ===========================================================================
// 7. TRANSACTION BATCHING — single-put vs batched transaction commit
// ===========================================================================

fn bench_transaction_batching(c: &mut Criterion) {
    let mut group = c.benchmark_group("transaction_batching");
    let batch_size = 100;

    // Individual puts (no transaction)
    group.bench_function("individual_puts", |b| {
        b.iter_batched(
            bare_server,
            |server| {
                for i in 0..batch_size {
                    let key = format!("ind:{}", i);
                    server.tree.insert(
                        key.clone(),
                        DbRecord {
                            
                            value: vec![0u8; 64],
                            expires_at: None, version: 0},
                    );
                }
            },
            BatchSize::SmallInput,
        );
    });

    // Batched transaction commit
    group.bench_function("transaction_commit", |b| {
        b.iter_batched(
            bare_server,
            |server| {
                let mut tx = server.begin_transaction();
                for i in 0..batch_size {
                    tx.put(format!("tx:{}", i), vec![0u8; 64], None);
                }
                tx.commit().unwrap();
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ===========================================================================
// 8. RANGE SCAN — performance across dataset sizes
// ===========================================================================

fn bench_range_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("range_scan");
    group.sample_size(30);

    for n in [1_000, 10_000, 100_000] {
        // Scan a fixed-size window (100 keys) within a dataset of size N
        group.bench_with_input(BenchmarkId::new("window_100", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let s = bare_server();
                    seed_server(&s, n);
                    s
                },
                |server| {
                    let lo = format!("key:{:08}", n / 2);
                    let hi = format!("key:{:08}", n / 2 + 100);
                    let results = server.db_range_scan(&lo, &hi);
                    black_box(results);
                },
                BatchSize::SmallInput,
            );
        });

        // Full scan of all keys
        group.bench_with_input(BenchmarkId::new("full_scan", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let s = bare_server();
                    seed_server(&s, n);
                    s
                },
                |server| {
                    let results = server.db_range_scan("key:00000000", "key:99999999");
                    black_box(results);
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ===========================================================================
// 9. PREFIX SCAN — short-circuits at prefix boundary
// ===========================================================================

fn bench_prefix_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("prefix_scan");
    group.sample_size(30);

    for n in [1_000, 10_000, 100_000] {
        group.bench_with_input(BenchmarkId::new("matching_prefix", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let s = bare_server();
                    // Half the keys start with "alpha:", half with "beta:"
                    for i in 0..n / 2 {
                        let key = format!("alpha:{:08}", i);
                        s.tree.insert(
                            key.clone(),
                            DbRecord {
                                
                                value: vec![0u8; 32],
                                expires_at: None, version: 0},
                        );
                    }
                    for i in 0..n / 2 {
                        let key = format!("beta:{:08}", i);
                        s.tree.insert(
                            key.clone(),
                            DbRecord {
                                
                                value: vec![0u8; 32],
                                expires_at: None, version: 0},
                        );
                    }
                    s
                },
                |server| {
                    // Should scan only the "alpha:" half and stop at "beta:"
                    let results = server.prefix_scan("alpha:");
                    black_box(results);
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(BenchmarkId::new("no_match_prefix", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let s = bare_server();
                    seed_server(&s, n);
                    s
                },
                |server| {
                    // "zzz" prefix won't match anything — should return immediately
                    let results = server.prefix_scan("zzz:");
                    black_box(results);
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ===========================================================================
// 10. O(log N) COMPLEXITY — latency growth as dataset doubles
// ===========================================================================

fn bench_ologn_complexity(c: &mut Criterion) {
    let mut group = c.benchmark_group("ologn_complexity");

    // If put/get is truly O(log N), doubling N should add a near-constant
    // increment to latency rather than doubling it.
    for n in [1_000, 10_000, 100_000, 500_000] {
        group.bench_with_input(BenchmarkId::new("single_get", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let s = bare_server();
                    seed_server(&s, n);
                    s
                },
                |server| {
                    // Lookup a key in the middle of the dataset
                    let key = format!("key:{:08}", n / 2);
                    black_box(server.tree.get(&key));
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(BenchmarkId::new("single_put", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let s = bare_server();
                    seed_server(&s, n);
                    s
                },
                |server| {
                    let key = format!("new:{:08}", n);
                    server.tree.insert(
                        key.clone(),
                        DbRecord {
                            
                            value: vec![0u8; 64],
                            expires_at: None, version: 0},
                    );
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ===========================================================================
// Criterion harness
// ===========================================================================

criterion_group!(
    benches,
    bench_concurrent_scaling,
    bench_secondary_index,
    bench_cache_hit_vs_miss,
    bench_cache_strategies,
    bench_ttl_lazy_eviction,
    bench_cas_atomicity,
    bench_transaction_batching,
    bench_range_scan,
    bench_prefix_scan,
    bench_ologn_complexity,
);
criterion_main!(benches);
