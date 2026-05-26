import { SUPPORTED_LOCALES, type InterpolationValues, type MessageKey, type SupportedLocale } from '../i18n';
import { LOCALE_LABELS } from '../locale';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

type LanguageSelectorProps = {
  locale: SupportedLocale;
  source: string;
  onChange: (locale: SupportedLocale) => void;
  t: Translator;
};

export function LanguageSelector({ locale, source, onChange, t }: LanguageSelectorProps) {
  return (
    <label className="language-selector" htmlFor="admin-locale-select">
      <span>{t('common.language.label')}</span>
      <select
        id="admin-locale-select"
        value={locale}
        aria-label={t('common.language.label')}
        onChange={(event) => onChange(event.target.value as SupportedLocale)}
      >
        {SUPPORTED_LOCALES.map((option) => (
          <option key={option} value={option}>
            {LOCALE_LABELS[option]}
          </option>
        ))}
      </select>
      <span className="language-source">{t('common.language.source', { source })}</span>
    </label>
  );
}
