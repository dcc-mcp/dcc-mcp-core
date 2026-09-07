// Public SDK producer against a test-owned native server, not a reimplemented client.
import assert from 'node:assert/strict';
import { Client, StreamableHTTPClientTransport } from '@modelcontextprotocol/client';

const url = new URL(process.argv[2]);
assert.equal(url.hostname, '127.0.0.1');
const deadline = setTimeout(() => { console.error('Parameter SDK deadline exceeded'); process.exit(1); }, 30_000);
try {
  for (const mode of ['auto', { pin: '2026-07-28' }]) {
    const mirrored = [];
    const client = new Client({ name: 'dcc-parameter-contract', version: '1' }, { versionNegotiation: { mode } });
    const transport = new StreamableHTTPClientTransport(url, {
      fetch: async (input, init) => {
        if (typeof init?.body === 'string' && JSON.parse(init.body).method === 'tools/call') {
          mirrored.push(new Headers(init.headers));
        }
        return fetch(input, init);
      },
    });
    try {
      await client.connect(transport);
      const { tools } = await client.listTools();
      const definition = tools.find((tool) => tool.name === 'annotated_echo');
      assert.ok(definition);
      assert.ok(definition.inputSchema.properties.optional.anyOf, 'annotated source constraints must survive');
      assert.ok(!tools.some((tool) => tool.name === 'invalid_definition'));
      for (const explicit of [false, true]) {
        const value = explicit ? ' leading ' : 'café';
        const result = await client.callTool({ name: 'annotated_echo', arguments: {
          routing: { 'tenant.name': value }, count: 42, active: false,
        } }, explicit ? { toolDefinition: definition } : undefined);
        assert.notEqual(result.isError, true);
        assert.equal(result.structuredContent.accepted, true);
        const sent = mirrored.at(-1);
        assert.equal(sent.get('Mcp-Param-Tenant'), `=?base64?${Buffer.from(value).toString('base64')}?=`);
        assert.equal(sent.get('Mcp-Param-Count'), '42');
        assert.equal(sent.get('Mcp-Param-Active'), 'false');
      }
      assert.equal(mirrored.length, 2, 'no mismatch retry should be needed');
      console.log(JSON.stringify({ mode, cachedSchema: true, explicitSchema: true, mirrors: true }));
    } finally {
      await client.close();
    }
  }
} finally {
  clearTimeout(deadline);
}
