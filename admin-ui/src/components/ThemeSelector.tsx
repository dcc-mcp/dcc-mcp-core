import { RiContrast2Line } from '@remixicon/react';
import { type InterpolationValues, type MessageKey } from '../i18n';
import { THEMES, type ThemeMode } from '../theme';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
} from './ui/select';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

type ThemeSelectorProps = {
  mode: ThemeMode;
  onChange: (mode: ThemeMode) => void;
  t: Translator;
};

const THEME_LABEL_KEY: Record<ThemeMode, MessageKey> = {
  light: 'common.theme.light',
  dark: 'common.theme.dark',
  system: 'common.theme.system',
};

const THEME_TRIGGER_LABEL_KEY: Record<ThemeMode, MessageKey> = {
  light: 'common.theme.triggerLight',
  dark: 'common.theme.triggerDark',
  system: 'common.theme.triggerSystem',
};

export function ThemeSelector({ mode, onChange, t }: ThemeSelectorProps) {
  return (
    <div className="theme-selector" title={t('common.theme.label')}>
      <RiContrast2Line className="preference-icon" aria-hidden="true" />
      <span id="admin-theme-select-label" className="preference-label">{t('common.theme.label')}</span>
      <Select
        value={mode}
        onValueChange={(value) => onChange(value as ThemeMode)}
      >
        <SelectTrigger
          id="admin-theme-select"
          className="admin-select-trigger preference-select-trigger"
          size="sm"
          aria-label={`${t('common.theme.label')}: ${t(THEME_LABEL_KEY[mode])}`}
        >
          <span className="preference-select-visible-value" aria-hidden="true">
            {t(THEME_TRIGGER_LABEL_KEY[mode])}
          </span>
        </SelectTrigger>
        <SelectContent
          className="admin-select-content preference-select-content"
          position="popper"
          align="start"
        >
          <SelectGroup>
            {THEMES.map((option) => (
              <SelectItem key={option} value={option}>
                {t(THEME_LABEL_KEY[option])}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
  );
}
