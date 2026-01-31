use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use tempfile::tempdir;

use butterfly_bot::interfaces::providers::MemoryProvider;
use butterfly_bot::providers::sqlite::{SqliteMemoryProvider, SqliteMemoryProviderConfig};

fn bench_memory(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("mem.db");

    let provider = rt.block_on(async {
        SqliteMemoryProvider::new(SqliteMemoryProviderConfig::new(db_path.to_str().unwrap()))
            .await
            .unwrap()
    });

    rt.block_on(async {
        for i in 0..200 {
            let msg = format!("Message {i}");
            provider.append_message("u1", "user", &msg).await.unwrap();
            provider
                .append_message("u1", "assistant", "Acknowledged")
                .await
                .unwrap();
        }
    });

    let mut group = c.benchmark_group("memory");
    group.bench_function(BenchmarkId::new("get_history", 12), |b| {
        b.iter(|| {
            rt.block_on(async {
                let _ = provider.get_history("u1", 12).await.unwrap();
            })
        })
    });

    group.bench_function(BenchmarkId::new("search_fts", 5), |b| {
        b.iter(|| {
            rt.block_on(async {
                let _ = provider.search("u1", "Message 1", 5).await.unwrap();
            })
        })
    });

    group.finish();
}

criterion_group!(benches, bench_memory);
criterion_main!(benches);
