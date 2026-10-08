/**
 * Runs async tasks one at a time, in the order they were queued. Tauri's async commands can
 * finish out of order, and some of them share a file on the Rust side. A failed task goes to
 * `onError` and does not stop the ones after it.
 */
export function createSerialQueue(onError: (error: unknown) => void) {
  let tail: Promise<unknown> = Promise.resolve();
  return {
    push(task: () => Promise<unknown>): void {
      tail = tail.then(task).catch(onError);
    },
    /** Resolves once every task queued so far has finished. */
    drained(): Promise<unknown> {
      return tail;
    },
  };
}
