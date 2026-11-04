import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createTranslator } from '../i18n';
import { NAVIGATION } from '../navigation';
import { DashboardShell } from './DashboardShell';
import { PanelSearchBar } from './PanelSearchBar';

afterEach(cleanup);

const props = {
  activePanel: 'discover' as const,
  discoverTab: 'skills',
  overviewTab: 'stats',
  tracesTab: 'traces',
  sessionsTab: 'sessions',
  reliabilityTab: 'status',
  statsRange: '24h',
  onNavigate: vi.fn(),
  preferences: <span>Preferences</span>,
  t: createTranslator('en'),
};

describe('DashboardShell', () => {
  it('keeps every deep link in four named groups and locates the current page', () => {
    render(<DashboardShell {...props}><h1>Skill library</h1></DashboardShell>);
    const navigation = screen.getByRole('navigation');
    expect(within(navigation).getAllByRole('link')).toHaveLength(NAVIGATION.length);
    expect(within(navigation).getAllByRole('heading')).toHaveLength(4);
    expect(within(navigation).getByRole('link', { name: 'Skills' })).toHaveAttribute('aria-current', 'page');
    expect(within(navigation).getByRole('link', { name: 'Marketplace' })).toHaveAttribute('href', '/admin?panel=discover&discoverTab=marketplace');
    expect(within(navigation).getByRole('link', { name: 'Calls' })).toHaveAttribute('href', '/admin?panel=traces&tracesTab=calls');
    expect(screen.getByLabelText('Current page')).toHaveTextContent('ExtensionsSkills');
    expect(screen.getByRole('combobox', { name: 'Go to page' })).toHaveTextContent('Skills');
    expect(screen.getByRole('main')).toHaveAttribute('id', 'main-content');
    expect(screen.getByRole('link', { name: 'Skip to content' })).toHaveAttribute('href', '#main-content');
  });

  it('preserves modified-link clicks while routing ordinary clicks inside the app', () => {
    const onNavigate = vi.fn();
    render(<DashboardShell {...props} onNavigate={onNavigate}>Content</DashboardShell>);
    const link = screen.getByRole('link', { name: 'Calls' });
    link.setAttribute('href', '#calls');
    fireEvent.click(link, { ctrlKey: true });
    fireEvent.click(link, { metaKey: true });
    expect(onNavigate).not.toHaveBeenCalled();
    fireEvent.click(link);
    expect(onNavigate).toHaveBeenCalledWith('traces', expect.objectContaining({ tracesTab: 'calls' }));
  });

  it('shows the nested workspace without losing its parent navigation state', () => {
    render(<DashboardShell {...props} activePanel="sessions" sessionsTab="memory">Content</DashboardShell>);
    expect(screen.getByRole('link', { name: 'Sessions' })).toHaveAttribute('aria-current', 'page');
    expect(screen.getByLabelText('Current page')).toHaveTextContent('ExecutionSessionsContext');
  });

  it('routes the mobile page picker to the same sub-tab as its desktop link', () => {
    Element.prototype.scrollIntoView = vi.fn();
    const onNavigate = vi.fn();
    render(<DashboardShell {...props} onNavigate={onNavigate}>Content</DashboardShell>);
    fireEvent.keyDown(screen.getByRole('combobox', { name: 'Go to page' }), { key: 'ArrowDown' });
    const option = screen.getByRole('option', { name: /^Marketplace$/ });
    fireEvent.keyDown(option, { key: 'Enter' });
    expect(onNavigate).toHaveBeenCalledWith('discover', expect.objectContaining({ discoverTab: 'marketplace' }));
  });
});

describe('PanelSearchBar', () => {
  it('keeps search and latency filtering accessible inside the revised toolbar', () => {
    const onChange = vi.fn();
    const onToggleLatency = vi.fn();
    render(<PanelSearchBar panel="traces" placeholder="Search traces" ariaLabel="Filter current panel" value="" onChange={onChange}
      showLatencyFilter slowOnly slowLabel="Slow only" allLabel="All latency" onToggleLatency={onToggleLatency} meta="2 / 8" />);
    fireEvent.change(screen.getByRole('searchbox', { name: 'Filter current panel' }), { target: { value: 'maya' } });
    expect(onChange).toHaveBeenCalledWith('maya');
    fireEvent.click(screen.getByRole('button', { name: 'All latency', pressed: true }));
    expect(onToggleLatency).toHaveBeenCalledOnce();
    expect(screen.getByText('2 / 8')).toHaveAttribute('aria-live', 'polite');
  });
});

it('keeps light-theme text and semantic inks above AA contrast on workbench surfaces', () => {
  const styles = readFileSync(resolve(import.meta.dirname, '../styles.css'), 'utf8');
  const lightTheme = styles.slice(styles.indexOf(':root {'), styles.indexOf(':root.dark'));
  const color = (token: string) => {
    const hex = lightTheme.match(new RegExp(`--${token}: #([0-9a-f]{6});`))?.[1];
    expect(hex, `Missing color token ${token}`).toBeDefined();
    return [0, 2, 4].map((index) => parseInt(hex!.slice(index, index + 2), 16) / 255);
  };
  const luminance = (rgb: number[]) => rgb.reduce((sum, channel, index) =>
    sum + (channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4) * [0.2126, 0.7152, 0.0722][index], 0);
  for (const surface of ['background', 'card']) {
    for (const ink of ['foreground', 'text-secondary', 'muted-foreground', 'primary', 'ok', 'warn', 'err', 'info', 'destructive']) {
      const contrast = (luminance(color(surface)) + 0.05) / (luminance(color(ink)) + 0.05);
      expect(contrast, `${ink} on ${surface}`).toBeGreaterThanOrEqual(4.5);
    }
  }
});
