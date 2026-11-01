# THROT

> **Control the Flow. Protect the Core.**

THROT is a high-performance, distributed API rate-limiting engine written in Rust. It enables developers to protect microservices and external-facing APIs from traffic spikes and abusive patterns using Redis-backed atomic rate-limiting algorithms and PostgreSQL for persistent policy management.

---

## Key Features

- **High-Performance Asynchronous Core**: Built on Tokio, Axum, and SQLx.
- **Distributed & Atomic**: Guarantees concurrency safety across multiple THROT nodes via atomic Redis operations and server-synchronized clocks (`redis.call('TIME')`).
- **5 Proven Rate-Limiting Algorithms**:
  - Fixed Window Counter
  - Sliding Window Log
  - Sliding Window Counter
  - Token Bucket
  - Leaky Bucket (Traffic Policing)
- **Multi-Tenant Architecture**: Manage Applications, cryptographic API keys, and scoped rate-limit policies.
- **Fail-Safe Reliability**: Configurable `fail_closed` vs `fail_open` modes per policy during infrastructure outages.
- **Production Observability**: Health (`/health`), readiness (`/ready`), and Prometheus-compatible metrics (`/metrics`).

---

## Architecture Overview

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

## Workspace Structure

The project is structured as a modular Cargo workspace:

```text
throt/
├── crates/
│   ├── throt-core/         # Domain models (Identity, Policy, Decision, Errors)
│   ├── throt-algorithms/   # Rate limiting algorithm implementations & trait
│   ├── throt-storage/      # Redis connection manager & PostgreSQL pool
│   ├── throt-api/          # Axum routes, middleware, handlers & envelopes
│   └── throt-server/       # Server entrypoint, configuration & graceful shutdown
├── migrations/             # SQLx database schema migrations
├── tests/                  # Integration and concurrency test suite
├── docker-compose.yml      # Local development environment (Postgres + Redis + THROT)
└── Dockerfile              # Multi-stage production container build
```

---

## Quickstart

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

## Health & Readiness Endpoints

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

---

## Running Tests

Execute the workspace test suite:

```bash
cargo test --workspace
```

---

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
