# THROT — SOLUTION.md

## Engineering Specification and Implementation Blueprint

**Product:** THROT  
**Category:** API Rate-Limiting Infrastructure  
**Core Technology:** Rust  
**Primary Objective:** Build a reliable, high-performance, distributed rate-limiting engine that developers can integrate into their applications.

**Tagline:** Control the Flow. Protect the Core.

---

# 1. PROJECT VISION

THROT is a standalone rate-limiting engine designed to help developers control API traffic, enforce request limits, and protect backend services from excessive or abusive request patterns.

The goal is to provide infrastructure that developers can integrate into their existing applications without having to implement rate-limiting algorithms, counter management, distributed coordination, and enforcement logic themselves.

THROT should be designed as a serious backend infrastructure product, not a demonstration project.

The architecture must prioritize:

- Performance.
- Correctness.
- Concurrency safety.
- Reliability.
- Distributed rate limiting.
- Security.
- Maintainability.
- Observability.
- Extensibility.

Do not sacrifice correctness for premature performance optimization.

# 2. CORE ARCHITECTURE

Build THROT as an independent HTTP service.

Applications communicate with THROT through its API. THROT evaluates incoming rate-limit requests and returns an explicit decision indicating whether the original application request should be allowed.

Important architectural distinction:

THROT is responsible for evaluating and enforcing rate limits through its service interface. It is not automatically a reverse proxy and does not intercept arbitrary application traffic.

A client application or integration layer must call THROT before allowing the protected operation to proceed.

The architecture should follow this structure:

```text
THROT
│
├── API Layer
│   ├── HTTP Routes
│   ├── Request Validation
│   ├── Authentication
│   └── Response Serialization
│
├── Core Engine
│   ├── Rate Limit Evaluation
│   ├── Policy Resolution
│   ├── Identity Resolution
│   └── Decision Generation
│
├── Algorithms
│   ├── Fixed Window Counter
│   ├── Sliding Window Log
│   ├── Sliding Window Counter
│   ├── Token Bucket
│   └── Leaky Bucket
│
├── Storage Layer
│   ├── Redis
│   └── PostgreSQL
│
├── Configuration
│   ├── Environment Configuration
│   ├── Rate Limit Policies
│   └── Application Configuration
│
├── Observability
│   ├── Structured Logging
│   ├── Metrics
│   └── Health Checks
│
└── Tests and Benchmarks
```

# 3. TECHNOLOGY STACK

Use the following technologies:

### Core Language

Rust

Use stable Rust and Cargo. Prefer clear, maintainable Rust code over unnecessary abstractions.

### Asynchronous Runtime

Tokio

Use Tokio for asynchronous execution, networking, and concurrent operations.

### HTTP Framework

Axum

Use Axum for the HTTP API, request handling, routing, and middleware where appropriate.

### Database

PostgreSQL

Use PostgreSQL for persistent application information, including:

- Registered applications.
- API keys and their metadata.
- Rate-limit policy definitions.
- Application configurations.
- Administrative information.

Use SQLx for database interactions.

### Distributed Storage

Redis

Use Redis for:

- Rate-limit counters.
- Token bucket state.
- Sliding-window state.
- Temporary distributed state.
- Atomic rate-limit operations.

Redis must be treated as a critical part of the distributed rate-limiting architecture.

### Serialization

Serde

Use Serde for serialization and deserialization.

### Configuration

Use environment variables for deployment configuration.

Use a suitable Rust configuration library if configuration complexity grows.

### Logging and Observability

Use:

- Tracing.
- Tracing Subscriber.
- Prometheus-compatible metrics.

### Testing

Use:

- Rust's built-in test framework.
- Tokio test utilities.
- Integration tests.
- Property-based testing where useful.
- Criterion for benchmarking.

Choose maintained and compatible library versions. Do not invent dependencies or use abandoned packages.

# 4. RATE-LIMITING ALGORITHMS

Implement the algorithms as independent, testable components.

Each algorithm should expose a consistent interface to the core engine.

## 4.1 Fixed Window Counter

Allow a configured number of requests within a fixed time interval.

Example:

- Limit: 100 requests.
- Window: 60 seconds.
- Identity: user_123.

After the limit is reached, reject additional requests until the next window.

Consider boundary behavior and counter expiration carefully.

## 4.2 Sliding Window Log

Maintain timestamps of requests within a rolling time interval.

Remove expired timestamps and determine whether another request can be accepted.

This algorithm provides precise rolling-window enforcement but can consume more storage for high-volume identities.

Account for memory and Redis storage overhead.

## 4.3 Sliding Window Counter

Use the previous and current fixed-window counters to estimate requests within a rolling window.

This provides a practical compromise between accuracy and storage efficiency.

Implement the weighted calculation correctly and test window transitions.

## 4.4 Token Bucket

Each identity has a bucket containing tokens.

- Tokens are replenished at a configured rate.
- Each accepted request consumes tokens.
- Requests are rejected when insufficient tokens remain.
- The bucket has a maximum capacity.

Use elapsed time to calculate token replenishment.

Handle concurrent updates atomically.

## 4.5 Leaky Bucket

Model requests as a controlled flow with a configured processing or allowance rate.

Define clearly whether the implementation is a policing model or a queue-based traffic-shaping model.

For the initial implementation, prioritize a policing model that makes an allow/reject decision. Do not introduce request queues without a clear requirement.

# 5. DISTRIBUTED RATE LIMITING

This is one of the most important parts of THROT.

A distributed deployment may have several THROT instances handling requests simultaneously.

All instances must enforce shared rate limits consistently.

Example:

```text
                 Applications
                       |
                       v
                 Load Balancer
                       |
              -------------------
              |        |        |
              v        v        v
           THROT 1  THROT 2  THROT 3
              |        |        |
              --------+---------
                       |
                       v
                     Redis
```

Do not rely on process-local counters for distributed enforcement.

Use Redis as the shared state layer.

IMPORTANT:

Operations involving reading state, checking a limit, updating a counter, and writing the result must be atomic.

Use Redis Lua scripts or another suitable atomic Redis operation.

Do not implement rate limiting as separate non-atomic GET and SET operations.

The implementation must account for:

- Concurrent requests.
- Counter expiration.
- Window transitions.
- Token replenishment.
- Atomic state updates.
- Redis connection failures.
- Redis timeouts.
- Retry behavior.
- Duplicate request handling where relevant.

Prefer Redis server-side time for distributed time-sensitive calculations where practical. Avoid depending entirely on application-server clocks, since different instances may have clock differences.

Document the consistency guarantees and limitations of every algorithm.

# 6. IDENTITY AND RATE-LIMIT POLICIES

THROT must support different rate limits for different identities.

Possible identities include:

- Application.
- API key.
- User.
- IP address.
- Custom client identifier.
- Endpoint or route.

An identity should be explicitly supplied or derived through a documented integration contract.

Do not blindly trust arbitrary identity values supplied by unauthenticated clients.

For example:

```json
{
  "identity": "user:123",
  "policy": "standard-api",
  "cost": 1
}
```

The engine should resolve the applicable policy and evaluate the request.

Policies should support:

- Maximum requests.
- Time window.
- Algorithm.
- Identity scope.
- Optional request cost.
- Optional burst capacity.
- Policy status.
- Policy ownership.

Use a clear precedence system when multiple policies match an identity.

Document how conflicts are resolved.

Do not silently apply arbitrary policies.

# 7. API DESIGN

Build a versioned REST API.

Use `/v1` as the initial API version.

## 7.1 Rate-Limit Evaluation

Endpoint:

POST /v1/limits/evaluate

Example request:

```json
{
  "identity": "user:123",
  "policy": "standard-api",
  "cost": 1
}
```

Example successful decision:

```json
{
  "allowed": true,
  "limit": 100,
  "remaining": 72,
  "reset_at": 1790000000,
  "retry_after": null
}
```

Example rejected decision:

```json
{
  "allowed": false,
  "limit": 100,
  "remaining": 0,
  "reset_at": 1790000000,
  "retry_after": 15
}
```

These are illustrative response formats. Define and document the final contract before implementation.

The API should communicate rate-limit decisions clearly and consistently.

## 7.2 Policy Management

Provide endpoints for:

- Creating a policy.
- Retrieving a policy.
- Updating a policy.
- Deleting or disabling a policy.
- Listing policies belonging to an application.

Example:

POST /v1/policies

## 7.3 Application Management

Provide endpoints for:

- Registering an application.
- Retrieving application details.
- Updating application information.
- Disabling an application.

## 7.4 API Key Management

Provide endpoints for:

- Creating API keys.
- Listing API key metadata.
- Revoking API keys.
- Rotating API keys where appropriate.

Never return stored API key secrets after their initial creation.

Store API key hashes rather than plaintext secrets.

Use a secure cryptographic hashing approach appropriate for high-entropy API keys.

## 7.5 Health Checks

Provide:

GET /health

GET /ready

The health endpoint should report whether the process is alive.

The readiness endpoint should indicate whether required dependencies are available for the service to perform its intended operations.

Do not expose secrets or sensitive infrastructure details through health endpoints.

# 8. DATABASE DESIGN

Use PostgreSQL for persistent metadata.

Suggested initial tables:

## applications

- id
- name
- status
- created_at
- updated_at

## api_keys

- id
- application_id
- key_hash
- key_prefix
- status
- created_at
- expires_at
- revoked_at

## rate_limit_policies

- id
- application_id
- name
- algorithm
- request_limit
- window_seconds
- burst_capacity
- refill_rate
- status
- created_at
- updated_at

## policy_assignments

- id
- policy_id
- identity_type
- identity_value
- created_at

Use appropriate foreign keys, indexes, unique constraints, and database migrations.

Avoid storing transient request counters in PostgreSQL.

Redis should handle high-frequency rate-limit state.

Review the database design before implementation and document any additional tables that are genuinely required.

# 9. REDIS KEY DESIGN

Design Redis keys carefully to avoid collisions between applications and identities.

Example conceptual structure:

```text
throt:{application_id}:{policy_id}:{identity}
```

Use a consistent key-naming convention.

Ensure that:

- Different applications cannot share counters accidentally.
- Different policies cannot collide.
- Identity values are normalized safely.
- Expiration is applied correctly.
- Key cardinality is considered.
- Sensitive identifiers are not unnecessarily exposed.

Use Redis Cluster-compatible key design if Redis Cluster support is intended.

Do not claim Redis Cluster compatibility until it has been tested.

# 10. ERROR HANDLING AND FAILURE MODES

THROT must handle infrastructure failures deliberately.

Consider:

- Redis unavailable.
- PostgreSQL unavailable.
- Invalid API key.
- Expired API key.
- Unknown policy.
- Invalid request payload.
- Unsupported algorithm.
- Request timeout.
- Internal processing failure.
- Configuration errors.

Never silently allow requests because an internal operation failed.

Define an explicit failure policy.

For rate-limit evaluation, support configurable behavior:

### Fail Closed

If the rate-limit decision cannot be established, reject the protected operation.

### Fail Open

If the rate-limit decision cannot be established, allow the protected operation.

The appropriate choice depends on the application's security and availability requirements.

Make the behavior explicit and observable.

Never confuse a dependency failure with a normal rate-limit rejection.

Use appropriate HTTP status codes and structured error responses.

# 11. SECURITY REQUIREMENTS

Security must be part of the architecture from the beginning.

Implement:

- API key authentication.
- Secure API key generation.
- Hashed API key storage.
- API key revocation.
- Request validation.
- Input size limits.
- Reasonable request timeouts.
- Secure configuration handling.
- Protection against identity spoofing.
- Structured audit events for administrative actions.
- Restricted administrative endpoints.

Do not log API key secrets.

Do not expose internal stack traces in production responses.

Do not trust client-supplied application IDs without authenticating the request and checking ownership.

Ensure administrative policy changes cannot be performed by ordinary application credentials.

# 12. PERFORMANCE AND SCALABILITY

THROT should be designed for high-throughput workloads, but performance claims must be supported by measurements.

Avoid:

- Unnecessary database queries during every evaluation.
- Excessive memory allocation.
- Unbounded in-memory state.
- Blocking operations inside asynchronous request handlers.
- Unnecessary serialization.
- Repeated configuration lookups.
- Non-atomic Redis operations.

Consider caching policy configuration in memory with a clearly defined invalidation or refresh strategy.

Redis should remain the authoritative source of distributed rate-limit state.

Introduce optimizations only after measuring the actual bottlenecks.

# 13. OBSERVABILITY

Provide structured logging and useful metrics.

Suggested metrics:

- Total rate-limit evaluations.
- Allowed requests.
- Rejected requests.
- Evaluation latency.
- Redis operation latency.
- Redis connection failures.
- Policy lookup latency.
- API authentication failures.
- Internal errors.

Metrics should support filtering by relevant dimensions without creating unbounded cardinality.

Do not use arbitrary user IDs or raw IP addresses as unrestricted metric labels.

Provide Prometheus-compatible metrics through an appropriate endpoint.

Use request IDs or trace IDs to correlate logs across components.

# 14. TESTING REQUIREMENTS

Testing is not optional.

Every major component must have meaningful tests.

## Unit Tests

Test:

- Every algorithm.
- Window boundaries.
- Token replenishment.
- Request costs.
- Policy resolution.
- Identity normalization.
- Invalid configurations.
- Reset calculations.

## Integration Tests

Test:

- API key authentication.
- Policy creation and retrieval.
- Rate-limit evaluation.
- Redis persistence.
- PostgreSQL persistence.
- Redis failures.
- Database failures.
- API response contracts.

## Concurrency Tests

Test simultaneous requests against the same identity.

Verify that the configured limit cannot be bypassed through concurrent requests.

For example, if a policy permits 100 requests in a window, a concurrent workload must not accidentally allow more than the algorithm's documented limit.

## Distributed Tests

Run multiple THROT instances against the same Redis instance.

Verify that all instances enforce shared limits correctly.

## Property-Based Tests

Where useful, test invariants such as:

- Remaining requests never become negative.
- Rejected requests do not increase the allowed count.
- Token buckets never exceed their configured capacity.
- Counters expire according to their policy.
- Independent identities do not interfere with each other.

## Benchmarks

Benchmark individual algorithms and complete API evaluation paths.

Measure:

- Throughput.
- Median latency.
- p95 latency.
- p99 latency.
- Memory usage.
- Redis operation overhead.

Document benchmark hardware, configuration, and workload.

Never invent performance numbers.

# 15. PROJECT STRUCTURE

Use a Cargo workspace when it provides genuine architectural value.

Suggested structure:

```text
throt/
│
├── Cargo.toml
├── Cargo.lock
├── .env.example
├── .gitignore
├── README.md
├── SOLUTION.md
│
├── crates/
│   │
│   ├── throt-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── engine.rs
│   │       ├── decision.rs
│   │       ├── identity.rs
│   │       └── policy.rs
│   │
│   ├── throt-algorithms/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── fixed_window.rs
│   │       ├── sliding_log.rs
│   │       ├── sliding_counter.rs
│   │       ├── token_bucket.rs
│   │       └── leaky_bucket.rs
│   │
│   ├── throt-storage/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── redis.rs
│   │       └── postgres.rs
│   │
│   ├── throt-api/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── routes/
│   │       ├── middleware/
│   │       ├── handlers/
│   │       └── response.rs
│   │
│   └── throt-server/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── config.rs
│           └── startup.rs
│
├── migrations/
│
├── tests/
│
├── benchmarks/
│
└── docs/
```

This is a proposed structure, not a requirement to create empty modules immediately.

Do not introduce crates solely to make the repository look sophisticated.

Keep dependencies and module boundaries justified.

# 16. CONFIGURATION

Use environment variables for deployment-specific settings.

Example `.env.example`:

```env
APP_ENV=development
HOST=0.0.0.0
PORT=8080

DATABASE_URL=postgres://user:password@localhost:5432/throt

REDIS_URL=redis://localhost:6379

LOG_LEVEL=info
```

These are placeholders.

Do not commit actual secrets.

Validate required configuration at startup and fail with clear, useful errors when configuration is invalid.

Do not put database table names, usernames, or passwords in environment variables as a substitute for proper database configuration.

# 17. DEVELOPMENT AND DEPLOYMENT

Provide a local development environment using Docker Compose.

It should start:

- THROT.
- Redis.
- PostgreSQL.

Provide:

- Database migrations.
- A development startup guide.
- Environment configuration instructions.
- Test commands.
- Build commands.
- Deployment documentation.

The service should support graceful shutdown.

Handle database and Redis connection lifecycle correctly.

Use Docker multi-stage builds where appropriate.

# 18. IMPLEMENTATION PHASES

Do not attempt to implement everything in one enormous step.

Build incrementally.

## Phase 1: Foundation

- Initialize the Rust project.
- Configure Cargo.
- Set up Tokio and Axum.
- Set up PostgreSQL.
- Set up Redis.
- Establish configuration management.
- Add health and readiness endpoints.
- Create the initial project structure.

Acceptance criteria:

The service starts successfully, connects to its dependencies, validates configuration, and exposes working health endpoints.

## Phase 2: Core Engine

- Define the rate-limit policy model.
- Define the identity model.
- Define the decision response.
- Create a consistent algorithm interface.
- Implement Fixed Window Counter.
- Add comprehensive unit tests.

Acceptance criteria:

The core engine can evaluate requests correctly without depending on HTTP handlers.

## Phase 3: Redis Integration

- Implement Redis-backed state.
- Implement atomic operations.
- Handle expiration.
- Handle concurrent requests.
- Add Redis integration tests.

Acceptance criteria:

Multiple engine instances can share and enforce the same rate-limit state.

## Phase 4: Additional Algorithms

Implement:

- Sliding Window Log.
- Sliding Window Counter.
- Token Bucket.
- Leaky Bucket.

Add tests for algorithm boundaries, concurrency, and correctness.

Acceptance criteria:

Every advertised algorithm has documented semantics and passing tests.

## Phase 5: API and Authentication

- Implement rate-limit evaluation endpoint.
- Implement application management.
- Implement API key management.
- Implement policy management.
- Add authentication and authorization.
- Add validation and consistent error responses.

Acceptance criteria:

An authenticated application can create policies and evaluate rate-limit decisions through the HTTP API.

## Phase 6: Reliability

- Implement explicit fail-open and fail-closed behavior.
- Improve timeout handling.
- Add structured logging.
- Add metrics.
- Add graceful shutdown.
- Test dependency failures.

Acceptance criteria:

Failure behavior is documented, predictable, and tested.

## Phase 7: Performance

- Add benchmarks.
- Profile hot paths.
- Measure Redis overhead.
- Test concurrent workloads.
- Optimize demonstrated bottlenecks.

Acceptance criteria:

Performance measurements are reproducible and documented.

## Phase 8: Production Readiness

- Review authentication.
- Review data isolation.
- Review concurrency correctness.
- Review configuration security.
- Review database indexes.
- Review Redis key expiration.
- Review operational documentation.
- Add deployment instructions.

Acceptance criteria:

The system can be deployed and operated with documented requirements, limitations, and failure behavior.

# 19. IMPORTANT ENGINEERING RULES

Follow these rules throughout development:

1. Do not hardcode application data, API keys, credentials, or rate-limit policies.
2. Do not use mock data as a substitute for real backend functionality.
3. Do not create frontend dashboards before the core service works.
4. Do not claim that an algorithm is distributed-safe without testing its atomic behavior.
5. Do not use process-local counters as the source of truth in a distributed deployment.
6. Do not perform blocking operations inside asynchronous request handlers.
7. Do not swallow errors.
8. Do not expose sensitive internal information in API responses or logs.
9. Do not add unnecessary dependencies.
10. Do not introduce abstractions that make simple operations difficult to understand.
11. Do not claim production readiness merely because the application starts.
12. Do not claim performance or scalability without benchmarks.
13. Write tests alongside implementation, not after the entire project.
14. Document important architectural decisions.
15. Keep the code modular, readable, and maintainable.

# 20. FUTURE FEATURES

These features should not delay the initial engine.

Consider them after the core engine is stable:

- JavaScript SDK.
- Python SDK.
- Developer dashboard.
- Usage analytics.
- Application-level usage reporting.
- Custom rate-limit policies.
- Per-route limits.
- Organization and team management.
- Subscription plans.
- Usage-based billing.
- Webhooks.
- Policy configuration APIs.
- Multiple deployment regions.
- Distributed coordination improvements.

Do not implement billing or subscription complexity before the core rate-limiting functionality is reliable.

# 21. DEFINITION OF DONE

THROT's first serious release should satisfy the following:

- It is written in Rust.
- It runs as an independent HTTP service.
- It uses Redis for shared rate-limit state.
- It uses PostgreSQL for persistent metadata.
- It supports multiple rate-limiting algorithms.
- It supports authenticated applications.
- It supports configurable rate-limit policies.
- It correctly handles concurrent evaluations.
- It works across multiple THROT instances sharing Redis.
- It has predictable dependency failure behavior.
- It exposes health and readiness endpoints.
- It provides structured logs and useful metrics.
- It has unit, integration, concurrency, and distributed tests.
- It has reproducible benchmarks.
- It has clear documentation.
- It does not rely on hardcoded application data or fake backend functionality.

# 22. FINAL INSTRUCTIONS TO THE AI DEVELOPER

Act as a senior Rust systems engineer and backend infrastructure architect.

Your responsibility is to build THROT as a technically sound, maintainable, and reliable rate-limiting engine.

Before writing implementation code:

1. Review this specification.
2. Identify architectural ambiguities and resolve them explicitly.
3. Define the core interfaces.
4. Design the data models.
5. Explain how Redis atomicity will be achieved.
6. Explain how concurrent requests will be handled.
7. Establish the initial project structure.
8. Present the implementation plan.

Then implement the project phase by phase.

Do not generate the entire codebase in one uncontrolled operation.

After each phase:

- Run the relevant tests.
- Fix failures.
- Review the implementation.
- Update documentation.
- Confirm that acceptance criteria are met.
- Explain what was implemented and what remains.

When a requirement is ambiguous, do not silently invent behavior. Document the assumption and choose the simplest technically sound approach.

Prioritize correctness over the number of features.

The final product should be something another experienced backend engineer can inspect, understand, test, extend, and confidently integrate into a real application.

**THROT is infrastructure software. Build it with the discipline that infrastructure software deserves.**
