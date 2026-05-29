import { type InterpolationValues, type MessageKey } from '../i18n';
import { THEMES, type ThemeMode } from '../theme';

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

export function ThemeSelector({ mode, onChange, t }: ThemeSelectorProps) {
  return (
    <label className="theme-selector" htmlFor="admin-theme-select">
      <span>{t('common.theme.label')}</span>
      <select
        id="admin-theme-select"
        value={mode}
        aria-label={t('common.theme.label')}
        onChange={(event) => onChange(event.target.value as ThemeMode)}
      >
        {THEMES.map((option) => (
          <option key={option} value={option}>
            {t(THEME_LABEL_KEY[option])}
          </option>
        ))}
      </select>
    </label>
  );
}
