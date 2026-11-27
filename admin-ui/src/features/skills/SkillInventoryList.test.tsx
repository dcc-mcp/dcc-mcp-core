import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { normalizeSkillRow } from '../../admin-types';
import { createTranslator } from '../../i18n';
import { SkillInventoryList } from './SkillInventoryList';

afterEach(cleanup);
const t = createTranslator('en');

describe('SkillInventoryList', () => {
  it('preserves all tools and instances in expandable details without changing state labels', () => {
    const skill = normalizeSkillRow({
      name: 'blender-geometry', dcc_type: 'blender', loaded: false,
      summary: 'Inspect and edit geometry.',
      tools: ['first', 'second', 'third', 'fourth', 'fifth'],
      instances: ['one', 'two', 'three', 'four'],
      adoption: { used: true, call_count: 12, failure_count: 1, search_hits: 3, best_rank: 2 },
    });
    const onOpen = vi.fn();
    render(<SkillInventoryList skills={[skill]} selected={null} onOpen={onOpen} t={t} />);
    const row = screen.getByRole('listitem');
    expect(within(row).getByText(t('skillPaths.state.unloaded'))).toBeInTheDocument();
    expect(within(row).getByText(t('skillPaths.state.used'))).toBeInTheDocument();
    expect(within(row).getByText(t('skillPaths.state.used'))).not.toBeVisible();
    expect(within(row).getByText('12 calls, 1 failures')).toBeInTheDocument();
    const details = row.querySelector('details')!;
    expect(details).not.toHaveAttribute('open');
    expect(within(row).getByText('fifth')).not.toBeVisible();
    fireEvent.click(details.querySelector('summary')!);
    expect(details).toHaveAttribute('open');
    expect(within(row).getByText('fifth')).toBeVisible();
    expect(within(row).getByText('four', { selector: 'code' })).toBeVisible();
    expect(within(row).getByText('best rank 2')).toBeVisible();
    expect(within(row).getByText(t('skillPaths.state.used'))).toBeVisible();
    fireEvent.click(within(row).getByRole('button', { name: 'Open blender-geometry detail' }));
    expect(onOpen).toHaveBeenCalledWith(skill);
  });

  it('uses the project DCC icon and a single state badge while retaining full copy in details', () => {
    const skill = normalizeSkillRow({ name: 'maya-modeling', dcc_type: 'maya', loaded: true,
      summary: 'Model and inspect geometry.', branding: { emoji: '🎨', tagline: 'Detailed branding tagline.' },
      version: '1.0.0', adoption: { used: true } });
    render(<SkillInventoryList skills={[skill]} selected={null} onOpen={vi.fn()} t={t} />);
    const row = screen.getByRole('listitem');
    expect(row.querySelectorAll('[data-slot="badge"]')).toHaveLength(1);
    expect(row.querySelector('.skill-inventory-avatar img')).toHaveAttribute('src', expect.stringContaining('maya'));
    expect(row).not.toHaveTextContent('🎨');
    expect(screen.getByText('Detailed branding tagline.')).not.toBeVisible();
    fireEvent.click(row.querySelector('summary')!);
    expect(screen.getByText('Detailed branding tagline.')).toBeVisible();
    expect(screen.getByText('v1.0.0')).toBeVisible();
  });

  it('shows one shared skill row with its DCC scope and partial instance state', () => {
    const skill = normalizeSkillRow({
      name: 'dcc-mcp',
      dcc_types: ['blender', 'maya'],
      load_state: 'partial',
      loaded_instance_count: 1,
      unloaded_instance_count: 1,
      instance_details: [
        { id: 'blender-1', prefix: 'blender-1', dcc_type: 'blender', loaded: false },
        { id: 'maya-1', prefix: 'maya-1', dcc_type: 'maya', loaded: true },
      ],
      adoption: {},
    });
    render(<SkillInventoryList skills={[skill]} selected={null} onOpen={vi.fn()} t={t} />);
    const row = screen.getByRole('listitem');
    expect(within(row).getByText(t('skillPaths.state.partial'))).toBeInTheDocument();
    expect(within(row).getByText('blender · maya')).toBeInTheDocument();
    fireEvent.click(row.querySelector('summary')!);
    expect(within(row).getByText('blender-1')).toHaveClass('is-unloaded');
    expect(within(row).getByText('maya-1')).toHaveClass('is-loaded');
  });

  it('shows catalog availability without inventing an instance or a partial state', () => {
    const skill = normalizeSkillRow({
      name: 'dcc-mcp',
      dcc_types: ['blender', 'maya'],
      load_state: 'loaded',
      loaded_instance_count: 1,
      unloaded_instance_count: 0,
      catalog_only: false,
      catalog_dcc_types: ['maya'],
      instance_details: [{ id: 'maya-1', prefix: 'maya-1', dcc_type: 'maya', loaded: true }],
      dcc_states: [{
        dcc_type: 'maya',
        load_state: 'loaded',
        loaded_instance_count: 1,
        unloaded_instance_count: 0,
        catalog_available: true,
      }],
    });
    render(<SkillInventoryList skills={[skill]} selected={null} onOpen={vi.fn()} t={t} />);
    const row = screen.getByRole('listitem');
    expect(within(row).getByRole('button', { name: 'Open dcc-mcp detail' })).toBeInTheDocument();
    expect(within(row).getByText(t('skillPaths.state.loaded'), { selector: '.skill-inventory-load-state' })).toBeInTheDocument();
    expect(row).not.toHaveTextContent('00000000');
    fireEvent.click(row.querySelector('summary')!);
    expect(within(row).getByText(t('skillPaths.table.dccAvailability'))).toBeVisible();
    expect(within(row).getByText(t('skillPaths.state.catalogAvailable'))).toBeVisible();
    expect(within(row).getByText('1 loaded, 0 unloaded')).toBeVisible();
  });

  it('shows catalog-only skills without a fake instance', () => {
    const skill = normalizeSkillRow({
      name: 'catalog-skill',
      dcc_types: ['blender', 'maya'],
      load_state: 'unloaded',
      catalog_only: true,
      catalog_dcc_types: ['blender', 'maya'],
      dcc_states: [
        { dcc_type: 'blender', load_state: 'unloaded', catalog_available: true },
        { dcc_type: 'maya', load_state: 'unloaded', catalog_available: true },
      ],
    });
    render(<SkillInventoryList skills={[skill]} selected={null} onOpen={vi.fn()} t={t} />);
    const row = screen.getByRole('listitem');
    expect(row).not.toHaveTextContent('00000000');
    fireEvent.click(row.querySelector('summary')!);
    expect(within(row).getAllByText(t('skillPaths.state.catalogAvailable'))).toHaveLength(3);
    expect(within(row).queryByText(t('skillPaths.state.unloaded'))).toBeNull();
    expect(within(row).getByText('blender')).toBeVisible();
    expect(within(row).getByText('maya')).toBeVisible();
  });
});
