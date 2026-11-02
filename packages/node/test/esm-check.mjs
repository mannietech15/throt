import { ThrotEngine, expressMiddleware } from '../index.js';
import assert from 'node:assert/strict';

async function main() {
  const engine = await ThrotEngine.create();
  assert.ok(engine.getApplicationId());
  
  engine.addPolicy({
    name: 'esm-policy',
    algorithm: 'fixed_window',
    requestLimit: 5,
    windowSeconds: 60,
  });

  const decision = await engine.evaluate({
    identity: 'esm-user',
    policy: 'esm-policy',
    cost: 1,
  });

  assert.equal(decision.allowed, true);
  assert.equal(decision.remaining, 4);
  console.log('ESM import and execution succeeded cleanly!');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
