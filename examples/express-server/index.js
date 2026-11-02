const http = require('http');
const { ThrotEngine, expressMiddleware } = require('../../packages/node');

async function main() {
  const engine = await ThrotEngine.create();

  engine.addPolicy({
    name: 'login-policy',
    algorithm: 'token_bucket',
    requestLimit: 5,
    burstCapacity: 5,
    refillRate: 1.0,
    windowSeconds: 60,
  });

  const middleware = expressMiddleware({
    engine,
    policy: 'login-policy',
    keyGenerator: (req) => req.headers['x-api-key'] || req.socket.remoteAddress || 'anonymous',
  });

  const server = http.createServer((req, res) => {
    // Wrap Express-style middleware in simple HTTP server
    res.status = (code) => { res.statusCode = code; return res; };
    res.json = (data) => {
      res.setHeader('Content-Type', 'application/json');
      res.end(JSON.stringify(data));
    };

    middleware(req, res, () => {
      res.status(200).json({
        success: true,
        message: 'Request allowed through THROT Express middleware',
        timestamp: new Date().toISOString(),
      });
    });
  });

  server.listen(3001, () => {
    console.log('THROT Express Example server listening on http://localhost:3001');
    server.close();
  });
}

main().catch(console.error);
