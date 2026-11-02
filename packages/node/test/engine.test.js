const test = require('node:test');
const assert = require('node:assert/strict');
const { ThrotEngine, expressMiddleware, fastifyRateLimit } = require('../index');

test('In-memory ThrotEngine basic lifecycle & TokenBucket evaluation', async () => {
  const engine = await ThrotEngine.create();
  assert.ok(engine.getApplicationId(), 'Application ID should exist');
  assert.equal(await engine.isHealthy(), true);

  engine.addPolicy({
    name: 'token-tier',
    algorithm: 'token_bucket',
    requestLimit: 10,
    burstCapacity: 10,
    refillRate: 5.0,
    windowSeconds: 60,
    failureMode: 'fail_closed',
  });

  // Evaluate 6 tokens
  const d1 = await engine.evaluate({
    identity: 'client:test1',
    policy: 'token-tier',
    cost: 6,
  });
  assert.equal(d1.allowed, true);
  assert.equal(d1.limit, 10);
  assert.equal(d1.remaining, 4);

  // Evaluate 5 tokens (should reject since only 4 remain)
  const d2 = await engine.evaluate({
    identity: 'client:test1',
    policy: 'token-tier',
    cost: 5,
  });
  assert.equal(d2.allowed, false);
  assert.ok(d2.retryAfter >= 1, 'retryAfter should be at least 1s');

  await engine.close();
});

test('In-memory FixedWindow boundary condition', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'fixed-exact',
    algorithm: 'fixed_window',
    requestLimit: 5,
    windowSeconds: 60,
  });

  // Exact capacity allocation
  const d1 = await engine.evaluate({
    identity: 'client:exact',
    policy: 'fixed-exact',
    cost: 5,
  });
  assert.equal(d1.allowed, true);
  assert.equal(d1.remaining, 0);

  // Exceeded
  const d2 = await engine.evaluate({
    identity: 'client:exact',
    policy: 'fixed-exact',
    cost: 1,
  });
  assert.equal(d2.allowed, false);
});

test('In-memory SlidingWindowCounter smoothing', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'swc-policy',
    algorithm: 'sliding_window_counter',
    requestLimit: 20,
    windowSeconds: 60,
  });

  const d1 = await engine.evaluate({
    identity: 'client:swc',
    policy: 'swc-policy',
    cost: 10,
  });
  assert.equal(d1.allowed, true);
  assert.equal(d1.remaining, 10);

  const d2 = await engine.evaluate({
    identity: 'client:swc',
    policy: 'swc-policy',
    cost: 10,
  });
  assert.equal(d2.allowed, true);
  assert.equal(d2.remaining, 0);

  const d3 = await engine.evaluate({
    identity: 'client:swc',
    policy: 'swc-policy',
    cost: 1,
  });
  assert.equal(d3.allowed, false);
});

test('In-memory LeakyBucket policing flow', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'lb-policy',
    algorithm: 'leaky_bucket',
    requestLimit: 5,
    burstCapacity: 5,
    refillRate: 2.0,
    windowSeconds: 10,
  });

  const d1 = await engine.evaluate({
    identity: 'client:lb',
    policy: 'lb-policy',
    cost: 5,
  });
  assert.equal(d1.allowed, true);
  assert.equal(d1.remaining, 0);

  const d2 = await engine.evaluate({
    identity: 'client:lb',
    policy: 'lb-policy',
    cost: 1,
  });
  assert.equal(d2.allowed, false);
});

test('Concurrent Node.js requests (100 simultaneous async evaluations)', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'concurrent-node',
    algorithm: 'token_bucket',
    requestLimit: 500,
    burstCapacity: 500,
    refillRate: 100.0,
    windowSeconds: 60,
  });

  const promises = [];
  for (let i = 0; i < 100; i++) {
    const ident = `node_concurrent_${i % 10}`;
    promises.push(
      engine.evaluate({
        identity: ident,
        policy: 'concurrent-node',
        cost: 1,
      })
    );
  }

  const results = await Promise.all(promises);
  assert.equal(results.length, 100);
  for (const r of results) {
    assert.equal(r.allowed, true);
  }
});

test('Express middleware allow and 429 rejection flow', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'express-limiter',
    algorithm: 'fixed_window',
    requestLimit: 2,
    windowSeconds: 60,
  });

  const middleware = expressMiddleware({
    engine,
    policy: 'express-limiter',
    keyGenerator: () => 'express-test-ip',
  });

  // Mock Request 1 (allowed)
  const req1 = {};
  const headers1 = {};
  const res1 = {
    setHeader: (k, v) => { headers1[k] = v; },
  };
  let calledNext1 = false;
  await middleware(req1, res1, () => { calledNext1 = true; });
  assert.equal(calledNext1, true);
  assert.equal(headers1['RateLimit-Remaining'], 1);

  // Mock Request 2 (allowed)
  const req2 = {};
  const headers2 = {};
  const res2 = {
    setHeader: (k, v) => { headers2[k] = v; },
  };
  let calledNext2 = false;
  await middleware(req2, res2, () => { calledNext2 = true; });
  assert.equal(calledNext2, true);
  assert.equal(headers2['RateLimit-Remaining'], 0);

  // Mock Request 3 (rejected with 429)
  const req3 = {};
  const headers3 = {};
  let statusCode = 0;
  let jsonBody = null;
  const res3 = {
    setHeader: (k, v) => { headers3[k] = v; },
    status: (code) => {
      statusCode = code;
      return {
        json: (data) => { jsonBody = data; },
      };
    },
  };
  let calledNext3 = false;
  await middleware(req3, res3, () => { calledNext3 = true; });
  assert.equal(calledNext3, false);
  assert.equal(statusCode, 429);
  assert.equal(jsonBody.error, 'Too Many Requests');
  assert.ok(headers3['Retry-After'] >= 1);
});

test('Fastify plugin allow and 429 rejection hook', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'fastify-limiter',
    algorithm: 'fixed_window',
    requestLimit: 1,
    windowSeconds: 60,
  });

  let registeredHook = null;
  const mockFastify = {
    addHook: (name, fn) => {
      if (name === 'onRequest') registeredHook = fn;
    },
  };

  fastifyRateLimit(mockFastify, {
    engine,
    policy: 'fastify-limiter',
    keyGenerator: () => 'fastify-client',
  }, (err) => {
    assert.ifError(err);
  });

  assert.ok(registeredHook, 'onRequest hook should be registered');

  // Request 1: allowed
  const reply1Headers = {};
  const reply1 = {
    header: (k, v) => { reply1Headers[k] = v; },
  };
  await registeredHook({ ip: '1.2.3.4' }, reply1);
  assert.equal(reply1Headers['RateLimit-Remaining'], 0);

  // Request 2: rejected
  const reply2Headers = {};
  let reply2Status = 0;
  let reply2Body = null;
  const reply2 = {
    header: (k, v) => { reply2Headers[k] = v; },
    status: (s) => {
      reply2Status = s;
      return {
        send: (b) => { reply2Body = b; },
      };
    },
  };
  await registeredHook({ ip: '1.2.3.4' }, reply2);
  assert.equal(reply2Status, 429);
  assert.equal(reply2Body.statusCode, 429);
});

test('Redis-backed ThrotEngine operations (if Redis is running)', async () => {
  try {
    const engine = await ThrotEngine.create({ redisUrl: 'redis://localhost:6379' });
    const isHealthy = await engine.isHealthy();
    assert.equal(isHealthy, true);

    engine.addPolicy({
      name: 'redis-node-test',
      algorithm: 'token_bucket',
      requestLimit: 10,
      burstCapacity: 10,
      refillRate: 10.0,
      windowSeconds: 60,
    });

    const d1 = await engine.evaluate({
      identity: 'redis:node:client',
      policy: 'redis-node-test',
      cost: 5,
    });
    assert.equal(d1.allowed, true);
    assert.equal(d1.remaining, 5);

    await engine.close();
  } catch (err) {
    if (err.message.includes('Redis connection failed')) {
      console.log('Skipping Redis test: Redis server not reachable');
    } else {
      throw err;
    }
  }
});
