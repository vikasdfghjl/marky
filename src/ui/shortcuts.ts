/** A reload would silently drop every buffer; there is nothing to refresh. */
const BLOCKED_KEYS = new Set(['F5', 'Ctrl+F5', 'Ctrl+Shift+R']);

/** "Ctrl+Shift+S" style name for a key press. */
function keyChord(e: KeyboardEvent): string {
  const key = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  return [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', key].filter(Boolean).join('+');
}

/**
 * Routes app shortcuts to `run`, in the capture phase so they win over the editor
 * and the webview's defaults. Reload keys are swallowed.
 */
export function listenForShortcuts<Command extends string>(shortcuts: Record<string, Command>, run: (command: Command) => void) {
  window.addEventListener(
    'keydown',
    (e) => {
      const chord = keyChord(e);
      const command = shortcuts[chord];
      if (!command && !BLOCKED_KEYS.has(chord)) return;
      e.preventDefault();
      e.stopPropagation();
      if (command) run(command);
    },
    { capture: true },
  );
}
