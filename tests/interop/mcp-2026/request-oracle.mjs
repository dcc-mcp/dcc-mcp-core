// Compare the same negative fixtures against the pinned official HTTP entry.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createMcpHandler, isLegacyRequest, McpServer } from '@modelcontextprotocol/server';

const fixtures = new URL('../../../crates/dcc-mcp-jsonrpc/tests/fixtures/modern_request_cases.json', import.meta.url);
const cases = JSON.parse(await readFile(fixtures, 'utf8'));
let invocations = 0;
const handler = createMcpHandler(() => {
  const server = new McpServer({ name: 'request-oracle', version: '1' });
  server.registerTool('search_tools', { description: 'Read-only fixture' }, async () => { invocations++; return { content: [] }; });
  return server;
}, { legacy: 'reject' });
const deadline = setTimeout(() => { console.error('Request oracle deadline exceeded'); process.exit(1); }, 30_000);
const releaseDifferences = [];
try {
  for (const test of cases) {
    const callsBefore = invocations;
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
    if (test.status === 202) {
      assert.equal(response.status, 202, test.name);
      assert.equal(await response.text(), '', test.name);
      continue;
    }
    const body = await response.json();
    if (test.sdk200Difference) {
      // Pin the known published-release difference, not a permissive skip.
      // The Rust contract follows the cited newer official source/spec.
      assert.equal(test.name, 'body-only-modern-claim');
      assert.equal(response.status, 200, test.name);
      assert.equal(body.result?.resultType, 'complete', test.name);
      releaseDifferences.push({ name: test.name, reason: test.sdk200Difference });
      continue;
    }
    assert.equal(response.status, test.status ?? 400, `${test.name}: ${JSON.stringify(body)}`);
    assert.equal(body.error.code, test.code, test.name);
    if (test.noInvocation) assert.equal(invocations, callsBefore, test.name);
    if (test.code === -32022) assert.deepEqual(body.error.data, { supported: ['2026-07-28'], requested: '2099-01-01' });
  }
  console.log(JSON.stringify({ oracle: '@modelcontextprotocol/server@2.0.0', boundaryCases: cases.length,
    matchedCases: cases.length - releaseDifferences.length, releaseDifferences }));
} finally {
  clearTimeout(deadline);
  await handler.close();
}
