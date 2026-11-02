import { ThrotEngine, AlgorithmType, expressMiddleware, fastifyRateLimit } from '../index';
import type { Decision, PolicyConfig } from '../index';

async function testTs() {
  const engine = await ThrotEngine.create();
  
  const policy: PolicyConfig = {
    name: 'ts-test',
    algorithm: 'token_bucket',
    requestLimit: 10,
    burstCapacity: 15,
    refillRate: 2.5,
    windowSeconds: 60,
    failureMode: 'fail_closed',
  };
  
  engine.addPolicy(policy);
  
  const decision: Decision = await engine.evaluate({
    identity: 'ts-user',
    policy: 'ts-test',
    cost: 1,
  });

  const allowed: boolean = decision.allowed;
  const remaining: number = decision.remaining;
  const limit: number = decision.limit;
  const resetAt: number = decision.resetAt;
  const retryAfter: number | undefined = decision.retryAfter;

  console.log({ allowed, remaining, limit, resetAt, retryAfter });
}
