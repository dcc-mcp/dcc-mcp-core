import { cleanup, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { normalizeSkillRow } from '../../admin-types';
import { createTranslator, SUPPORTED_LOCALES } from '../../i18n';
import { SkillDetailPanel } from './SkillDetailPanel';
import { SkillsPanel } from './SkillsPanel';

const rows = [
  { name: 'maya-history', dcc_type: 'maya', loaded: false, adoption: { used: true, call_count: 2 } },
  { name: 'blender-ready', dcc_type: 'blender', loaded: true, adoption: { used: false, call_count: 0 } },
];

function mockInventory() {
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
    const path = String(input);
    return new Response(JSON.stringify(path.endsWith('/skills') ? { skills: rows } : { paths: [] }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  }));
}

const callbacks = { onUpdated: vi.fn(), onError: vi.fn() };

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe.each(SUPPORTED_LOCALES)('skill loading and call history in %s', (locale) => {
  const t = createTranslator(locale);

  it.each([
    ['skillPaths.state.unloaded', 'maya-history'],
    ['skillPaths.state.loaded', 'blender-ready'],
    ['skillPaths.state.used', 'maya-history'],
    ['skillPaths.state.notUsed', 'blender-ready'],
  ] as const)('finds rows using visible %s text', async (key, name) => {
    mockInventory();
    render(<SkillsPanel active search={t(key)} updatedAt="" t={t} {...callbacks} />);
    expect(await screen.findByText(name, { selector: '.skill-inventory-title' })).toBeVisible();
  });

  it('labels detail loading state separately from call history', () => {
    const { container } = render(
      <SkillDetailPanel skill={normalizeSkillRow(rows[0])} detail={null} busy={false}
        onReload={vi.fn()} onClose={vi.fn()} t={t} />,
    );
    const state = container.querySelector('.skill-detail-summary-grid > span')!;
    expect(state.querySelector('strong')).toHaveTextContent(t('skillPaths.detail.loadingState'));
    expect(state).toHaveTextContent(t('skillPaths.state.unloaded'));
    expect(state).not.toHaveTextContent(t('skillPaths.table.state'));
  });
});

it('recomputes localized state search when the language changes', async () => {
  mockInventory();
  const en = createTranslator('en');
  const zh = createTranslator('zh-CN');
  const query = zh('skillPaths.state.used');
  const { rerender } = render(<SkillsPanel active search={query} updatedAt="" t={en} {...callbacks} />);
  expect(await screen.findByText(en('skillPaths.empty.skillsSearch'))).toBeVisible();
  rerender(<SkillsPanel active search={query} updatedAt="" t={zh} {...callbacks} />);
  expect(await screen.findByText('maya-history', { selector: '.skill-inventory-title' })).toBeVisible();
});
