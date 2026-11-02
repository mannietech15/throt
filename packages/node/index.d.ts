/**
 * Supported rate-limiting algorithms in THROT.
 */
export type AlgorithmType =
  | 'fixed_window'
  | 'sliding_window_log'
  | 'sliding_window_counter'
  | 'token_bucket'
  | 'leaky_bucket';

/**
 * Behavior when downstream storage or evaluation encounters a failure.
 */
export type FailureMode = 'fail_closed' | 'fail_open';

/**
 * Configuration for registering a rate-limiting policy.
 */
export interface PolicyConfig {
  /** Unique name of the policy (e.g. 'auth-login', 'tier-standard'). */
  name: string;
  /** Rate-limiting algorithm. */
  algorithm: AlgorithmType;
  /** Maximum number of allowed units within the window. */
  requestLimit: number;
  /** Time window in seconds (default: 60). */
  windowSeconds?: number;
  /** Burst capacity (Token Bucket / Leaky Bucket). Must be >= requestLimit. */
  burstCapacity?: number;
  /** Token replenishment or water leak rate per second. */
  refillRate?: number;
  /** Failure mode fallback behavior ('fail_closed' by default). */
  failureMode?: FailureMode;
}

/**
 * Rate-limit evaluation request input.
 */
export interface EvaluationRequest {
  /** Target client identity (e.g. IP address, user ID, API key). */
  identity: string;
  /** Explicit policy name to evaluate against. If omitted, uses default policy. */
  policy?: string;
  /** Cost/weight of this request in quota units (default: 1). */
  cost?: number;
}

/**
 * Result of rate-limit evaluation.
 */
export interface Decision {
  /** Whether the request is permitted. */
  allowed: boolean;
  /** Total limit quota configured for the matched policy. */
  limit: number;
  /** Remaining quota available in the active window. */
  remaining: number;
  /** Unix timestamp in seconds when the current window resets. */
  resetAt: number;
  /** If rejected, recommended number of seconds to wait before retrying. */
  retryAfter?: number;
}

/**
 * Configuration options for creating a THROT engine.
 */
export interface EngineConfig {
  /** UUID of the application. Auto-generates a UUIDv4 if omitted. */
  applicationId?: string;
  /**
   * Redis connection URL (e.g. 'redis://localhost:6379').
   * If omitted, THROT runs in ultra-high-performance lock-striped in-memory mode.
   */
  redisUrl?: string;
}

/**
 * Native in-process THROT rate-limiting engine.
 */
export class ThrotEngine {
  /**
   * Initializes a new THROT engine.
   *
   * @example
   * ```ts
   * // In-memory mode (ultra-fast, local lock-striped state):
   * const engine = await ThrotEngine.create();
   *
   * // Redis-backed mode (cluster-safe, distributed atomic state):
   * const redisEngine = await ThrotEngine.create({ redisUrl: 'redis://localhost:6379' });
   * ```
   */
  static create(config?: EngineConfig): Promise<ThrotEngine>;

  /**
   * Registers a rate-limiting policy into the engine.
   */
  addPolicy(config: PolicyConfig): void;

  /**
   * Sets the default fallback policy for requests without an explicit policy name.
   */
  setDefaultPolicy(policyName: string): void;

  /**
   * Evaluates an incoming request asynchronously without blocking the Node.js event loop.
   */
  evaluate(request: EvaluationRequest): Promise<Decision>;

  /**
   * Performs a health check against the storage backend.
   */
  isHealthy(): Promise<boolean>;

  /**
   * Returns the UUID of the active application.
   */
  getApplicationId(): string;

  /**
   * Gracefully releases background resources and runtime connections.
   */
  close(): Promise<void>;
}

/**
 * Factory helper for initializing a THROT engine instance.
 */
export function createEngine(config?: EngineConfig): Promise<ThrotEngine>;

export { expressMiddleware } from './express';
export { fastifyRateLimit } from './fastify';
