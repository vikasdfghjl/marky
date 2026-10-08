import { api } from './ipc';

/** Rust caps marky.log too; this keeps an error loop from flooding IPC. */
const MAX_REPORTS_PER_SESSION = 20;
let reports = 0;

/** Sends one error to marky.log (diagnostics.rs). Never throws: logging must not break its caller. */
export function logError(context: string, error: unknown) {
  if (reports >= MAX_REPORTS_PER_SESSION) return;
  reports++;
  const detail = error instanceof Error ? (error.stack ?? error.message) : String(error);
  api.logError(`${context}: ${detail}`).catch(() => {});
}

/** Uncaught errors and rejected promises on the page go to the log instead of vanishing. */
export function logUncaughtErrors() {
  window.addEventListener('error', (e) => logError('uncaught error', e.error ?? e.message));
  window.addEventListener('unhandledrejection', (e) => logError('unhandled rejection', e.reason));
}
