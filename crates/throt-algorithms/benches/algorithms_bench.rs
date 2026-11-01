use criterion::{black_box, criterion_group, criterion_main, Criterion};
use throt_algorithms::{
    FixedWindowRateLimiter, LeakyBucketRateLimiter, SlidingCounterRateLimiter,
    SlidingLogRateLimiter, TokenBucketRateLimiter,
};
use throt_core::{AlgorithmType, FailureMode, RateLimitPolicy};
use uuid::Uuid;

fn create_bench_policy(algo: AlgorithmType) -> RateLimitPolicy {
    RateLimitPolicy {
        id: Uuid::new_v4(),
        application_id: Uuid::new_v4(),
        name: "bench".to_string(),
        algorithm: algo,
        request_limit: 1000,
        window_seconds: 60,
        burst_capacity: Some(1000),
        refill_rate: Some(100.0),
        failure_mode: FailureMode::FailClosed,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn bench_algorithms(c: &mut Criterion) {
    let fixed_window = FixedWindowRateLimiter::new();
    let fw_policy = create_bench_policy(AlgorithmType::FixedWindow);

    c.bench_function("fixed_window_eval", |b| {
        let mut time = 1000;
        b.iter(|| {
            time += 1;
            let _ = fixed_window.evaluate_at(
                black_box("user:bench"),
                black_box(1),
                black_box(&fw_policy),
                black_box(time),
            );
        })
    });

    let sliding_counter = SlidingCounterRateLimiter::new();
    let sc_policy = create_bench_policy(AlgorithmType::SlidingWindowCounter);

    c.bench_function("sliding_counter_eval", |b| {
        let mut time = 1000;
        b.iter(|| {
            time += 1;
            let _ = sliding_counter.evaluate_at(
                black_box("user:bench"),
                black_box(1),
                black_box(&sc_policy),
                black_box(time),
            );
        })
    });

    let token_bucket = TokenBucketRateLimiter::new();
    let tb_policy = create_bench_policy(AlgorithmType::TokenBucket);

    c.bench_function("token_bucket_eval", |b| {
        let mut time = 1000.0;
        b.iter(|| {
            time += 0.01;
            let _ = token_bucket.evaluate_at(
                black_box("user:bench"),
                black_box(1),
                black_box(&tb_policy),
                black_box(time),
            );
        })
    });

    let leaky_bucket = LeakyBucketRateLimiter::new();
    let lb_policy = create_bench_policy(AlgorithmType::LeakyBucket);

    c.bench_function("leaky_bucket_eval", |b| {
        let mut time = 1000.0;
        b.iter(|| {
            time += 0.01;
            let _ = leaky_bucket.evaluate_at(
                black_box("user:bench"),
                black_box(1),
                black_box(&lb_policy),
                black_box(time),
            );
        })
    });

    let sliding_log = SlidingLogRateLimiter::new();
    let sl_policy = create_bench_policy(AlgorithmType::SlidingWindowLog);

    c.bench_function("sliding_log_eval", |b| {
        let mut time = 1000000;
        b.iter(|| {
            time += 10;
            let _ = sliding_log.evaluate_at(
                black_box("user:bench"),
                black_box(1),
                black_box(&sl_policy),
                black_box(time),
            );
        })
    });
}

criterion_group!(benches, bench_algorithms);
criterion_main!(benches);
