#![deny(clippy::all)]

use napi_derive::napi;
use std::sync::{Arc, OnceLock};
use throt_algorithms::InMemoryRateLimitEvaluator;
use throt_core::{
    AlgorithmType, Decision, EvaluationRequest, FailureMode, InMemoryPolicyStore, RateLimitEngine,
    RateLimitEvaluator, RateLimitPolicy,
};
use throt_storage::{RedisRateLimitEvaluator, RedisStorage};
use uuid::Uuid;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn get_runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .min(8);

        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(threads)
            .enable_all()
            .thread_name("throt-worker")
            .build()
            .expect("Failed to initialize THROT background Tokio runtime")
    })
}

/// JavaScript rate limit policy configuration.
#[napi(object)]
pub struct JsPolicyConfig {
    pub name: String,
    pub algorithm: String,
    pub request_limit: i64,
    pub window_seconds: Option<i64>,
    pub burst_capacity: Option<i64>,
    pub refill_rate: Option<f64>,
    pub failure_mode: Option<String>,
}

/// JavaScript rate limit evaluation request.
#[napi(object)]
pub struct JsEvaluationRequest {
    pub identity: String,
    pub policy: Option<String>,
    pub cost: Option<i64>,
}

/// JavaScript rate limit evaluation decision.
#[napi(object)]
pub struct JsDecision {
    pub allowed: bool,
    pub limit: i64,
    pub remaining: i64,
    pub reset_at: i64,
    pub retry_after: Option<i64>,
}

impl From<Decision> for JsDecision {
    fn from(d: Decision) -> Self {
        Self {
            allowed: d.allowed,
            limit: d.limit as i64,
            remaining: d.remaining as i64,
            reset_at: d.reset_at,
            retry_after: d.retry_after.map(|r| r as i64),
        }
    }
}

/// Configuration options for initializing the THROT engine.
#[napi(object)]
pub struct JsEngineConfig {
    pub application_id: Option<String>,
    pub redis_url: Option<String>,
}

/// Native in-process THROT rate-limiting engine.
#[napi]
pub struct ThrotEngine {
    application_id: Uuid,
    policy_store: Arc<InMemoryPolicyStore>,
    engine: Arc<RateLimitEngine>,
    redis_storage: Option<RedisStorage>,
}

#[napi]
impl ThrotEngine {
    /// Creates a new in-process THROT engine instance (in-memory or Redis-backed).
    #[napi(factory)]
    pub async fn create(config: Option<JsEngineConfig>) -> napi::Result<Self> {
        let conf = config.unwrap_or(JsEngineConfig {
            application_id: None,
            redis_url: None,
        });

        let application_id = match conf.application_id {
            Some(id_str) => Uuid::parse_str(&id_str).map_err(|e| {
                napi::Error::from_reason(format!("Invalid UUID for application_id: {e}"))
            })?,
            None => Uuid::new_v4(),
        };

        let policy_store = Arc::new(InMemoryPolicyStore::new());

        let (evaluator, redis_storage): (Arc<dyn RateLimitEvaluator>, Option<RedisStorage>) =
            if let Some(url) = conf.redis_url {
                let storage = get_runtime()
                    .spawn(async move { RedisStorage::connect(&url).await })
                    .await
                    .map_err(|e| napi::Error::from_reason(format!("Join error: {e}")))?
                    .map_err(|e| {
                        napi::Error::from_reason(format!("Redis connection failed: {e}"))
                    })?;

                (
                    Arc::new(RedisRateLimitEvaluator::new(storage.clone())),
                    Some(storage),
                )
            } else {
                (Arc::new(InMemoryRateLimitEvaluator::new()), None)
            };

        let engine = Arc::new(RateLimitEngine::new(policy_store.clone(), evaluator));

        Ok(Self {
            application_id,
            policy_store,
            engine,
            redis_storage,
        })
    }

    /// Registers a rate-limiting policy.
    #[napi]
    pub fn add_policy(&self, config: JsPolicyConfig) -> napi::Result<()> {
        let algo = match config.algorithm.to_lowercase().as_str() {
            "fixed_window" => AlgorithmType::FixedWindow,
            "sliding_window_log" => AlgorithmType::SlidingWindowLog,
            "sliding_window_counter" => AlgorithmType::SlidingWindowCounter,
            "token_bucket" => AlgorithmType::TokenBucket,
            "leaky_bucket" => AlgorithmType::LeakyBucket,
            other => {
                return Err(napi::Error::from_reason(format!(
                    "Unknown algorithm: '{other}'. Supported: fixed_window, sliding_window_log, sliding_window_counter, token_bucket, leaky_bucket"
                )))
            }
        };

        let failure_mode = match config.failure_mode.as_deref().unwrap_or("fail_closed") {
            "fail_open" => FailureMode::FailOpen,
            _ => FailureMode::FailClosed,
        };

        let mut builder = RateLimitPolicy::builder(config.name, algo)
            .application_id(self.application_id)
            .request_limit(config.request_limit as u64)
            .failure_mode(failure_mode);

        if let Some(w) = config.window_seconds {
            builder = builder.window_seconds(w as u64);
        }
        if let Some(b) = config.burst_capacity {
            builder = builder.burst_capacity(b as u64);
        }
        if let Some(r) = config.refill_rate {
            builder = builder.refill_rate(r);
        }

        let policy = builder
            .build()
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        self.policy_store.insert_policy(policy);
        Ok(())
    }

    /// Sets the default fallback policy for unassigned requests.
    #[napi]
    pub fn set_default_policy(&self, policy_name: String) -> napi::Result<()> {
        let store = self.policy_store.clone();
        let app_id = self.application_id;

        // Verify the policy exists in policy_store
        let rt = get_runtime();
        let maybe_policy = rt.block_on(async {
            throt_core::PolicyStore::get_by_name(&*store, app_id, &policy_name).await
        });

        match maybe_policy {
            Ok(Some(policy)) => {
                self.policy_store.set_default_policy(app_id, policy);
                Ok(())
            }
            Ok(None) => Err(napi::Error::from_reason(format!(
                "Policy '{policy_name}' not found. Register it first via .addPolicy()."
            ))),
            Err(e) => Err(napi::Error::from_reason(e.to_string())),
        }
    }

    /// Evaluates a rate-limit request asynchronously without blocking the Node.js event loop.
    #[napi]
    pub async fn evaluate(&self, request: JsEvaluationRequest) -> napi::Result<JsDecision> {
        let app_id = self.application_id;
        let cost = request.cost.unwrap_or(1).max(1) as u64;

        let core_req =
            EvaluationRequest::for_application(app_id, request.identity, request.policy, cost)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        let engine = self.engine.clone();

        let decision = get_runtime()
            .spawn(async move { engine.evaluate(core_req).await })
            .await
            .map_err(|e| napi::Error::from_reason(format!("Evaluation join error: {e}")))?
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        Ok(JsDecision::from(decision))
    }

    /// Checks storage backend connectivity.
    #[napi]
    pub async fn is_healthy(&self) -> napi::Result<bool> {
        if let Some(ref redis) = self.redis_storage {
            let redis_clone = redis.clone();
            let res = get_runtime()
                .spawn(async move { redis_clone.ping().await })
                .await
                .map_err(|e| napi::Error::from_reason(format!("Healthcheck join error: {e}")))?;

            Ok(res.is_ok())
        } else {
            Ok(true) // In-memory backend is always healthy
        }
    }

    /// Returns the active Application UUID string.
    #[napi]
    pub fn get_application_id(&self) -> String {
        self.application_id.to_string()
    }

    /// Gracefully closes background storage connections.
    #[napi]
    pub async fn close(&self) -> napi::Result<()> {
        // Resources drop gracefully when references release
        Ok(())
    }
}
