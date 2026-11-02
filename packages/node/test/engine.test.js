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

test('In-memory SlidingWindowLog rolling eviction & precision', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'swl-policy',
    algorithm: 'sliding_window_log',
    requestLimit: 3,
    windowSeconds: 2,
  });

  // Request 1: allowed, remaining 2
  const d1 = await engine.evaluate({ identity: 'client:swl', policy: 'swl-policy', cost: 1 });
  assert.equal(d1.allowed, true);
  assert.equal(d1.limit, 3);
  assert.equal(d1.remaining, 2);

  // Request 2: allowed, remaining 1
  const d2 = await engine.evaluate({ identity: 'client:swl', policy: 'swl-policy', cost: 1 });
  assert.equal(d2.allowed, true);
  assert.equal(d2.remaining, 1);

  // Request 3: allowed, remaining 0
  const d3 = await engine.evaluate({ identity: 'client:swl', policy: 'swl-policy', cost: 1 });
  assert.equal(d3.allowed, true);
  assert.equal(d3.remaining, 0);

  // Request 4: rejected (capacity reached)
  const d4 = await engine.evaluate({ identity: 'client:swl', policy: 'swl-policy', cost: 1 });
  assert.equal(d4.allowed, false);
  assert.ok(d4.retryAfter >= 1, 'retryAfter should be at least 1s');
  assert.ok(d4.resetAt > Math.floor(Date.now() / 1000) - 5);

  // Wait for window expiration (2.1s)
  await new Promise((resolve) => setTimeout(resolve, 2100));

  // Request 5: should now be allowed as old timestamps evicted
  const d5 = await engine.evaluate({ identity: 'client:swl', policy: 'swl-policy', cost: 1 });
  assert.equal(d5.allowed, true);
  assert.equal(d5.remaining, 2);

  await engine.close();
});

test('TokenBucket burst capacity handling and refill', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'burst-policy',
    algorithm: 'token_bucket',
    requestLimit: 10,
    burstCapacity: 20, // Allow burst up to 20
    refillRate: 5.0,
    windowSeconds: 60,
  });

  // Consume burst of 15 tokens (exceeds requestLimit of 10, but within burstCapacity of 20)
  const d1 = await engine.evaluate({
    identity: 'client:burst',
    policy: 'burst-policy',
    cost: 15,
  });
  assert.equal(d1.allowed, true);
  assert.equal(d1.limit, 20); // Effective limit reflects burst capacity
  assert.equal(d1.remaining, 5);

  // Request exceeding remaining 5 tokens
  const d2 = await engine.evaluate({
    identity: 'client:burst',
    policy: 'burst-policy',
    cost: 6,
  });
  assert.equal(d2.allowed, false);
  assert.equal(d2.remaining, 0);
  assert.ok(d2.retryAfter >= 1);
});

test('Cost edge cases: zero, negative, and excessive cost handling', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'cost-policy',
    algorithm: 'fixed_window',
    requestLimit: 10,
    windowSeconds: 60,
  });

  // Zero cost should be clamped safely to 1
  const dZero = await engine.evaluate({
    identity: 'client:cost',
    policy: 'cost-policy',
    cost: 0,
  });
  assert.equal(dZero.allowed, true);
  assert.equal(dZero.remaining, 9);

  // Negative cost should be clamped safely to 1
  const dNeg = await engine.evaluate({
    identity: 'client:cost',
    policy: 'cost-policy',
    cost: -5,
  });
  assert.equal(dNeg.allowed, true);
  assert.equal(dNeg.remaining, 8);

  // Cost exceeding total limit should be immediately rejected with remaining: 0
  const dExceed = await engine.evaluate({
    identity: 'client:cost',
    policy: 'cost-policy',
    cost: 100,
  });
  assert.equal(dExceed.allowed, false);
  assert.equal(dExceed.remaining, 0, 'On rejection remaining is 0');
});

test('Default policy fallback resolution', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'default-tier',
    algorithm: 'fixed_window',
    requestLimit: 3,
    windowSeconds: 60,
  });

  engine.setDefaultPolicy('default-tier');

  // Evaluate without providing policy name
  const d1 = await engine.evaluate({ identity: 'client:unassigned', cost: 1 });
  assert.equal(d1.allowed, true);
  assert.equal(d1.remaining, 2);

  const d2 = await engine.evaluate({ identity: 'client:unassigned', cost: 2 });
  assert.equal(d2.allowed, true);
  assert.equal(d2.remaining, 0);

  const d3 = await engine.evaluate({ identity: 'client:unassigned', cost: 1 });
  assert.equal(d3.allowed, false);
});

test('Validation and error handling', async () => {
  // Invalid Application UUID
  await assert.rejects(
    async () => {
      await ThrotEngine.create({ applicationId: 'invalid-non-uuid' });
    },
    /Invalid UUID/
  );

  const engine = await ThrotEngine.create();

  // Invalid algorithm name
  assert.throws(
    () => {
      engine.addPolicy({
        name: 'bad-algo',
        algorithm: 'non_existent_algorithm',
        requestLimit: 10,
      });
    },
    /Unknown algorithm/
  );

  // Non-existent policy evaluation
  await assert.rejects(
    async () => {
      await engine.evaluate({
        identity: 'client:1',
        policy: 'unregistered-policy',
      });
    },
    /Policy not found/
  );

  // Empty/whitespace identity
  await assert.rejects(
    async () => {
      await engine.evaluate({
        identity: '   ',
        policy: 'default-tier',
      });
    },
    /cannot be empty/
  );
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

test('Redis connection failure handling on unreachable host/port', async () => {
  // Test connecting to an invalid port where Redis is definitely not running
  await assert.rejects(
    async () => {
      await ThrotEngine.create({ redisUrl: 'redis://127.0.0.1:59999' });
    },
    /Redis connection failed/
  );
});

test('Engine runtime lifecycle and graceful cleanup via close()', async () => {
  const engine1 = await ThrotEngine.create();
  const engine2 = await ThrotEngine.create();

  engine1.addPolicy({
    name: 'cleanup-test',
    algorithm: 'fixed_window',
    requestLimit: 5,
    windowSeconds: 60,
  });

  const d = await engine1.evaluate({ identity: 'client:cleanup', policy: 'cleanup-test', cost: 1 });
  assert.equal(d.allowed, true);

  // Close both engines gracefully
  await engine1.close();
  await engine2.close();
  assert.ok(true, 'Engines closed cleanly without leaking background handles');
});

test('Sliding Window Log concurrent evaluations under race conditions', async () => {
  const engine = await ThrotEngine.create();
  engine.addPolicy({
    name: 'swl-concurrent',
    algorithm: 'sliding_window_log',
    requestLimit: 50,
    windowSeconds: 60,
  });

  // Launch 50 concurrent requests for the same identity
  const promises = [];
  for (let i = 0; i < 50; i++) {
    promises.push(
      engine.evaluate({
        identity: 'client:swl_race',
        policy: 'swl-concurrent',
        cost: 1,
      })
    );
  }

  const results = await Promise.all(promises);
  assert.equal(results.length, 50);

  // All 50 should be allowed
  const allowedCount = results.filter((r) => r.allowed).length;
  assert.equal(allowedCount, 50);

  // 51st request should be rejected
  const d51 = await engine.evaluate({
    identity: 'client:swl_race',
    policy: 'swl-concurrent',
    cost: 1,
  });
  assert.equal(d51.allowed, false);
  assert.equal(d51.remaining, 0);

  await engine.close();
});


