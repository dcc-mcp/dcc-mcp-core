import { expect, test } from '@playwright/test';

test('experiments panel presents a trace monitoring workspace', async ({ page }) => {
  await page.goto('/admin/?panel=experiments');

  const panel = page.locator('section[data-panel="experiments"]');
  await expect(panel).toBeVisible();
  await expect(panel.getByRole('heading', { name: 'Experiment Traces' })).toBeVisible();
  await expect(panel.getByRole('complementary', { name: 'Trace filters' })).toBeVisible();
  await expect(panel.getByRole('tab', { name: 'Trace' })).toHaveAttribute('aria-selected', 'true');
  await expect(panel.getByRole('region', { name: 'Runs & evidence' })).toBeVisible();
  await expect(panel.locator('.experiment-run-card')).toHaveCount(3);
  await expect(panel.locator('.experiment-run-card').nth(1)).toContainText('run-reference');
  await expect(panel.locator('.experiment-run-judge')).toContainText('scene-quality');
  await expect(panel.locator('.experiment-run-judge')).toContainText('fidelity 0.72');
  await expect(panel.locator('.experiment-trace-kpis .metric-tile')).toHaveCount(5);
  await expect(panel.getByRole('columnheader', { name: 'Input / output' })).toBeVisible();
  await expect(panel.locator('.experiment-trace-row')).toHaveCount(3);
  await expect(panel.locator('.experiment-trace-row').first()).toContainText('session-reference');
});

test('selecting a trace opens searchable span and payload detail', async ({ page }) => {
  await page.goto('/admin/?panel=experiments');

  const panel = page.locator('section[data-panel="experiments"]');
  await panel.locator('.experiment-trace-row').first().click();

  await expect(panel.getByRole('button', { name: 'Back to traces' })).toBeVisible();
  await expect(panel.getByText('Trace ID', { exact: true })).toBeVisible();
  await expect(panel.getByPlaceholder('Search span name')).toBeVisible();
  await expect(panel.locator('.experiment-span-item')).toHaveCount(4);
  await expect(panel.getByRole('tab', { name: 'Input / output' })).toHaveAttribute('aria-selected', 'true');
  await expect(panel.locator('.experiment-message-detail')).toHaveCount(4);
  await expect(panel.locator('.experiment-message-detail').first()).toContainText('system');
});

test('trace detail stays contained at the narrow admin breakpoint', async ({ page }) => {
  await page.setViewportSize({ width: 673, height: 871 });
  await page.goto('/admin/?panel=experiments');

  const panel = page.locator('section[data-panel="experiments"]');
  await panel.locator('.experiment-trace-row').first().click();
  await expect(panel.locator('.experiment-trace-detail')).toBeVisible();

  const layout = await panel.evaluate((element) => {
    const body = element.querySelector<HTMLElement>('.experiment-detail-body');
    const summary = element.querySelector<HTMLElement>('.experiment-detail-summary');
    return {
      contained: element.scrollWidth <= element.clientWidth,
      bodyColumns: body ? getComputedStyle(body).gridTemplateColumns.split(' ').length : 0,
      summaryColumns: summary ? getComputedStyle(summary).gridTemplateColumns.split(' ').length : 0,
    };
  });
  expect(layout).toEqual({ contained: true, bodyColumns: 1, summaryColumns: 2 });
});
