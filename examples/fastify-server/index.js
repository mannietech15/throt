const { ThrotEngine, fastifyRateLimit } = require('../../packages/node');

async function main() {
  const engine = await ThrotEngine.create();

  engine.addPolicy({
    name: 'api-v1',
    algorithm: 'sliding_window_counter',
    requestLimit: 100,
    windowSeconds: 60,
  });

  console.log('THROT Fastify Example initialized with policy "api-v1"');
  await engine.close();
}

main().catch(console.error);
