export const SUPPORTED_LOCALES = ['en', 'zh-CN', 'ja', 'ko'] as const;

export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

export type LocaleDetectionSource = 'override' | 'navigator' | 'fallback';

export type LocaleDetection = {
  locale: SupportedLocale;
  source: LocaleDetectionSource;
  matchedTag?: string;
};

export type InterpolationValues = Record<string, string | number | boolean | null | undefined>;

export const DEFAULT_LOCALE: SupportedLocale = 'en';

const SUPPORTED_LOCALE_SET = new Set<string>(SUPPORTED_LOCALES);

import commonEn from './locales/en/common.json' with { type: 'json' };
import commonZhCN from './locales/zh-CN/common.json' with { type: 'json' };
import commonJa from './locales/ja/common.json' with { type: 'json' };
import commonKo from './locales/ko/common.json' with { type: 'json' };
import actionEn from './locales/en/action.json' with { type: 'json' };
import actionZhCN from './locales/zh-CN/action.json' with { type: 'json' };
import actionJa from './locales/ja/action.json' with { type: 'json' };
import actionKo from './locales/ko/action.json' with { type: 'json' };
import chromeEn from './locales/en/chrome.json' with { type: 'json' };
import chromeZhCN from './locales/zh-CN/chrome.json' with { type: 'json' };
import chromeJa from './locales/ja/chrome.json' with { type: 'json' };
import chromeKo from './locales/ko/chrome.json' with { type: 'json' };
import navigationEn from './locales/en/navigation.json' with { type: 'json' };
import navigationZhCN from './locales/zh-CN/navigation.json' with { type: 'json' };
import navigationJa from './locales/ja/navigation.json' with { type: 'json' };
import navigationKo from './locales/ko/navigation.json' with { type: 'json' };
import searchEn from './locales/en/search.json' with { type: 'json' };
import searchZhCN from './locales/zh-CN/search.json' with { type: 'json' };
import searchJa from './locales/ja/search.json' with { type: 'json' };
import searchKo from './locales/ko/search.json' with { type: 'json' };
import setupEn from './locales/en/setup.json' with { type: 'json' };
import setupZhCN from './locales/zh-CN/setup.json' with { type: 'json' };
import setupJa from './locales/ja/setup.json' with { type: 'json' };
import setupKo from './locales/ko/setup.json' with { type: 'json' };
import debugEn from './locales/en/debug.json' with { type: 'json' };
import debugZhCN from './locales/zh-CN/debug.json' with { type: 'json' };
import debugJa from './locales/ja/debug.json' with { type: 'json' };
import debugKo from './locales/ko/debug.json' with { type: 'json' };
import activityEn from './locales/en/activity.json' with { type: 'json' };
import activityZhCN from './locales/zh-CN/activity.json' with { type: 'json' };
import activityJa from './locales/ja/activity.json' with { type: 'json' };
import activityKo from './locales/ko/activity.json' with { type: 'json' };
import healthEn from './locales/en/health.json' with { type: 'json' };
import healthZhCN from './locales/zh-CN/health.json' with { type: 'json' };
import healthJa from './locales/ja/health.json' with { type: 'json' };
import healthKo from './locales/ko/health.json' with { type: 'json' };
import instancesEn from './locales/en/instances.json' with { type: 'json' };
import instancesZhCN from './locales/zh-CN/instances.json' with { type: 'json' };
import instancesJa from './locales/ja/instances.json' with { type: 'json' };
import instancesKo from './locales/ko/instances.json' with { type: 'json' };
import toolsEn from './locales/en/tools.json' with { type: 'json' };
import toolsZhCN from './locales/zh-CN/tools.json' with { type: 'json' };
import toolsJa from './locales/ja/tools.json' with { type: 'json' };
import toolsKo from './locales/ko/tools.json' with { type: 'json' };
import workflowsEn from './locales/en/workflows.json' with { type: 'json' };
import workflowsZhCN from './locales/zh-CN/workflows.json' with { type: 'json' };
import workflowsJa from './locales/ja/workflows.json' with { type: 'json' };
import workflowsKo from './locales/ko/workflows.json' with { type: 'json' };
import tasksEn from './locales/en/tasks.json' with { type: 'json' };
import tasksZhCN from './locales/zh-CN/tasks.json' with { type: 'json' };
import tasksJa from './locales/ja/tasks.json' with { type: 'json' };
import tasksKo from './locales/ko/tasks.json' with { type: 'json' };
import openapiEn from './locales/en/openapi.json' with { type: 'json' };
import openapiZhCN from './locales/zh-CN/openapi.json' with { type: 'json' };
import openapiJa from './locales/ja/openapi.json' with { type: 'json' };
import openapiKo from './locales/ko/openapi.json' with { type: 'json' };
import callsEn from './locales/en/calls.json' with { type: 'json' };
import callsZhCN from './locales/zh-CN/calls.json' with { type: 'json' };
import callsJa from './locales/ja/calls.json' with { type: 'json' };
import callsKo from './locales/ko/calls.json' with { type: 'json' };
import tracesEn from './locales/en/traces.json' with { type: 'json' };
import tracesZhCN from './locales/zh-CN/traces.json' with { type: 'json' };
import tracesJa from './locales/ja/traces.json' with { type: 'json' };
import tracesKo from './locales/ko/traces.json' with { type: 'json' };
import trafficEn from './locales/en/traffic.json' with { type: 'json' };
import trafficZhCN from './locales/zh-CN/traffic.json' with { type: 'json' };
import trafficJa from './locales/ja/traffic.json' with { type: 'json' };
import trafficKo from './locales/ko/traffic.json' with { type: 'json' };
import statsEn from './locales/en/stats.json' with { type: 'json' };
import statsZhCN from './locales/zh-CN/stats.json' with { type: 'json' };
import statsJa from './locales/ja/stats.json' with { type: 'json' };
import statsKo from './locales/ko/stats.json' with { type: 'json' };
import governanceEn from './locales/en/governance.json' with { type: 'json' };
import governanceZhCN from './locales/zh-CN/governance.json' with { type: 'json' };
import governanceJa from './locales/ja/governance.json' with { type: 'json' };
import governanceKo from './locales/ko/governance.json' with { type: 'json' };
import logsEn from './locales/en/logs.json' with { type: 'json' };
import logsZhCN from './locales/zh-CN/logs.json' with { type: 'json' };
import logsJa from './locales/ja/logs.json' with { type: 'json' };
import logsKo from './locales/ko/logs.json' with { type: 'json' };
import skillPathsEn from './locales/en/skill-paths.json' with { type: 'json' };
import skillPathsZhCN from './locales/zh-CN/skill-paths.json' with { type: 'json' };
import skillPathsJa from './locales/ja/skill-paths.json' with { type: 'json' };
import skillPathsKo from './locales/ko/skill-paths.json' with { type: 'json' };
import analyticsEn from './locales/en/analytics.json' with { type: 'json' };
import analyticsZhCN from './locales/zh-CN/analytics.json' with { type: 'json' };
import analyticsJa from './locales/ja/analytics.json' with { type: 'json' };
import analyticsKo from './locales/ko/analytics.json' with { type: 'json' };

export const I18N_MESSAGES = {
  common: {
    en: commonEn,
    'zh-CN': commonZhCN,
    ja: commonJa,
    ko: commonKo,
  },
  action: {
    en: actionEn,
    'zh-CN': actionZhCN,
    ja: actionJa,
    ko: actionKo,
  },
  chrome: {
    en: chromeEn,
    'zh-CN': chromeZhCN,
    ja: chromeJa,
    ko: chromeKo,
  },
  navigation: {
    en: navigationEn,
    'zh-CN': navigationZhCN,
    ja: navigationJa,
    ko: navigationKo,
  },
  search: {
    en: searchEn,
    'zh-CN': searchZhCN,
    ja: searchJa,
    ko: searchKo,
  },
  setup: {
    en: setupEn,
    'zh-CN': setupZhCN,
    ja: setupJa,
    ko: setupKo,
  },
  debug: {
    en: debugEn,
    'zh-CN': debugZhCN,
    ja: debugJa,
    ko: debugKo,
  },
  activity: {
    en: activityEn,
    'zh-CN': activityZhCN,
    ja: activityJa,
    ko: activityKo,
  },
  health: {
    en: healthEn,
    'zh-CN': healthZhCN,
    ja: healthJa,
    ko: healthKo,
  },
  instances: {
    en: instancesEn,
    'zh-CN': instancesZhCN,
    ja: instancesJa,
    ko: instancesKo,
  },
  tools: {
    en: toolsEn,
    'zh-CN': toolsZhCN,
    ja: toolsJa,
    ko: toolsKo,
  },
  workflows: {
    en: workflowsEn,
    'zh-CN': workflowsZhCN,
    ja: workflowsJa,
    ko: workflowsKo,
  },
  tasks: {
    en: tasksEn,
    'zh-CN': tasksZhCN,
    ja: tasksJa,
    ko: tasksKo,
  },
  openapi: {
    en: openapiEn,
    'zh-CN': openapiZhCN,
    ja: openapiJa,
    ko: openapiKo,
  },
  calls: {
    en: callsEn,
    'zh-CN': callsZhCN,
    ja: callsJa,
    ko: callsKo,
  },
  traces: {
    en: tracesEn,
    'zh-CN': tracesZhCN,
    ja: tracesJa,
    ko: tracesKo,
  },
  traffic: {
    en: trafficEn,
    'zh-CN': trafficZhCN,
    ja: trafficJa,
    ko: trafficKo,
  },
  stats: {
    en: statsEn,
    'zh-CN': statsZhCN,
    ja: statsJa,
    ko: statsKo,
  },
  governance: {
    en: governanceEn,
    'zh-CN': governanceZhCN,
    ja: governanceJa,
    ko: governanceKo,
  },
  logs: {
    en: logsEn,
    'zh-CN': logsZhCN,
    ja: logsJa,
    ko: logsKo,
  },
  skillPaths: {
    en: skillPathsEn,
    'zh-CN': skillPathsZhCN,
    ja: skillPathsJa,
    ko: skillPathsKo,
  },
  analytics: {
    en: analyticsEn,
    'zh-CN': analyticsZhCN,
    ja: analyticsJa,
    ko: analyticsKo,
  },
} as const;

export type I18nNamespace = keyof typeof I18N_MESSAGES;
export const I18N_NAMESPACES = Object.keys(I18N_MESSAGES) as I18nNamespace[];

type NamespaceMessages<N extends I18nNamespace> = (typeof I18N_MESSAGES)[N]['en'];
type NamespaceLocalKey<N extends I18nNamespace> = Extract<keyof NamespaceMessages<N>, string>;

export type ScopedMessageKey<N extends I18nNamespace = I18nNamespace> = {
  [Key in I18nNamespace]: NamespaceLocalKey<Key> extends never ? never : `${Key}.${NamespaceLocalKey<Key>}`;
}[N];

export type MessageKey = ScopedMessageKey;

export type NamespaceAuditIssue = {
  namespace: I18nNamespace;
  locale: SupportedLocale;
  missingKeys: string[];
  extraKeys: string[];
};

function cleanLocaleTag(tag: string): string {
  return tag.trim().replace(/_/g, '-');
}

function canonicalSupportedLocale(tag: string): SupportedLocale | null {
  if (SUPPORTED_LOCALE_SET.has(tag)) {
    return tag as SupportedLocale;
  }

  const parts = tag.split('-').filter(Boolean);
  const language = parts[0]?.toLowerCase();
  const regionsAndScripts = parts.slice(1).map((part) => part.toLowerCase());

  if (language === 'en') {
    return 'en';
  }
  if (language === 'ja') {
    return 'ja';
  }
  if (language === 'ko') {
    return 'ko';
  }
  if (language === 'zh') {
    if (
      parts.length === 1
      || regionsAndScripts.includes('hans')
      || regionsAndScripts.includes('cn')
      || regionsAndScripts.includes('sg')
    ) {
      return 'zh-CN';
    }
  }

  return null;
}

function splitMessageKey(key: MessageKey): [I18nNamespace, string] {
  const [namespace, ...localParts] = key.split('.');
  return [namespace as I18nNamespace, localParts.join('.')];
}

function interpolate(template: string, values?: InterpolationValues): string {
  if (!values) {
    return template;
  }

  return template.replace(/\{([A-Za-z0-9_.-]+)\}/g, (match, key: string) => {
    const value = values[key];
    return value == null ? match : String(value);
  });
}

export function normalizeLocaleTag(tag: string | null | undefined): SupportedLocale | null {
  if (!tag) {
    return null;
  }
  return canonicalSupportedLocale(cleanLocaleTag(tag));
}

export function detectLocale(options?: {
  override?: string | null;
  navigatorLanguages?: readonly string[] | null;
  navigatorLanguage?: string | null;
  fallback?: SupportedLocale;
}): LocaleDetection {
  const fallback = options?.fallback ?? DEFAULT_LOCALE;
  const override = normalizeLocaleTag(options?.override);
  if (override) {
    return { locale: override, source: 'override', matchedTag: options?.override ?? undefined };
  }

  const candidates = [
    ...(options?.navigatorLanguages ?? []),
    ...(options?.navigatorLanguage ? [options.navigatorLanguage] : []),
  ];
  for (const tag of candidates) {
    const locale = normalizeLocaleTag(tag);
    if (locale) {
      return { locale, source: 'navigator', matchedTag: tag };
    }
  }

  return { locale: fallback, source: 'fallback' };
}

export function detectBrowserLocale(override?: string | null): LocaleDetection {
  if (typeof navigator === 'undefined') {
    return detectLocale({ override });
  }

  return detectLocale({
    override,
    navigatorLanguages: navigator.languages,
    navigatorLanguage: navigator.language,
  });
}

export function translate(locale: SupportedLocale, key: MessageKey, values?: InterpolationValues): string {
  const [namespace, localKey] = splitMessageKey(key);
  const namespaceMessages = I18N_MESSAGES[namespace];
  const localizedMessages = namespaceMessages[locale] as Partial<Record<string, string>>;
  const fallbackMessages = namespaceMessages[DEFAULT_LOCALE] as Partial<Record<string, string>>;
  const localized = localizedMessages[localKey];
  const fallback = fallbackMessages[localKey];
  return interpolate(String(localized ?? fallback ?? key), values);
}

export function createTranslator(locale: SupportedLocale): (key: MessageKey, values?: InterpolationValues) => string {
  return (key, values) => translate(locale, key, values);
}

export function createNamespaceTranslator<N extends I18nNamespace>(
  locale: SupportedLocale,
  namespaces: readonly N[],
): (key: ScopedMessageKey<N>, values?: InterpolationValues) => string {
  const allowed = new Set<I18nNamespace>(namespaces);
  return (key, values) => {
    const [namespace] = splitMessageKey(key as MessageKey);
    if (!allowed.has(namespace)) {
      throw new Error(`Translation key "${key}" is outside requested namespaces: ${namespaces.join(', ')}`);
    }
    return translate(locale, key as MessageKey, values);
  };
}

export function auditI18nNamespaces(): NamespaceAuditIssue[] {
  const issues: NamespaceAuditIssue[] = [];

  for (const namespace of I18N_NAMESPACES) {
    const expectedKeys = new Set(Object.keys(I18N_MESSAGES[namespace][DEFAULT_LOCALE]));
    for (const locale of SUPPORTED_LOCALES) {
      const localeMessages = I18N_MESSAGES[namespace][locale];
      const actualKeys = new Set(Object.keys(localeMessages));
      const missingKeys = [...expectedKeys].filter((key) => !actualKeys.has(key)).sort();
      const extraKeys = [...actualKeys].filter((key) => !expectedKeys.has(key)).sort();
      if (missingKeys.length > 0 || extraKeys.length > 0) {
        issues.push({ namespace, locale, missingKeys, extraKeys });
      }
    }
  }

  return issues;
}
