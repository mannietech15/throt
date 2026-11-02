<p align="center">
  <img src="throt.png" alt="THROT - API Rate Limiting" width="100%" />
</p>

# THROT

> **Control the Flow. Protect the Core.**

THROT is a high-performance, distributed API rate-limiting engine written in Rust. It enables developers to protect microservices and external-facing APIs from traffic spikes and abusive patterns using Redis-backed atomic rate-limiting algorithms and PostgreSQL for persistent policy management.

---

## Key Features

- **High-Performance Asynchronous Core**: Built on Tokio, Axum, and SQLx.
- **Distributed & Atomic**: Guarantees concurrency safety across multiple THROT nodes via atomic Redis Lua scripts synchronized with Redis server time (`redis.call('TIME')`).
- **5 Proven Rate-Limiting Algorithms**:
  - **Fixed Window Counter**: Window-aligned counters with instant reset.
  - **Sliding Window Log**: Timestamp-level rolling window precision.
  - **Sliding Window Counter**: Memory-efficient weighted previous-window approximation.
  - **Token Bucket**: Burst-capable continuous fractional token refill.
  - **Leaky Bucket (GCRA)**: Constant-rate traffic policing meter.
- **Multi-Tenant Architecture**: Manage Applications, cryptographic API keys (SHA-256 hashed), and scoped rate-limit policies.
- **Fail-Safe Reliability**: Configurable `fail_closed` vs `fail_open` modes per policy during infrastructure outages.
- **Production Observability**: Health (`/health`), readiness (`/ready`), and Prometheus metrics (`/metrics`).

---

## Architecture Overview

<p align="center">
  <img src="rate-limit-diagram.png" alt="THROT Architecture Overview" width="100%" />
</p>

```text
               Client Applications
                       │
                       ▼
                 Load Balancer
                       │
         ┌─────────────┼─────────────┐
         ▼             ▼             ▼
      THROT 1       THROT 2       THROT 3
         │             │             │
         └─────────────┼─────────────┘
                       │
             ┌─────────┴─────────┐
             ▼                   ▼
      Redis Cluster        PostgreSQL
  (Atomic State & TTL) (Metadata & Policies)
```

---

---

## Two Integration Modes

THROT is built with a single, highly optimized Rust engine that can be consumed in two distinct ways:

### Comparison: Which Mode Should You Choose?

| Feature              | Mode A: In-Process Engine (`@throt/node`)               | Mode B: Standalone Daemon (`throt-server`)             |
| :------------------- | :------------------------------------------------------ | :----------------------------------------------------- |
| **Primary Use Case** | Node.js microservices, Express/Fastify apps             | Polyglot fleets (Go, Python, Java, Ruby), API Gateways |
| **Communication**    | Direct zero-overhead C-ABI / NAPI-RS bridge             | HTTP / REST API with JSON envelopes                    |
| **Latency**          | **Sub-microsecond** in-memory (~15-50 µs via NAPI)      | Network roundtrip (~1-5 ms depending on VPC)           |
| **Storage Backends** | In-Memory (64-way lock-striped) & Distributed Redis     | Distributed Redis & PostgreSQL                         |
| **Deployment**       | Installed as an npm package (`npm install @throt/node`) | Deployed as a standalone Docker container / process    |
| **Cross-Language**   | JavaScript & TypeScript                                 | Any language capable of issuing HTTP requests          |

---

### Mode A: In-Process Engine (`@throt/node`)

Install the package directly into your Node.js application:

```bash
npm install @throt/node
```

Precompiled native binaries are bundled for Linux (x64/ARM64), macOS (Intel/Apple Silicon), and Windows (x64). Developers do not need Rust, Cargo, or a C compiler installed.

#### Basic Usage

```ts
import { ThrotEngine } from "@throt/node";

// Initialize ultra-fast in-memory engine (or pass { redisUrl: 'redis://localhost:6379' } for cluster Redis)
const engine = await ThrotEngine.create();

// Register a rate-limiting policy
engine.addPolicy({
  name: "login-tier",
  algorithm: "token_bucket",
  requestLimit: 10,
  burstCapacity: 10,
  refillRate: 2.0, // 2 tokens replenished per second
  windowSeconds: 60,
  failureMode: "fail_closed",
});

// Evaluate incoming requests
const decision = await engine.evaluate({
  identity: "user:192.168.1.1",
  policy: "login-tier",
  cost: 1,
});

if (decision.allowed) {
  console.log(`Allowed! Remaining quota: ${decision.remaining}`);
} else {
  console.log(`Rate limited! Retry after ${decision.retryAfter}s`);
}
```

#### Express Middleware

```ts
import express from "express";
import { ThrotEngine, expressMiddleware } from "@throt/node";

const app = express();
const engine = await ThrotEngine.create();

engine.addPolicy({
  name: "public-api",
  algorithm: "sliding_window_counter",
  requestLimit: 100,
  windowSeconds: 60,
});

app.use(
  "/api",
  expressMiddleware({
    engine,
    policy: "public-api",
    keyGenerator: (req) => req.headers["x-api-key"] || req.ip,
  }),
);

app.get("/api/data", (req, res) => {
  res.json({ message: "Hello from rate-protected endpoint!" });
});
```

#### Fastify Plugin

```ts
import Fastify from "fastify";
import { ThrotEngine, fastifyRateLimit } from "@throt/node";

const app = Fastify();
const engine = await ThrotEngine.create();

engine.addPolicy({
  name: "fastify-tier",
  algorithm: "token_bucket",
  requestLimit: 60,
  windowSeconds: 60,
});

app.register(fastifyRateLimit, {
  engine,
  policy: "fastify-tier",
});
```

---

### Mode B: Standalone HTTP Daemon (`throt-server`)

For polyglot infrastructure or centralized API gateways, deploy THROT as an independent containerized service:

```bash
docker compose up -d
```

Applications communicate over HTTP:

```bash
curl -X POST http://localhost:8080/v1/limits/evaluate \
  -H "Content-Type: application/json" \
  -H "X-API-Key: <your-api-key>" \
  -d '{
    "identity": "client:ip_10.0.0.1",
    "policy": "payment_service",
    "cost": 1
  }'
```

---

## Workspace Structure

The project is structured as a modular Cargo & npm workspace:

```text
throt/
├── crates/
│   ├── throt-core/         # Language-independent domain models & RateLimitEngine
│   ├── throt-algorithms/   # 5 rate limiting algorithms, Lua scripts, and lock-striped backend
│   ├── throt-storage/      # Redis connection manager & PostgreSQL metadata pool
│   ├── throt-api/          # Standalone HTTP REST API & Axum handlers
│   ├── throt-server/       # Standalone daemon binary entrypoint
│   └── throt-napi/         # Native C-ABI / NAPI-RS bridge with dedicated Tokio runtime
├── packages/
│   └── node/               # @throt/node package, Express & Fastify integrations, TypeScript declarations
├── examples/               # Practical integration examples (Express, Fastify, HTTP client)
├── benches/                # Algorithm microbenchmarks & Node.js NAPI-RS integration benchmarks
├── migrations/             # SQLx database schema migrations
├── docker-compose.yml      # Local development environment (Postgres + Redis + THROT)
└── Dockerfile              # Multi-stage production container build
```

### Prerequisites

- [Rust](https://rustup.rs/) (v1.80+ or latest stable)
- [Docker](https://docs.docker.com/get-docker/) & Docker Compose

### 1. Launch Dependencies

```bash
docker compose up -d throt-postgres throt-redis
```

### 2. Configure Environment

Copy the example configuration:

```bash
cp .env.example .env
```

### 3. Run THROT

```bash
cargo run -p throt-server
```

The service will bind to `http://localhost:8080`.

---

## API Reference

All requests to `/v1/*` (except app creation) require authentication via `X-API-Key: <key>` or `Authorization: Bearer <key>`.

### 1. Register Application

```bash
curl -X POST http://localhost:8080/v1/apps \
  -H "Content-Type: application/json" \
  -d '{"name": "Billing Service"}'
```

Response (contains plaintext API key, displayed only once):

```json
{
  "success": true,
  "data": {
    "application": {
      "id": "1e14ff2b-cf0c-4028-a40d-c0ce93309e4f",
      "name": "Billing Service",
      "status": "active",
      "created_at": "2026-11-01T23:50:00Z",
      "updated_at": "2026-11-01T23:50:00Z"
    },
    "api_key": {
      "id": "c1f7a28e-5b1b-417d-94ea-2d7c71629864",
      "application_id": "1e14ff2b-cf0c-4028-a40d-c0ce93309e4f",
      "raw_key": "throt_live_9d4f29a038c...",
      "key_prefix": "throt_live_9d4f",
      "created_at": "2026-11-01T23:50:00Z"
    }
  }
}
```

### 2. Create Rate Limit Policy

```bash
curl -X POST http://localhost:8080/v1/policies \
  -H "Content-Type: application/json" \
  -H "X-API-Key: throt_live_9d4f29a038c..." \
  -d '{
    "name": "standard-api",
    "algorithm": "token_bucket",
    "request_limit": 100,
    "window_seconds": 60,
    "burst_capacity": 120,
    "refill_rate": 2.0,
    "failure_mode": "fail_closed"
  }'
```

### 3. Evaluate Rate Limit Decision

```bash
curl -X POST http://localhost:8080/v1/limits/evaluate \
  -H "Content-Type: application/json" \
  -H "X-API-Key: throt_live_9d4f29a038c..." \
  -d '{
    "identity": "user:12345",
    "policy": "standard-api",
    "cost": 1
  }'
```

Allowed Response (`200 OK`):

```json
{
  "allowed": true,
  "limit": 120,
  "remaining": 119,
  "reset_at": 1790000060
}
```

Rejected Response (`200 OK`):

```json
{
  "allowed": false,
  "limit": 120,
  "remaining": 0,
  "reset_at": 1790000060,
  "retry_after": 15
}
```

---

## Health & Observability Endpoints

- **Liveness Probe**:

  ```bash
  curl http://localhost:8080/health
  # {"status":"pass","timestamp":1790000000}
  ```

- **Readiness Probe**:

  ```bash
  curl http://localhost:8080/ready
  # {"ready":true,"redis":"ok","postgres":"ok","timestamp":1790000000}
  ```

- **Prometheus Metrics**:
  ```bash
  curl http://localhost:8080/metrics
  # throt_evaluations_total{algorithm="TokenBucket",allowed="true"} 42
  # throt_evaluations_fallback_total{mode="FailClosed"} 0
  ```

---

## Running Tests & Benchmarks

### Test Suite

Run Rust unit, integration, and Redis concurrency tests:

```bash
cargo test --workspace
```

Run Node.js NAPI-RS native binding tests:

```bash
npm test
```

### Performance Benchmarks

#### 1. Core Rust Algorithm Latency (Criterion)

Measured via Criterion with 100 samples per algorithm on a release build:

| Algorithm                  | Mean Latency  | Throughput           | Complexity       | Memory Characteristics                   |
| :------------------------- | :------------ | :------------------- | :--------------- | :--------------------------------------- |
| **Fixed Window**           | **89.34 ns**  | **~11.19 M ops/sec** | $\mathcal{O}(1)$ | Scalar counter, 0 allocations            |
| **Sliding Window Counter** | **89.98 ns**  | **~11.11 M ops/sec** | $\mathcal{O}(1)$ | Weighted approximation, 0 allocations    |
| **Token Bucket**           | **107.74 ns** | **~9.28 M ops/sec**  | $\mathcal{O}(1)$ | Floating replenishment, 0 allocations    |
| **Leaky Bucket (GCRA)**    | **107.71 ns** | **~9.28 M ops/sec**  | $\mathcal{O}(1)$ | Continuous drain, 0 allocations          |
| **Sliding Window Log**     | **485.29 ns** | **~2.06 M ops/sec**  | $\mathcal{O}(N)$ | Rolling vector shift / memory allocation |

Run Criterion microbenchmarks:

```bash
cargo bench -p throt-algorithms
```

#### 2. Node.js NAPI-RS In-Process Throughput

Measured via native Node.js bridge (`benches/node_bench.js`):

| Evaluation Type                        | Latency         | Throughput          |
| :------------------------------------- | :-------------- | :------------------ |
| **Concurrent Async (Batches of 10)**   | **15.72 µs/op** | **~63,629 ops/sec** |
| **Sequential Async (Single-threaded)** | **56.89 µs/op** | **~17,577 ops/sec** |

Run Node.js integration benchmarks:

```bash
npm run bench
```

---

## Supported Platforms & Environments

### Operating Systems & Architectures

Precompiled native packages are distributed via npm optional dependencies for the following targets:

| Platform | Architecture | Binary Package | Verification Status |
| :--- | :--- | :--- | :--- |
| **Linux (glibc)** | `x86_64` | `@throt/node-linux-x64-gnu` | ✅ **Runtime Verified** (Native Ubuntu runner & local suite) |
| **macOS** | `aarch64` (Apple Silicon M1-M4) | `@throt/node-darwin-arm64` | ✅ **Runtime Verified** (Native Apple Silicon `macos-latest` runner) |
| **Windows** | `x86_64` | `@throt/node-win32-x64-msvc` | ✅ **Runtime Verified** (Native Windows Server `windows-latest` runner) |
| **Linux (glibc)** | `aarch64` (ARM64) | `@throt/node-linux-arm64-gnu` | ⚙️ **Cross-Compiled & Link Verified** (`aarch64-linux-gnu-gcc` in CI) |
| **macOS** | `x86_64` (Intel) | `@throt/node-darwin-x64` | ⚙️ **Cross-Compiled & Link Verified** (Xcode Clang targeting x86_64) |

> **Note on Verification**: Platforms marked **Runtime Verified** have successfully loaded the compiled `.node` native addon and passed the complete test suite on native host hardware. Platforms marked **Cross-Compiled & Link Verified** have their shared libraries built and dynamically linked in CI, but have not yet been runtime-tested on physical host hardware.

### Node.js Version Support

THROT's NAPI-RS native bindings target active and current Node.js release lines:

- **Node.js 22.x LTS**: ✅ Verified in automated CI test suite & local development
- **Node.js 24.x LTS**: ✅ Verified in automated CI test suite & local development
- **Node.js 26.x Current**: ✅ Verified in CI test suite (Node-API v9 ABI-stable forward compatible)

*(Note: Node.js 18 and 20 are End-of-Life and are not supported for production deployments).*


---

## Technical Limitations & Considerations

1. **Sliding Window Log Memory Consumption**:
   The sliding window log retains individual request timestamps within the active time window. Under extreme traffic (> 50,000 requests/sec per identity), use **Sliding Window Counter** or **Token Bucket** instead to avoid unbounded timestamp vector growth.

2. **In-Memory vs. Distributed State Scope**:
   In-memory engine mode (`storage: { type: 'memory' }`) maintains local 64-way lock-striped state per OS process. If your application scales horizontally across multiple independent pods or servers, configure `{ redisUrl: 'redis://...' }` to guarantee globally synchronized cluster state.

3. **Cluster Key Hashing in Redis**:
   Single-key Redis hash evaluation is implemented across all 5 algorithms (`throt:swc:{id}:{policy}`), ensuring 100% hash-slot compatibility in Redis Cluster without cross-slot routing errors.

---

## Contributing

We welcome contributions to THROT! Please follow these engineering standards:

1. **Rust Formatting & Linting**:
   ```bash
   cargo fmt --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```
2. **Rust & Concurrency Tests**:
   ```bash
   cargo test --workspace
   ```
3. **Node.js & NAPI-RS Native Tests**:
   ```bash
   npm test
   ```
4. **Commits**: Follow conventional commits (`feat:`, `fix:`, `perf:`, `docs:`, `test:`).

---

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.

