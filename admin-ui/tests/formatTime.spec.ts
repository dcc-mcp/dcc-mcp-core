import { expect, test } from '@playwright/test';

import { formatTime, timestampTitle } from '../src/time';

function localIso(year: number, month: number, day: number, hour: number, minute: number, second: number): string {
  return new Date(year, month - 1, day, hour, minute, second).toISOString();
}

test.describe('formatTime', () => {
  test('keeps same-day timestamps compact', () => {
    const now = new Date(2026, 4, 22, 12, 0, 0);

    expect(formatTime(localIso(2026, 5, 22, 14, 8, 23), now)).toBe('14:08:23');
  });

  test('adds month and day for cross-day timestamps', () => {
    const now = new Date(2026, 4, 22, 12, 0, 0);

    expect(formatTime(localIso(2026, 5, 21, 14, 8, 23), now)).toBe('05/21 14:08:23');
  });

  test('returns a dash for missing or invalid timestamps', () => {
    expect(formatTime(null)).toBe('-');
    expect(formatTime(undefined)).toBe('-');
    expect(formatTime('not-a-date')).toBe('-');
  });

  test('provides absolute ISO text for hover titles', () => {
    const value = localIso(2026, 5, 22, 14, 8, 23);

    expect(timestampTitle(value)).toBe(new Date(value).toISOString());
    expect(timestampTitle(null)).toBeUndefined();
  });
});
