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

const EMPTY_MESSAGES = {} as const;

const COMMON_MESSAGES = {
  'action.refresh': 'Refresh',
  'status.loading': 'Loading...',
  'status.labelValue': '{label}: {value}',
} as const;

const CHROME_MESSAGES = {
  'app.title': 'Admin Dashboard',
  'app.subtitle': 'DCC-MCP Gateway',
} as const;

const NAVIGATION_MESSAGES = {
  'docs.label': 'Docs',
  'docs.title': 'Open project docs on GitHub',
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
  'group.onboarding': 'Onboarding',
  'group.operations': 'Operations',
  'group.workspace': 'Workspace',
  'group.contracts': 'Contracts',
  'group.observability': 'Observability',
  'group.configuration': 'Configuration',
} as const;

const SEARCH_MESSAGES = {
  'input.ariaLabel': 'Filter current panel',
  'input.default': 'Search this panel...',
  'input.openapi': 'Filter operations, paths, tags...',
  'input.stats': 'Filter stats charts...',
} as const;

function mirrorNamespace<const T extends Record<string, string>>(messages: T): Record<SupportedLocale, T> {
  return {
    en: messages,
    'zh-CN': messages,
    ja: messages,
    ko: messages,
  };
}

export const I18N_MESSAGES = {
  common: mirrorNamespace(COMMON_MESSAGES),
  chrome: mirrorNamespace(CHROME_MESSAGES),
  navigation: mirrorNamespace(NAVIGATION_MESSAGES),
  search: mirrorNamespace(SEARCH_MESSAGES),
  setup: mirrorNamespace(EMPTY_MESSAGES),
  health: mirrorNamespace(EMPTY_MESSAGES),
  instances: mirrorNamespace(EMPTY_MESSAGES),
  tools: mirrorNamespace(EMPTY_MESSAGES),
  workflows: mirrorNamespace(EMPTY_MESSAGES),
  tasks: mirrorNamespace(EMPTY_MESSAGES),
  openapi: mirrorNamespace(EMPTY_MESSAGES),
  calls: mirrorNamespace(EMPTY_MESSAGES),
  traces: mirrorNamespace(EMPTY_MESSAGES),
  traffic: mirrorNamespace(EMPTY_MESSAGES),
  stats: mirrorNamespace(EMPTY_MESSAGES),
  governance: mirrorNamespace(EMPTY_MESSAGES),
  logs: mirrorNamespace(EMPTY_MESSAGES),
  skillPaths: mirrorNamespace(EMPTY_MESSAGES),
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
