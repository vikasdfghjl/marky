// A minimal Chrome DevTools Protocol client for the release app (Node 24 has a global WebSocket).
// Start Marky with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 first.

export const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

interface CdpTarget {
  type: string;
  webSocketDebuggerUrl: string;
}

interface CdpMessage {
  id?: number;
  error?: { message: string };
  result?: { result?: { value?: unknown } };
}

interface PendingRequest {
  resolve(message: CdpMessage): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
}

const REQUEST_TIMEOUT_MS = 10_000;

/** Connects to the app's page and returns `send` (any CDP method) and `evaluate` (an expression's value). */
export async function connectToPage(port = 9222, timeoutMs = REQUEST_TIMEOUT_MS) {
  const controller = new AbortController();
  const fetchTimer = setTimeout(() => controller.abort(), timeoutMs);
  let targets: CdpTarget[];
  try {
    const response = await fetch(`http://127.0.0.1:${port}/json`, { signal: controller.signal });
    if (!response.ok) throw new Error(`DevTools on port ${port} returned HTTP ${response.status}`);
    targets = (await response.json()) as CdpTarget[];
  } catch (error) {
    if (error instanceof Error && error.message.startsWith('DevTools on port')) throw error;
    throw new Error(`could not read DevTools targets on port ${port} within ${timeoutMs} ms`, { cause: error });
  } finally {
    clearTimeout(fetchTimer);
  }
  const page = targets.find((t) => t.type === 'page');
  if (!page) throw new Error(`no page target on port ${port}`);
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  let nextId = 0;
  let opened = false;
  const pending = new Map<number, PendingRequest>();
  const failPending = (error: Error) => {
    for (const request of pending.values()) {
      clearTimeout(request.timer);
      request.reject(error);
    }
    pending.clear();
  };

  ws.onmessage = (e) => {
    const msg = JSON.parse(String(e.data)) as CdpMessage;
    if (!msg.id) return;
    const request = pending.get(msg.id);
    if (!request) return;
    clearTimeout(request.timer);
    pending.delete(msg.id);
    if (msg.error) request.reject(new Error(`DevTools request failed: ${msg.error.message}`));
    else request.resolve(msg);
  };
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      ws.close();
      reject(new Error(`DevTools WebSocket did not open within ${timeoutMs} ms`));
    }, timeoutMs);
    ws.onopen = () => {
      opened = true;
      clearTimeout(timer);
      resolve();
    };
    ws.onerror = () => {
      clearTimeout(timer);
      const error = new Error('DevTools WebSocket connection failed');
      failPending(error);
      reject(error);
    };
    ws.onclose = () => {
      clearTimeout(timer);
      const error = new Error('DevTools WebSocket closed');
      failPending(error);
      if (!opened) reject(error);
    };
  });

  const send = (method: string, params: Record<string, unknown> = {}) =>
    new Promise<CdpMessage>((resolve, reject) => {
      if (ws.readyState !== WebSocket.OPEN) {
        reject(new Error(`cannot send ${method}: DevTools WebSocket is not open`));
        return;
      }
      const id = ++nextId;
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`DevTools request ${method} timed out after ${REQUEST_TIMEOUT_MS} ms`));
      }, REQUEST_TIMEOUT_MS);
      pending.set(id, { resolve, reject, timer });
      try {
        ws.send(JSON.stringify({ id, method, params }));
      } catch (error) {
        clearTimeout(timer);
        pending.delete(id);
        reject(new Error(`could not send DevTools request ${method}`, { cause: error }));
      }
    });
  const evaluate = async (expression: string) =>
    (await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })).result?.result?.value;
  return {
    send,
    evaluate,
    close: () => {
      failPending(new Error('DevTools connection closed by client'));
      ws.close();
    },
  };
}
