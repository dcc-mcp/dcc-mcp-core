import { describe, it, expect } from 'vitest';
import { locales } from './locales';

describe('Locales Dictionary', () => {
  it('should have both en and zh languages', () => {
    expect(locales.en).toBeDefined();
    expect(locales.zh).toBeDefined();
  });

  it('should have the same keys in both en and zh', () => {
    const enKeys = Object.keys(locales.en).sort();
    const zhKeys = Object.keys(locales.zh).sort();

    expect(enKeys).toEqual(zhKeys);
  });

  it('should translate title and description correctly', () => {
    expect(locales.en.title).toBe('Marketplace');
    expect(locales.zh.title).toBe('Skills 市场');
  });
});
