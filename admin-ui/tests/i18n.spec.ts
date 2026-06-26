import { expect, test } from '@playwright/test';

import {
  I18N_MESSAGES,
  I18N_NAMESPACES,
  SUPPORTED_LOCALES,
  auditI18nNamespaces,
  createNamespaceTranslator,
  createTranslator,
  detectLocale,
  normalizeLocaleTag,
  translate,
} from '../src/i18n';

test.describe('i18n locale detection', () => {
  test('normalizes supported browser locale variants', () => {
    expect(normalizeLocaleTag('en-US')).toBe('en');
    expect(normalizeLocaleTag('zh')).toBe('zh-CN');
    expect(normalizeLocaleTag('zh-Hans')).toBe('zh-CN');
    expect(normalizeLocaleTag('zh_CN')).toBe('zh-CN');
    expect(normalizeLocaleTag('ja-JP')).toBe('ja');
    expect(normalizeLocaleTag('ko-KR')).toBe('ko');
  });

  test('detects every supported runtime locale from browser preferences', () => {
    expect(detectLocale({ navigatorLanguages: ['en-US'] })).toMatchObject({ locale: 'en', source: 'navigator' });
    expect(detectLocale({ navigatorLanguages: ['zh-Hans-CN'] })).toMatchObject({ locale: 'zh-CN', source: 'navigator' });
    expect(detectLocale({ navigatorLanguages: ['ja-JP'] })).toMatchObject({ locale: 'ja', source: 'navigator' });
    expect(detectLocale({ navigatorLanguages: ['ko-KR'] })).toMatchObject({ locale: 'ko', source: 'navigator' });
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

  test('keeps every namespace covered for every supported locale', () => {
    expect(auditI18nNamespaces()).toEqual([]);
    for (const namespace of I18N_NAMESPACES) {
      expect(Object.keys(I18N_MESSAGES[namespace]).sort()).toEqual(
        [...SUPPORTED_LOCALES].sort(),
      );
    }
  });

  test('returns namespaced strings instead of missing translation keys', () => {
    const t = createTranslator('ja');

    expect(t('navigation.panel.setup')).toBe('コマンドセンター');
    expect(translate('zh-CN', 'search.input.default')).toBe('搜索此面板...');
    expect(translate('ko', 'governance.section.recentRequestDecisions')).toBe('최근 요청 결정');
  });

  test('scopes panel translators and interpolates dynamic values', () => {
    const t = createNamespaceTranslator('en', ['common', 'navigation']);

    expect(t('navigation.panel.health')).toBe('Health');
    expect(t('common.status.labelValue', { label: 'Ready', value: 2 })).toBe('Ready: 2');
    expect(() => (t as (key: string) => string)('search.input.default')).toThrow(/outside requested namespaces/);
  });
});
