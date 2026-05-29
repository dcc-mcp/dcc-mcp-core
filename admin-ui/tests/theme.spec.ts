import { expect, test } from '@playwright/test';

import { DEFAULT_THEME_MODE, isThemeMode, resolveTheme, THEMES } from '../src/theme';

test.describe('theme mode helpers', () => {
  test('defaults to the dark scheme', () => {
    expect(DEFAULT_THEME_MODE).toBe('dark');
  });

  test('recognises only the supported modes', () => {
    expect(THEMES).toEqual(['light', 'dark', 'system']);
    expect(isThemeMode('light')).toBe(true);
    expect(isThemeMode('dark')).toBe(true);
    expect(isThemeMode('system')).toBe(true);
    expect(isThemeMode('solarized')).toBe(false);
    expect(isThemeMode(null)).toBe(false);
    expect(isThemeMode(undefined)).toBe(false);
  });

  test('resolves explicit modes to themselves', () => {
    expect(resolveTheme('light')).toBe('light');
    expect(resolveTheme('dark')).toBe('dark');
  });

  test('resolves system to a concrete light/dark value', () => {
    expect(['light', 'dark']).toContain(resolveTheme('system'));
  });
});
