import { expect, test } from '@playwright/test';

import { createTranslator, detectLocale, normalizeLocaleTag, translate } from '../src/i18n';

test.describe('i18n locale detection', () => {
  test('normalizes supported browser locale variants', () => {
    expect(normalizeLocaleTag('en-US')).toBe('en');
    expect(normalizeLocaleTag('zh')).toBe('zh-CN');
    expect(normalizeLocaleTag('zh-Hans')).toBe('zh-CN');
    expect(normalizeLocaleTag('zh_CN')).toBe('zh-CN');
    expect(normalizeLocaleTag('ja-JP')).toBe('ja');
    expect(normalizeLocaleTag('ko-KR')).toBe('ko');
  });

  test('falls back to English when preferences are unsupported', () => {
    expect(detectLocale({ navigatorLanguages: ['fr-FR', 'de-DE'] })).toEqual({
      locale: 'en',
      source: 'fallback',
    });
  });

  test('prefers explicit overrides for future manual selection', () => {
    expect(detectLocale({ override: 'ko-KR', navigatorLanguages: ['ja-JP'] })).toEqual({
      locale: 'ko',
      source: 'override',
      matchedTag: 'ko-KR',
    });
  });

  test('uses navigator language order before the fallback language field', () => {
    expect(detectLocale({ navigatorLanguages: ['fr-FR', 'zh-Hans'], navigatorLanguage: 'ja-JP' })).toEqual({
      locale: 'zh-CN',
      source: 'navigator',
      matchedTag: 'zh-Hans',
    });
  });

  test('returns English strings instead of missing translation keys', () => {
    const t = createTranslator('ja');

    expect(t('panel.setup')).toBe('Connect IDE');
    expect(translate('zh-CN', 'search.default')).toBe('Search this panel...');
  });
});
