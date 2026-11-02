import type { ThrotEngine, Decision } from './index';

export interface ExpressMiddlewareOptions {
  /** The initialized ThrotEngine instance. */
  engine: ThrotEngine;
  /** Explicit policy name to match. */
  policy?: string;
  /** Static quota cost or dynamic extractor function based on request. */
  cost?: number | ((req: any) => number);
  /** Function to extract client identity (e.g. user ID, IP address). */
  keyGenerator?: (req: any) => string;
  /** Custom handler triggered on rejection instead of default 429 response. */
  onRateLimited?: (req: any, res: any, next: any, decision: Decision) => void;
}

/**
 * Returns an Express-compatible rate-limiting middleware.
 *
 * @example
 * ```ts
 * import express from 'express';
 * import { ThrotEngine, expressMiddleware } from '@throt/node';
 *
 * const app = express();
 * const engine = await ThrotEngine.create();
 * engine.addPolicy({
 *   name: 'api',
 *   algorithm: 'sliding_window_counter',
 *   requestLimit: 100,
 *   windowSeconds: 60,
 * });
 *
 * app.use('/api', expressMiddleware({ engine, policy: 'api' }));
 * ```
 */
export function expressMiddleware(
  options: ExpressMiddlewareOptions
): (req: any, res: any, next: any) => Promise<void>;
