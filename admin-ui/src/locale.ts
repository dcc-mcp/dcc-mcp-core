import { SUPPORTED_LOCALES, type SupportedLocale } from './i18n';

export const LOCALE_STORAGE_KEY = 'dcc-mcp-admin-locale';

export const LOCALE_LABELS: Record<SupportedLocale, string> = {
  en: 'English',
  'zh-CN': '简体中文',
  ja: '日本語',
  ko: '한국어',
};

export const LOCALE_TRIGGER_LABELS: Record<SupportedLocale, string> = {
  en: 'EN',
  'zh-CN': '中文',
  ja: '日本語',
  ko: '한국어',
};

const SUPPORTED_LOCALE_ID_SET = new Set<string>(SUPPORTED_LOCALES);

export function isSupportedLocale(value: string | null | undefined): value is SupportedLocale {
  return value != null && SUPPORTED_LOCALE_ID_SET.has(value);
}

export function readLocaleOverride(): SupportedLocale | null {
  const u = new URL(window.location.href);
  const queryLocale = u.searchParams.get('lang');
  if (isSupportedLocale(queryLocale)) {
    return queryLocale;
  }
  try {
    const stored = window.localStorage.getItem(LOCALE_STORAGE_KEY);
    return isSupportedLocale(stored) ? stored : null;
  } catch {
    return null;
  }
}

export function storeLocaleOverride(locale: SupportedLocale): void {
  try {
    window.localStorage.setItem(LOCALE_STORAGE_KEY, locale);
  } catch {
    // localStorage may be unavailable in hardened embedded views.
  }
}
