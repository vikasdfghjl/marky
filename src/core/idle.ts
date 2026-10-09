import { api } from '../platform/ipc';

// After IDLE_AFTER_MS without input Rust trims WebView2's memory (memory.rs); the next input
// restores it. Only the two transitions cross IPC, never the input itself.

const IDLE_AFTER_MS = 30_000;
/** The idle check is a poll, not a timer reset per event: scrolling can fire hundreds of events a second. */
const CHECK_EVERY_MS = 5_000;
// Not `pointermove`: hovering needs no memory, and a resting pointer still produces moves
// (Chromium sends synthetic ones after layout changes), which kept waking a trimmed window.
const ACTIVITY_EVENTS = ['keydown', 'pointerdown', 'wheel'] as const;

/** Returns `noteActivity`, for use that is not DOM input (menu commands, opened files, disk changes). */
export function trimMemoryWhenIdle(): () => void {
  let lastActivityAt = performance.now();
  let idle = false;

  function noteActivity() {
    lastActivityAt = performance.now();
    if (!idle) return;
    idle = false;
    void api.setIdle(false);
  }

  window.setInterval(() => {
    if (idle || performance.now() - lastActivityAt < IDLE_AFTER_MS) return;
    idle = true;
    void api.setIdle(true);
  }, CHECK_EVERY_MS);

  for (const type of ACTIVITY_EVENTS) window.addEventListener(type, noteActivity, { capture: true, passive: true });
  return noteActivity;
}
