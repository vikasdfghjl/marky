// The one place that knows whether the app is dark. CSS picks colours from the root's
// color-scheme (light-dark() tokens in styles.css); code that renders its own colours
// (Mermaid) asks isDark() and listens with onThemeChange().

export const THEME_SETTINGS = ['system', 'light', 'dark'] as const;
export type ThemeSetting = (typeof THEME_SETTINGS)[number];

const systemDark = window.matchMedia('(prefers-color-scheme: dark)');
const listeners = new Set<() => void>();
let setting: ThemeSetting = 'system';

export function isDark(): boolean {
  return setting === 'dark' || (setting === 'system' && systemDark.matches);
}

export function applyTheme(next: ThemeSetting) {
  if (next === setting) return;
  const wasDark = isDark();
  setting = next;
  if (next === 'system') delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = next;
  if (isDark() !== wasDark) listeners.forEach((listener) => listener());
}

export function onThemeChange(listener: () => void) {
  listeners.add(listener);
}

systemDark.addEventListener('change', () => {
  if (setting === 'system') listeners.forEach((listener) => listener());
});
