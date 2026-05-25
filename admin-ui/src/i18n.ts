export const SUPPORTED_LOCALES = ['en', 'zh-CN', 'ja', 'ko'] as const;

export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

export type LocaleDetectionSource = 'override' | 'navigator' | 'fallback';

export type LocaleDetection = {
  locale: SupportedLocale;
  source: LocaleDetectionSource;
  matchedTag?: string;
};

export const DEFAULT_LOCALE: SupportedLocale = 'en';

const SUPPORTED_LOCALE_SET = new Set<string>(SUPPORTED_LOCALES);

const EN_MESSAGES = {
  'app.title': 'Admin Dashboard',
  'app.subtitle': 'DCC-MCP Gateway',
  'action.refresh': 'Refresh',
  'nav.docs': 'Docs',
  'nav.docs.title': 'Open project docs on GitHub',
  'panel.setup': 'Connect IDE',
  'panel.debug': 'Debug',
  'panel.instances': 'Instances',
  'panel.activity': 'Activity',
  'panel.health': 'Health',
  'panel.workflows': 'Workflows',
  'panel.tasks': 'Tasks',
  'panel.tools': 'Tools',
  'panel.openapi': 'OpenAPI Inspector',
  'panel.stats': 'Stats',
  'panel.governance': 'Governance',
  'panel.traffic': 'Traffic',
  'panel.traces': 'Traces',
  'panel.calls': 'Calls',
  'panel.logs': 'Logs',
  'panel.skillPaths': 'Skills',
  'panelGroup.onboarding': 'Onboarding',
  'panelGroup.operations': 'Operations',
  'panelGroup.workspace': 'Workspace',
  'panelGroup.contracts': 'Contracts',
  'panelGroup.observability': 'Observability',
  'panelGroup.configuration': 'Configuration',
  'search.ariaLabel': 'Filter current panel',
  'search.default': 'Search this panel...',
  'search.openapi': 'Filter operations, paths, tags...',
  'search.stats': 'Filter stats charts...',
  'status.loading': 'Loading...',
} as const;

export type MessageKey = keyof typeof EN_MESSAGES;

type LocaleMessages = Partial<Record<MessageKey, string>>;

const LOCALE_MESSAGES: Record<SupportedLocale, LocaleMessages> = {
  en: EN_MESSAGES,
  'zh-CN': {},
  ja: {},
  ko: {},
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

export function translate(locale: SupportedLocale, key: MessageKey): string {
  return LOCALE_MESSAGES[locale]?.[key] ?? EN_MESSAGES[key];
}

export function createTranslator(locale: SupportedLocale): (key: MessageKey) => string {
  return (key) => translate(locale, key);
}
