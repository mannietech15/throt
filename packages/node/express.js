/**
 * Express middleware for THROT rate limiting.
 */
function defaultKeyGenerator(req) {
  return (
    req.ip ||
    req.headers['x-forwarded-for'] ||
    req.socket?.remoteAddress ||
    'anonymous'
  );
}

function expressMiddleware(options) {
  const {
    engine,
    policy,
    cost = 1,
    keyGenerator = defaultKeyGenerator,
    onRateLimited,
  } = options;

  if (!engine) {
    throw new Error('THROT expressMiddleware requires an initialized `engine` instance.');
  }

  return async function throtMiddleware(req, res, next) {
    try {
      const identity = keyGenerator(req);
      const reqCost = typeof cost === 'function' ? cost(req) : cost;

      const decision = await engine.evaluate({
        identity,
        policy,
        cost: reqCost,
      });

      // Standard IETF / Draft RFC RateLimit headers
      res.setHeader('RateLimit-Limit', decision.limit);
      res.setHeader('RateLimit-Remaining', decision.remaining);
      res.setHeader('RateLimit-Reset', decision.resetAt);

      if (decision.allowed) {
        return next();
      }

      if (decision.retryAfter) {
        res.setHeader('Retry-After', decision.retryAfter);
      }

      if (typeof onRateLimited === 'function') {
        return onRateLimited(req, res, next, decision);
      }

      res.status(429).json({
        statusCode: 429,
        error: 'Too Many Requests',
        message: 'Rate limit quota exceeded. Please slow down.',
        retryAfter: decision.retryAfter,
        resetAt: decision.resetAt,
      });
    } catch (err) {
      // In fail-safe environments, pass unexpected internal errors to Express error handler
      next(err);
    }
  };
}

module.exports = {
  expressMiddleware,
};
