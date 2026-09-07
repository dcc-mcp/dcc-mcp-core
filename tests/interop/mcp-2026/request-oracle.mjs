// Compare the same negative fixtures against the pinned official HTTP entry.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createMcpHandler, isLegacyRequest, McpServer } from '@modelcontextprotocol/server';

const fixtures = new URL('../../../crates/dcc-mcp-jsonrpc/tests/fixtures/modern_request_cases.json', import.meta.url);
const cases = JSON.parse(await readFile(fixtures, 'utf8'));
const handler = createMcpHandler(() => new McpServer({ name: 'request-oracle', version: '1' }), { legacy: 'reject' });
const deadline = setTimeout(() => { console.error('Request oracle deadline exceeded'); process.exit(1); }, 30_000);
try {
  for (const test of cases) {
    const request = new Request('http://127.0.0.1/mcp', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Accept: 'application/json, text/event-stream', ...test.headers },
      body: JSON.stringify(test.body),
    });
    if (test.route === 'legacy') {
      // Dual-era ingress forwards this body unchanged. A strict modern-only
      // endpoint rejects it later, which would test a different contract.
      assert.equal(await isLegacyRequest(request), true, test.name);
      continue;
    }
    const response = await handler.fetch(request);
    assert.equal(response.status, test.status ?? 400, test.name);
    const body = await response.json();
    assert.equal(body.error.code, test.code, test.name);
    if (test.code === -32022) assert.deepEqual(body.error.data, { supported: ['2026-07-28'], requested: '2099-01-01' });
  }
  console.log(JSON.stringify({ oracle: '@modelcontextprotocol/server@2.0.0', boundaryCases: cases.length, matched: true }));
} finally {
  clearTimeout(deadline);
  await handler.close();
}
