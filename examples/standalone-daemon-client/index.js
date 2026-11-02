/**
 * Example: Interacting with the THROT Standalone HTTP Daemon (Mode B)
 *
 * In this mode, applications communicate with an independently deployed THROT daemon
 * (throt-server) over HTTP without needing local native binaries.
 */
async function evaluateViaHttp(daemonUrl, apiKey, request) {
  const response = await fetch(`${daemonUrl}/v1/limits/evaluate`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-API-Key': apiKey,
    },
    body: JSON.stringify(request),
  });

  if (!response.ok && response.status !== 429) {
    const errorBody = await response.text();
    throw new Error(`HTTP ${response.status}: ${errorBody}`);
  }

  const decision = await response.json();
  return {
    status: response.status,
    headers: {
      limit: response.headers.get('RateLimit-Limit'),
      remaining: response.headers.get('RateLimit-Remaining'),
      reset: response.headers.get('RateLimit-Reset'),
      retryAfter: response.headers.get('Retry-After'),
    },
    decision,
  };
}

async function main() {
  console.log('--- THROT Standalone Daemon Client Example ---');
  console.log('Communicates with POST /v1/limits/evaluate on http://localhost:8080');
}

main().catch(console.error);
