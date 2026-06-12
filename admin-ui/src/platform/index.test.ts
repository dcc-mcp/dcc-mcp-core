import { afterEach, describe, expect, it } from 'vitest';
import { AdminApiError, adminApiBase, adminJsonResponse, adminOkResponse } from './index';

const originalHref = window.location.href;

function mockLocation(href: string) {
  const u = new URL(href);
  Object.defineProperty(window, 'location', {
    value: {
      href,
      pathname: u.pathname,
      search: u.search,
      origin: u.origin,
    },
    writable: true,
    configurable: true,
  });
}

afterEach(() => {
  mockLocation(originalHref);
});

describe('adminApiBase', () => {
  it('uses /admin/api when the single-file admin UI is served from the origin root', () => {
    mockLocation('http://localhost:3721/?panel=discover&discoverTab=marketplace');

    expect(adminApiBase()).toBe('http://localhost:3721/admin/api');
  });

  it('uses /admin/api when the admin UI is served from /admin/index.html', () => {
    mockLocation('http://localhost:3721/admin/index.html?panel=traces');

    expect(adminApiBase()).toBe('http://localhost:3721/admin/api');
  });
});

describe('adminJsonResponse', () => {
  it('parses successful JSON responses', async () => {
    const response = new Response(JSON.stringify({ ok: true }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });

    await expect(adminJsonResponse<{ ok: boolean }>(response, '/health'))
      .resolves
      .toEqual({ ok: true });
  });

  it('rejects HTML fallback responses with a diagnostic message', async () => {
    const response = new Response('<!doctype html><title>Vite</title>', {
      status: 200,
      headers: { 'content-type': 'text/html' },
    });

    await expect(adminJsonResponse(response, '/marketplace/catalog'))
      .rejects
      .toThrow('Admin API returned HTML for /marketplace/catalog');
  });

  it('includes the actual requested URL when HTML fallback reaches a fetch response', async () => {
    const response = new Response('<!doctype html><title>Vite</title>', {
      status: 200,
      headers: { 'content-type': 'text/html' },
    });
    Object.defineProperty(response, 'url', {
      value: 'http://localhost:3721/admin/api/traces?limit=200',
      configurable: true,
    });

    await expect(adminJsonResponse(response, '/traces?limit=200'))
      .rejects
      .toMatchObject({
        name: 'AdminApiError',
        requestUrl: 'http://localhost:3721/admin/api/traces?limit=200',
        message: expect.stringContaining('requested http://localhost:3721/admin/api/traces?limit=200'),
      } satisfies Partial<AdminApiError>);
  });

  it('rejects invalid JSON without exposing raw parser jargon as the only clue', async () => {
    const response = new Response('not-json', {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });

    await expect(adminJsonResponse(response, '/integrations'))
      .rejects
      .toThrow('Invalid JSON from Admin API /integrations');
  });

  it('keeps structured JSON error payloads on non-OK responses', async () => {
    const payload = {
      status: 'binary_not_found',
      message: 'Binary not found in manifest.',
    };
    const response = new Response(JSON.stringify(payload), {
      status: 404,
      statusText: 'Not Found',
      headers: { 'content-type': 'application/json' },
    });

    await expect(adminJsonResponse(response, '/instances/maya-123/update'))
      .rejects
      .toMatchObject({
        name: 'AdminApiError',
        status: 404,
        payload,
      } satisfies Partial<AdminApiError>);
  });
});

describe('adminOkResponse', () => {
  it('accepts empty success responses', async () => {
    const response = new Response(null, { status: 204 });

    await expect(adminOkResponse(response, '/skill-paths/7'))
      .resolves
      .toBeUndefined();
  });

  it('accepts JSON success responses without returning the body', async () => {
    const response = new Response(JSON.stringify({ ok: true }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });

    await expect(adminOkResponse(response, '/skill-paths'))
      .resolves
      .toBeUndefined();
  });

  it('rejects HTML fallback responses for mutation endpoints', async () => {
    const response = new Response('<html><body>app shell</body></html>', {
      status: 200,
      headers: { 'content-type': 'text/html' },
    });

    await expect(adminOkResponse(response, '/skill-paths'))
      .rejects
      .toThrow('Admin API returned HTML for /skill-paths');
  });
});
