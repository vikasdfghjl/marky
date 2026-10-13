import { LINE_ENDINGS, type LineEnding } from '../platform/ipc';
import { THEME_SETTINGS, type ThemeSetting } from './theme';

export interface Settings {
  theme: ThemeSetting;
  /** Editor font size in px. */
  fontSize: number;
  wordWrap: boolean;
  /** Preview turns quotes, dashes, "..." and (c)/(tm)/(r) into typographic characters. */
  typographer: boolean;
  /** Save files (not untitled documents) shortly after typing stops. PRD F13: off by default. */
  autoSave: boolean;
  newFileLineEnding: LineEnding;
  /** Off gives a preview-only reading view; the preview then shows even if showPreview is off. */
  showEditor: boolean;
  showPreview: boolean;
  showOutline: boolean;
}

const DEFAULT_SETTINGS: Settings = {
  theme: 'system',
  fontSize: 14,
  wordWrap: true,
  typographer: true,
  autoSave: false,
  newFileLineEnding: 'crlf',
  showEditor: true,
  showPreview: true,
  showOutline: false,
};

export const FONT_SIZES = [12, 13, 14, 15, 16, 18, 20] as const;

/**
 * Reads saved settings defensively: the file may be missing, from an older version,
 * or hand-edited, so every field falls back to its default on its own.
 */
export function readSettings(raw: unknown): Settings {
  const saved = (typeof raw === 'object' && raw !== null ? raw : {}) as Partial<Record<keyof Settings, unknown>>;
  const pick = <T>(value: unknown, allowed: readonly T[], fallback: T): T =>
    allowed.includes(value as T) ? (value as T) : fallback;
  const flag = (value: unknown, fallback: boolean) => (typeof value === 'boolean' ? value : fallback);

  return {
    theme: pick(saved.theme, THEME_SETTINGS, DEFAULT_SETTINGS.theme),
    fontSize: pick(saved.fontSize, FONT_SIZES, DEFAULT_SETTINGS.fontSize as (typeof FONT_SIZES)[number]),
    wordWrap: flag(saved.wordWrap, DEFAULT_SETTINGS.wordWrap),
    typographer: flag(saved.typographer, DEFAULT_SETTINGS.typographer),
    autoSave: flag(saved.autoSave, DEFAULT_SETTINGS.autoSave),
    newFileLineEnding: pick(saved.newFileLineEnding, LINE_ENDINGS, DEFAULT_SETTINGS.newFileLineEnding),
    showEditor: flag(saved.showEditor, DEFAULT_SETTINGS.showEditor),
    showPreview: flag(saved.showPreview, DEFAULT_SETTINGS.showPreview),
    showOutline: flag(saved.showOutline, DEFAULT_SETTINGS.showOutline),
  };
}

export type SettingsStore = ReturnType<typeof createSettingsStore>;

/**
 * Current settings plus change notification. Every change is saved at once; settings
 * change rarely, so there is nothing to debounce.
 */
export function createSettingsStore(save: (settings: Settings) => void) {
  let current = DEFAULT_SETTINGS;
  const listeners = new Set<(settings: Settings, changed: Partial<Settings>) => void>();

  return {
    get: () => current,
    update(changes: Partial<Settings>) {
      current = { ...current, ...changes };
      save(current);
      listeners.forEach((listener) => listener(current, changes));
    },
    /** Applies settings read at startup: every listener runs, nothing is saved back. */
    load(settings: Settings) {
      current = settings;
      listeners.forEach((listener) => listener(current, current));
    },
    onChange(listener: (settings: Settings, changed: Partial<Settings>) => void) {
      listeners.add(listener);
    },
  };
}
