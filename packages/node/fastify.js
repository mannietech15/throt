/**
 * Fastify plugin for THROT rate limiting.
 */
function defaultKeyGenerator(req) {
  return req.ip || req.headers['x-forwarded-for'] || req.socket?.remoteAddress || 'anonymous';
}

function fastifyRateLimit(fastify, options, done) {
  const {
    engine,
    policy,
    cost = 1,
    keyGenerator = defaultKeyGenerator,
    onRateLimited,
  } = options;

  if (!engine) {
    return done(new Error('THROT fastifyRateLimit plugin requires an initialized `engine` instance.'));
  }

  fastify.addHook('onRequest', async (req, reply) => {
    const identity = keyGenerator(req);
    const reqCost = typeof cost === 'function' ? cost(req) : cost;

    const decision = await engine.evaluate({
      identity,
      policy,
      cost: reqCost,
    });

    reply.header('RateLimit-Limit', decision.limit);
    reply.header('RateLimit-Remaining', decision.remaining);
    reply.header('RateLimit-Reset', decision.resetAt);

    if (decision.allowed) {
      return;
    }

    if (decision.retryAfter) {
      reply.header('Retry-After', decision.retryAfter);
    }

    if (typeof onRateLimited === 'function') {
      return onRateLimited(req, reply, decision);
    }

    return reply.status(429).send({
      statusCode: 429,
      error: 'Too Many Requests',
      message: 'Rate limit quota exceeded. Please slow down.',
      retryAfter: decision.retryAfter,
      resetAt: decision.resetAt,
    });
  });

  done();
}

module.exports = {
  fastifyRateLimit,
};
