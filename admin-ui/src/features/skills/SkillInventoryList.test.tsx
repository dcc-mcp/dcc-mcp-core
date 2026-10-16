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
    expect(within(row).getByText('unloaded')).toBeInTheDocument();
    expect(within(row).getByText('used')).toBeInTheDocument();
    expect(within(row).getByText('12 calls, 1 failures')).toBeInTheDocument();
    const details = row.querySelector('details')!;
    expect(details).not.toHaveAttribute('open');
    expect(within(row).getByText('fifth')).not.toBeVisible();
    fireEvent.click(details.querySelector('summary')!);
    expect(details).toHaveAttribute('open');
    expect(within(row).getByText('fifth')).toBeVisible();
    expect(within(row).getByText('four', { selector: 'code' })).toBeVisible();
    expect(within(row).getByText('best rank 2')).toBeVisible();
    fireEvent.click(within(row).getByRole('button', { name: 'Open blender-geometry detail' }));
    expect(onOpen).toHaveBeenCalledWith(skill);
  });
});
