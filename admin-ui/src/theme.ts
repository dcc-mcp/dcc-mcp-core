/// Color-scheme (light / dark / system) preference handling.
///
/// The palette is driven entirely by CSS custom properties; switching themes
/// just toggles a `.dark` class on `<html>` and updates `color-scheme`. The
/// `system` mode follows the OS preference live via `matchMedia`.

export const THEMES = ['light', 'dark', 'system'] as const;
export type ThemeMode = (typeof THEMES)[number];
export type ResolvedTheme = 'light' | 'dark';

export const THEME_STORAGE_KEY = 'dcc-mcp-admin-theme';

const THEME_ID_SET = new Set<string>(THEMES);

export function isThemeMode(value: string | null | undefined): value is ThemeMode {
  return value != null && THEME_ID_SET.has(value);
}

/// The dashboard's primary identity is the dark / black scheme, so an
/// unset preference defaults to `dark` rather than following the OS.
export const DEFAULT_THEME_MODE: ThemeMode = 'dark';

/// Read the persisted preference, falling back to the dark default when
/// unset or when storage is unavailable (hardened embedded views).
export function readThemeMode(): ThemeMode {
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    return isThemeMode(stored) ? stored : DEFAULT_THEME_MODE;
  } catch {
    return DEFAULT_THEME_MODE;
  }
}

export function storeThemeMode(mode: ThemeMode): void {
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, mode);
  } catch {
    // localStorage may be unavailable; the in-memory choice still applies.
  }
}

export function prefersDark(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-color-scheme: dark)').matches
  );
}

/// Resolve a mode to a concrete light/dark value, consulting the OS for
/// `system`.
export function resolveTheme(mode: ThemeMode): ResolvedTheme {
  if (mode === 'system') {
    return prefersDark() ? 'dark' : 'light';
  }
  return mode;
}

/// Apply a resolved theme to the document: toggle the `.dark` class used by
/// the CSS variable overrides and set `color-scheme` so native controls
/// (scrollbars, form widgets) match.
export function applyTheme(resolved: ResolvedTheme): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.classList.toggle('dark', resolved === 'dark');
  root.dataset.adminTheme = resolved;
  root.style.colorScheme = resolved;
}
