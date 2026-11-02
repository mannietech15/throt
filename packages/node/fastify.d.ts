import type { ThrotEngine, Decision } from './index';

export interface FastifyRateLimitOptions {
  /** The initialized ThrotEngine instance. */
  engine: ThrotEngine;
  /** Explicit policy name to evaluate against. */
  policy?: string;
  /** Static quota cost or dynamic extractor function based on request. */
  cost?: number | ((req: any) => number);
  /** Function to extract client identity (e.g. IP address or API key). */
  keyGenerator?: (req: any) => string;
  /** Custom handler triggered on rejection. */
  onRateLimited?: (req: any, reply: any, decision: Decision) => any;
}

/**
 * Fastify plugin for THROT rate limiting.
 *
 * @example
 * ```ts
 * import Fastify from 'fastify';
 * import { ThrotEngine, fastifyRateLimit } from '@throt/node';
 *
 * const app = Fastify();
 * const engine = await ThrotEngine.create();
 * engine.addPolicy({
 *   name: 'global',
 *   algorithm: 'token_bucket',
 *   requestLimit: 60,
 *   windowSeconds: 60,
 * });
 *
 * app.register(fastifyRateLimit, { engine, policy: 'global' });
 * ```
 */
export function fastifyRateLimit(
  fastify: any,
  options: FastifyRateLimitOptions,
  done: (err?: Error) => void
): void;
