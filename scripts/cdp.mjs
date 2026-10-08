// A minimal Chrome DevTools Protocol client for the release app (Node 24 has a global WebSocket).
// Start Marky with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 first.

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Connects to the app's page and returns `send` (any CDP method) and `evaluate` (an expression's value). */
export async function connectToPage(port = 9222) {
  const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  const page = targets.find((t) => t.type === 'page');
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  let nextId = 0;
  const pending = new Map();
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data);
    if (!msg.id || !pending.has(msg.id)) return;
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  };
  await new Promise((r) => (ws.onopen = r));

  const send = (method, params = {}) =>
    new Promise((r) => {
      const id = ++nextId;
      pending.set(id, r);
      ws.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) =>
    (await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })).result.result?.value;
  return { send, evaluate, close: () => ws.close() };
}
