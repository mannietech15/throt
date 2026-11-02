const { ThrotEngine } = require('../packages/node');

async function benchmark() {
  console.log('--- THROT Native In-Process Engine Benchmark (Node.js) ---');
  const engine = await ThrotEngine.create();

  engine.addPolicy({
    name: 'bench-policy',
    algorithm: 'token_bucket',
    requestLimit: 1_000_000,
    burstCapacity: 1_000_000,
    refillRate: 100_000.0,
    windowSeconds: 60,
  });

  const iterations = 50_000;
  console.log(`Executing ${iterations.toLocaleString()} sequential in-process rate evaluations...`);

  const startSeq = process.hrtime.bigint();
  for (let i = 0; i < iterations; i++) {
    await engine.evaluate({
      identity: 'user:bench_seq',
      policy: 'bench-policy',
      cost: 1,
    });
  }
  const endSeq = process.hrtime.bigint();
  const totalNsSeq = Number(endSeq - startSeq);
  const avgNsSeq = totalNsSeq / iterations;
  const opsSecSeq = (iterations / (totalNsSeq / 1e9)).toFixed(0);

  console.log(`Sequential Latency: ${(avgNsSeq / 1000).toFixed(2)} µs/op (${Number(opsSecSeq).toLocaleString()} ops/sec)`);

  const concurrentBatches = 5_000;
  const batchSize = 10;
  console.log(`Executing ${concurrentBatches * batchSize} concurrent evaluations (batches of ${batchSize})...`);

  const startConc = process.hrtime.bigint();
  for (let b = 0; b < concurrentBatches; b++) {
    const promises = [];
    for (let i = 0; i < batchSize; i++) {
      promises.push(
        engine.evaluate({
          identity: `user:bench_conc_${i}`,
          policy: 'bench-policy',
          cost: 1,
        })
      );
    }
    await Promise.all(promises);
  }
  const endConc = process.hrtime.bigint();
  const totalNsConc = Number(endConc - startConc);
  const totalOps = concurrentBatches * batchSize;
  const avgNsConc = totalNsConc / totalOps;
  const opsSecConc = (totalOps / (totalNsConc / 1e9)).toFixed(0);

  console.log(`Concurrent Latency: ${(avgNsConc / 1000).toFixed(2)} µs/op (${Number(opsSecConc).toLocaleString()} ops/sec)`);

  await engine.close();
}

benchmark().catch(console.error);
