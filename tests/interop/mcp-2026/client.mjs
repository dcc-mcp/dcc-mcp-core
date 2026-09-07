// Focused response interoperability only, not full MCP 2026 conformance.
import assert from 'node:assert/strict';
import { Client, StreamableHTTPClientTransport } from '@modelcontextprotocol/client';

const url = new URL(process.argv[2]);
assert.equal(url.hostname, '127.0.0.1', 'smoke must use the test-owned loopback server');
const deadline = setTimeout(() => { console.error('SDK smoke deadline exceeded'); process.exit(1); }, 30_000);
try {
  for (const mode of ['auto', { pin: '2026-07-28' }]) {
    const client = new Client(
      { name: 'dcc-response-contract', version: '1' },
      { versionNegotiation: { mode } },
    );
    const transport = new StreamableHTTPClientTransport(url);
    try {
      await client.connect(transport);
      assert.equal(client.getProtocolEra(), 'modern');
      assert.ok(client.getDiscoverResult().supportedVersions.includes('2026-07-28'));
      assert.equal(typeof client.getServerVersion().name, 'string');
      const tools = await client.listTools();
      assert.ok(tools.tools.some((tool) => tool.name === 'search_tools'));
      const result = await client.callTool({ name: 'search_tools', arguments: { query: 'scene' } });
      assert.notEqual(result.isError, true);
      assert.ok(Array.isArray(result.content));
      console.log(JSON.stringify({ mode, era: client.getProtocolEra(), discover: true, list: true, call: true }));
    } finally {
      await client.close();
    }
  }
} finally {
  clearTimeout(deadline);
}
