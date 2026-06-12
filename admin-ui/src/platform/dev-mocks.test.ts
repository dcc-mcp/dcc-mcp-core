import process from 'node:process';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createServer, type ViteDevServer } from 'vite';
import { adminApiMockPlugin } from '../../dev-mocks';

type JsonEndpoint = {
  path: string;
  init?: RequestInit;
  key: string;
};

const repoRoot = process.cwd();

const jsonEndpoints: JsonEndpoint[] = [
  { path: '/admin/api/health', key: 'status' },
  { path: '/admin/api/activity?limit=300', key: 'events' },
  { path: '/admin/api/instances', key: 'instances' },
  { path: '/admin/api/workers', key: 'workers' },
  { path: '/admin/api/tools', key: 'tools' },
  { path: '/admin/api/calls', key: 'calls' },
  { path: '/admin/api/traces?limit=200', key: 'traces' },
  { path: '/admin/api/traces/req-123', key: 'request_id' },
  { path: '/admin/api/traffic?limit=300', key: 'frames' },
  { path: '/admin/api/tasks?limit=300', key: 'tasks' },
  { path: '/admin/api/workflows?limit=200', key: 'workflows' },
  { path: '/admin/api/stats?range=24h', key: 'total_calls' },
  { path: '/admin/api/analytics/overview?range=30d', key: 'kpi' },
  { path: '/admin/api/analytics/timeseries?range=30d&granularity=day', key: 'series' },
  { path: '/admin/api/analytics/heatmap?range=30d', key: 'heatmap' },
  { path: '/admin/api/governance?limit=300', key: 'schema_version' },
  { path: '/admin/api/logs', key: 'logs' },
  { path: '/admin/api/skills', key: 'skills' },
  { path: '/admin/api/skill-paths', key: 'paths' },
  { path: '/admin/api/skill-detail?name=maya-modeling&dcc_type=maya', key: 'skill' },
  { path: '/admin/api/integrations', key: 'integrations' },
  { path: '/admin/api/marketplace/catalog', key: 'entries' },
  { path: '/admin/api/marketplace/installed', key: 'packages' },
  { path: '/admin/api/marketplace/sources', key: 'sources' },
  { path: '/admin/api/marketplace/outdated', key: 'packages' },
  { path: '/admin/api/issue-report/req-123', key: 'request_id' },
  { path: '/admin/api/debug-bundle/req-123', key: 'files' },
  { path: '/v1/debug/agent-traces/req-123', key: 'schema_version' },
  { path: '/v1/debug/issue-reports/req-123', key: 'schema_version' },
  { path: '/v1/debug/bundles/req-123', key: 'files' },
  { path: '/v1/debug/traces/req-123', key: 'request_id' },
  { path: '/v1/debug/instances', key: 'instances' },
  { path: '/v1/debug/integrations', key: 'integrations' },
  { path: '/v1/openapi.json', key: 'openapi' },
  { path: '/api/health', key: 'status' },
  {
    path: '/admin/api/instances/maya-1234567890/update',
    key: 'status',
    init: {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ binary: 'dcc-mcp-server' }),
    },
  },
  {
    path: '/admin/api/skill-paths',
    key: 'ok',
    init: {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: 'C:/demo/skills' }),
    },
  },
  {
    path: '/admin/api/skill-paths/7',
    key: 'ok',
    init: { method: 'DELETE' },
  },
  {
    path: '/admin/api/marketplace/install',
    key: 'installed',
    init: {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'maya-modeling', dcc: 'maya', force: true }),
    },
  },
  {
    path: '/admin/api/marketplace/update',
    key: 'results',
    init: {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'cross-dcc-utils', dcc: 'maya' }),
    },
  },
  {
    path: '/admin/api/marketplace/uninstall',
    key: 'uninstalled',
    init: {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'maya-modeling', dcc: 'maya' }),
    },
  },
  {
    path: '/admin/api/marketplace/sources',
    key: 'sources',
    init: {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ source: 'dcc-mcp/community-marketplace' }),
    },
  },
  {
    path: '/admin/api/integrations',
    key: 'kind',
    init: {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        kind: 'wecom',
        config: {
          webhook_url: 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=demo',
          event_types: ['gateway.ready'],
          template: 'DCC $dcc_type $url',
        },
      }),
    },
  },
];

describe('admin API dev mock middleware', () => {
  let server: ViteDevServer;
  let baseUrl: string;

  beforeAll(async () => {
    server = await createServer({
      root: repoRoot,
      logLevel: 'error',
      server: {
        host: '127.0.0.1',
        port: 0,
      },
      plugins: [adminApiMockPlugin()],
    });
    await server.listen();
    const address = server.httpServer?.address();
    if (!address || typeof address === 'string') {
      throw new Error('Vite test server did not expose a TCP address');
    }
    baseUrl = `http://127.0.0.1:${address.port}`;
  });

  afterAll(async () => {
    await server?.close();
  });

  it('serves JSON for every endpoint used by panel hooks and stable debug links', async () => {
    for (const endpoint of jsonEndpoints) {
      const response = await fetch(`${baseUrl}${endpoint.path}`, endpoint.init);
      const contentType = response.headers.get('content-type') ?? '';
      const text = await response.text();

      expect(response.ok, endpoint.path).toBe(true);
      expect(contentType, endpoint.path).toContain('application/json');
      expect(text.trim().startsWith('<'), endpoint.path).toBe(false);

      const payload = JSON.parse(text) as Record<string, unknown>;
      expect(payload, endpoint.path).toHaveProperty(endpoint.key);
    }
  });

  it('serves export endpoints as non-HTML downloads', async () => {
    for (const path of [
      '/admin/api/traffic/export?limit=1000',
      '/admin/api/analytics/export?range=30d',
      '/v1/debug/traffic/export?limit=1000',
      '/v1/debug/analytics/export?range=30d',
    ]) {
      const response = await fetch(`${baseUrl}${path}`);
      const contentType = response.headers.get('content-type') ?? '';
      const text = await response.text();

      expect(response.ok, path).toBe(true);
      expect(contentType.toLowerCase(), path).not.toContain('text/html');
      expect(text.trim().startsWith('<'), path).toBe(false);
      expect(text.trim().length, path).toBeGreaterThan(0);
    }
  });
});
