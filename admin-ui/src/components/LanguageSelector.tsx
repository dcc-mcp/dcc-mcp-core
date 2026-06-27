import { RiTranslate2 } from '@remixicon/react';
import { SUPPORTED_LOCALES, type InterpolationValues, type MessageKey, type SupportedLocale } from '../i18n';
import { LOCALE_LABELS, LOCALE_TRIGGER_LABELS } from '../locale';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
} from './ui/select';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

type LanguageSelectorProps = {
  locale: SupportedLocale;
  source: string;
  onChange: (locale: SupportedLocale) => void;
  t: Translator;
};

export function LanguageSelector({ locale, source, onChange, t }: LanguageSelectorProps) {
  const sourceLabel = t('common.language.source', { source });
  const selectedLabel = LOCALE_LABELS[locale];
  return (
    <div className="language-selector" title={sourceLabel}>
      <RiTranslate2 className="preference-icon" aria-hidden="true" />
      <span id="admin-locale-select-label" className="preference-label">{t('common.language.label')}</span>
      <Select
        value={locale}
        onValueChange={(value) => onChange(value as SupportedLocale)}
      >
        <SelectTrigger
          id="admin-locale-select"
          className="admin-select-trigger preference-select-trigger"
          size="sm"
          aria-label={`${t('common.language.label')}: ${selectedLabel}`}
        >
          <span className="preference-select-visible-value" aria-hidden="true">
            {LOCALE_TRIGGER_LABELS[locale]}
          </span>
        </SelectTrigger>
        <SelectContent
          className="admin-select-content preference-select-content"
          position="popper"
          align="start"
        >
          <SelectGroup>
            {SUPPORTED_LOCALES.map((option) => (
              <SelectItem key={option} value={option}>
                {LOCALE_LABELS[option]}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
  );
}
