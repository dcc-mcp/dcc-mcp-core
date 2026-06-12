import { describe, it, expect, beforeEach, vi } from 'vitest';
import type { Panel } from '../admin-types';

// ── Helpers ──────────────────────────────────────────────────────────────────

/** Capture calls to window.history.replaceState. */
function mockLocation(search: string) {
  const href = `http://localhost:9765/admin${search}`;
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

// ── Dynamic import so modules re-evaluate after location mock is set ─────────

async function loadNavigation() {
  // Force re-import for each test so window.location is fresh.
  return import('../navigation/index');
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('PANELS', () => {
  it('includes discover and overview entries', async () => {
    const { PANELS } = await loadNavigation();
    const ids = PANELS.map((p) => p.id);
    expect(ids).toContain('discover');
    expect(ids).toContain('overview');
  });

  it('keeps all current panel entries (Phase 1 consolidated)', async () => {
    const { PANELS } = await loadNavigation();
    const ids = new Set(PANELS.map((p) => p.id));
    for (const existing of [
      'setup', 'discover', 'debug', 'instances', 'activity', 'health',
      'workflows', 'tasks', 'tools', 'openapi', 'traces',
      'governance', 'logs', 'analytics', 'overview',
    ] as Panel[]) {
      expect(ids.has(existing)).toBe(true);
    }
  });
});

describe('PANEL_ALIAS_MAP', () => {
  it('exists and is an object', async () => {
    const { PANEL_ALIAS_MAP } = await loadNavigation();
    expect(PANEL_ALIAS_MAP).toBeDefined();
    expect(typeof PANEL_ALIAS_MAP).toBe('object');
  });

  it('has no keys that collide with valid Panel ids', async () => {
    const { PANEL_ALIAS_MAP, PANEL_ID_SET } = await loadNavigation();
    for (const key of Object.keys(PANEL_ALIAS_MAP)) {
      expect(
        PANEL_ID_SET.has(key as Panel),
        `PANEL_ALIAS_MAP key "${key}" is also a valid Panel id — aliases must be non-panel names`,
      ).toBe(false);
    }
  });

  it('maps every value to a valid Panel id', async () => {
    const { PANEL_ALIAS_MAP, PANEL_ID_SET } = await loadNavigation();
    for (const [key, value] of Object.entries(PANEL_ALIAS_MAP)) {
      expect(
        PANEL_ID_SET.has(value),
        `PANEL_ALIAS_MAP["${key}"] = "${value}" is not a known Panel id`,
      ).toBe(true);
    }
  });
});

describe('isPanelId', () => {
  it('accepts valid panel ids', async () => {
    const { isPanelId } = await loadNavigation();
    expect(isPanelId('setup')).toBe(true);
    expect(isPanelId('traces')).toBe(true);
    expect(isPanelId('discover')).toBe(true);
    expect(isPanelId('overview')).toBe(true);
  });

  it('rejects null / undefined / empty', async () => {
    const { isPanelId } = await loadNavigation();
    expect(isPanelId(null)).toBe(false);
    expect(isPanelId(undefined)).toBe(false);
    expect(isPanelId('')).toBe(false);
  });

  it('rejects unknown panel names', async () => {
    const { isPanelId } = await loadNavigation();
    expect(isPanelId('unknown-panel')).toBe(false);
    expect(isPanelId('dashboard')).toBe(false);
  });
});

describe('readPanelFromUrl', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('returns the panel from the ?panel query param', async () => {
    mockLocation('?panel=traces');
    const replaceState = vi.spyOn(window.history, 'replaceState');
    const { readPanelFromUrl } = await loadNavigation();
    expect(readPanelFromUrl()).toBe('traces');
    expect(replaceState).not.toHaveBeenCalled();
  });

  it('returns "setup" when ?panel is missing', async () => {
    mockLocation('');
    const { readPanelFromUrl } = await loadNavigation();
    expect(readPanelFromUrl()).toBe('setup');
  });

  it('returns "setup" when ?panel is invalid and not in alias map', async () => {
    mockLocation('?panel=unknown');
    const { readPanelFromUrl } = await loadNavigation();
    expect(readPanelFromUrl()).toBe('setup');
  });

  it.each([
    ['marketplace', 'discover', '/admin?panel=discover&discoverTab=marketplace'],
    ['skill-paths', 'discover', '/admin?panel=discover&discoverTab=skills'],
    ['stats&range=7d', 'overview', '/admin?panel=overview&range=7d&overviewTab=stats'],
    ['calls&trace=req-123', 'traces', '/admin?panel=traces&trace=req-123&tracesTab=calls'],
  ] as const)('resolves aliased panel "%s" and self-heals to its sub-tab', async (raw, panel, expectedUrl) => {
    mockLocation(`?panel=${raw}`);
    const replaceState = vi.spyOn(window.history, 'replaceState');
    const { readPanelFromUrl } = await loadNavigation();
    expect(readPanelFromUrl()).toBe(panel);
    expect(replaceState).toHaveBeenCalledWith(null, '', expectedUrl);
  });

  it('does not redirect unknown aliases', async () => {
    mockLocation('?panel=old-dashboard');
    const replaceState = vi.spyOn(window.history, 'replaceState');
    const { readPanelFromUrl } = await loadNavigation();
    expect(readPanelFromUrl()).toBe('setup');
    expect(replaceState).not.toHaveBeenCalled();
  });

  it('accepts discover and overview as valid panel ids', async () => {
    for (const panel of ['discover', 'overview'] as const) {
      mockLocation(`?panel=${panel}`);
      const { readPanelFromUrl } = await loadNavigation();
      expect(readPanelFromUrl()).toBe(panel);
    }
  });
});

describe('readDiscoverTabFromUrl', () => {
  it('reads discoverTab query param', async () => {
    mockLocation('?panel=discover&discoverTab=search');
    const { readDiscoverTabFromUrl } = await loadNavigation();
    expect(readDiscoverTabFromUrl()).toBe('search');
  });

  it('returns empty string when discoverTab is absent', async () => {
    mockLocation('?panel=discover');
    const { readDiscoverTabFromUrl } = await loadNavigation();
    expect(readDiscoverTabFromUrl()).toBe('');
  });

  it('returns empty string when discoverTab is whitespace only', async () => {
    mockLocation('?panel=discover&discoverTab=   ');
    const { readDiscoverTabFromUrl } = await loadNavigation();
    expect(readDiscoverTabFromUrl()).toBe('');
  });
});

describe('readOverviewTabFromUrl', () => {
  it('reads overviewTab query param', async () => {
    mockLocation('?panel=overview&overviewTab=summary');
    const { readOverviewTabFromUrl } = await loadNavigation();
    expect(readOverviewTabFromUrl()).toBe('summary');
  });

  it('returns empty string when overviewTab is absent', async () => {
    mockLocation('?panel=overview');
    const { readOverviewTabFromUrl } = await loadNavigation();
    expect(readOverviewTabFromUrl()).toBe('');
  });
});

describe('readTracesTabFromUrl', () => {
  it('reads tracesTab query param', async () => {
    mockLocation('?panel=traces&tracesTab=list');
    const { readTracesTabFromUrl } = await loadNavigation();
    expect(readTracesTabFromUrl()).toBe('list');
  });

  it('returns empty string when tracesTab is absent', async () => {
    mockLocation('?panel=traces');
    const { readTracesTabFromUrl } = await loadNavigation();
    expect(readTracesTabFromUrl()).toBe('');
  });
});

describe('hrefForAdmin with tab params', () => {
  // hrefForAdmin accepts extra query params via its second argument.
  // This verifies the mechanism works for the new tab params.
  it('passes discoverTab through extra params', async () => {
    mockLocation('');
    const { hrefForAdmin } = await loadNavigation();
    const href = hrefForAdmin('discover', { discoverTab: 'search' });
    expect(href).toContain('panel=discover');
    expect(href).toContain('discoverTab=search');
  });

  it('passes overviewTab through extra params', async () => {
    mockLocation('');
    const { hrefForAdmin } = await loadNavigation();
    const href = hrefForAdmin('overview', { overviewTab: 'summary' });
    expect(href).toContain('panel=overview');
    expect(href).toContain('overviewTab=summary');
  });

  it('passes tracesTab through extra params', async () => {
    mockLocation('');
    const { hrefForAdmin } = await loadNavigation();
    const href = hrefForAdmin('traces', { tracesTab: 'list' });
    expect(href).toContain('panel=traces');
    expect(href).toContain('tracesTab=list');
  });

  it.each([
    ['stats', 'panel=overview', 'overviewTab=stats'],
    ['traffic', 'panel=overview', 'overviewTab=traffic'],
    ['calls', 'panel=traces', 'tracesTab=calls'],
    ['marketplace', 'panel=discover', 'discoverTab=marketplace'],
    ['integrations', 'panel=discover', 'discoverTab=integrations'],
    ['skill-paths', 'panel=discover', 'discoverTab=skills'],
  ] as const)('emits canonical URLs for legacy panel "%s"', async (panel, expectedPanel, expectedTab) => {
    mockLocation('');
    const { hrefForAdmin } = await loadNavigation();
    const href = hrefForAdmin(panel);
    expect(href).toContain(expectedPanel);
    expect(href).toContain(expectedTab);
    expect(href).not.toContain(`panel=${panel}`);
  });

  it('preserves canonical tab params while adding normal query params', async () => {
    mockLocation('');
    const { hrefForAdmin } = await loadNavigation();
    const href = hrefForAdmin('stats', { range: '7d' });
    expect(href).toBe('/admin?panel=overview&range=7d&overviewTab=stats');
  });

  it('generates canonical stats links for trace details', async () => {
    mockLocation('?panel=traces');
    const { traceLinks } = await loadNavigation();
    const links = traceLinks('req-123');
    expect(links.stats_url).toBe('http://localhost:9765/admin?panel=overview&range=24h&overviewTab=stats');
  });
});
