// A minimal Chrome DevTools Protocol client for the release app (Node 24 has a global WebSocket).
// Start Marky with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 first.

export const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

interface CdpTarget {
  type: string;
  webSocketDebuggerUrl: string;
}

interface CdpMessage {
  id?: number;
  result?: { result?: { value?: unknown } };
}

/** Connects to the app's page and returns `send` (any CDP method) and `evaluate` (an expression's value). */
export async function connectToPage(port = 9222) {
  const targets = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()) as CdpTarget[];
  const page = targets.find((t) => t.type === 'page');
  if (!page) throw new Error(`no page target on port ${port}`);
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  let nextId = 0;
  const pending = new Map<number, (msg: CdpMessage) => void>();
  ws.onmessage = (e) => {
    const msg = JSON.parse(String(e.data)) as CdpMessage;
    if (!msg.id || !pending.has(msg.id)) return;
    pending.get(msg.id)!(msg);
    pending.delete(msg.id);
  };
  await new Promise<void>((r) => {
    ws.onopen = () => r();
  });

  const send = (method: string, params: Record<string, unknown> = {}) =>
    new Promise<CdpMessage>((resolve) => {
      const id = ++nextId;
      pending.set(id, resolve);
      ws.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression: string) =>
    (await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })).result?.result?.value;
  return { send, evaluate, close: () => ws.close() };
}
