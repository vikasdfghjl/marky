import { api } from '../platform/ipc';

// Startup instrumentation for the cold-start budget. The Rust side writes the
// timeline to MARKY_PERF_LOG when set (see scripts/bench.ps1).

const longTasks: string[] = [];
new PerformanceObserver((list) => {
  for (const t of list.getEntries()) longTasks.push(`${Math.round(t.startTime)}+${Math.round(t.duration)}`);
}).observe({ type: 'longtask', buffered: true });

/** Calls `onMeasured` with ms from process start to the first painted frame after the editor is ready. */
export function measureColdStart(onMeasured: (startupMs: number) => void) {
  requestAnimationFrame(() => {
    performance.mark('first-frame');
    // A message posted from rAF runs after that frame's paint.
    const afterPaint = new MessageChannel();
    afterPaint.port1.onmessage = async () => {
      performance.mark('painted');
      onMeasured(await api.startupMs(performance.now(), timeline()));
    };
    afterPaint.port2.postMessage(null);
  });
}

function timeline(): string {
  const nav = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming | undefined;
  const marks = performance.getEntriesByType('mark').map((m) => `${m.name}=${Math.round(m.startTime)}`);
  if (nav) marks.unshift(`html=${Math.round(nav.responseEnd)}`, `dcl=${Math.round(nav.domContentLoadedEventEnd)}`);
  if (longTasks.length) marks.push(`longtasks=${longTasks.join(',')}`);
  return marks.join(' ');
}
