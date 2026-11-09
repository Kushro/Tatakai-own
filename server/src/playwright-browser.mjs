import { createNetwork, publicUrl, limitedText } from './security.mjs';
// Optional headless bridge. Interactive sign-in and Android page/file interfaces
// remain explicit unsupported operations; desktop retains the full Electron bridge.
export class PlaywrightBrowser {
  constructor(chromium) { this.chromium = chromium; this.pages = new Map(); }
  initialize() {
    this.session ||= { getUserAgent: () => 'Mozilla/5.0 TatakaiServer', cookies: {
      get: async ({ url }) => (await this.context()).cookies(url),
      set: async cookie => (await this.context()).addCookies([{ name: cookie.name, value: cookie.value, url: cookie.url, httpOnly: cookie.httpOnly, secure: cookie.secure, sameSite: 'Lax' }]),
    } };
  }
  async context() {
    if (!this.contextPromise) this.contextPromise = (async () => {
      this.browser = await this.chromium.launch({ headless: true });
      const context = await this.browser.newContext({ acceptDownloads: false, permissions: [], serviceWorkers: 'block' });
      await context.route('**/*', async route => {
        try { publicUrl(route.request().url()); await route.continue(); } catch { await route.abort(); }
      });
      return context;
    })();
    return this.contextPromise;
  }
  async handle(event, reply) {
    if (event.op === 'eval') {
      const page = this.pages.get(event.page); if (!page) throw Error('Browser page is unavailable');
      const result = await page.evaluate(String(event.script || '')); return reply('webReply', { rid: event.rid, result: JSON.stringify(result ?? null) });
    }
    if (event.op === 'closePage') { await this.pages.get(event.rid)?.close(); this.pages.delete(event.rid); return; }
    if (!['fetch', 'resolve'].includes(event.op)) throw Error('Interactive browser authentication and Android page APIs require Tatakai desktop');
    publicUrl(event.url);
    if (event.op === 'fetch') {
      const network = createNetwork(); const { response, url } = await network(event.url, { method: event.method || 'GET', headers: event.headers, body: ['GET', 'HEAD'].includes(event.method || 'GET') ? undefined : event.body, signal: AbortSignal.timeout(25000) });
      return reply('webReply', { rid: event.rid, status: response.status, url, headers: Object.fromEntries(response.headers), body: await limitedText(response) });
    }
    const page = await (await this.context()).newPage(); this.pages.set(event.rid, page);
    try {
      await page.goto(event.url, { timeout: 25000, waitUntil: 'domcontentloaded' });
      if (event.script) { const result = await page.evaluate(String(event.script)); await reply('webScript', { rid: event.rid, result: JSON.stringify(result ?? null) }); }
      return reply('webReply', { rid: event.rid });
    } finally { await page.close(); this.pages.delete(event.rid); }
  }
  stop() { void this.browser?.close(); }
}
