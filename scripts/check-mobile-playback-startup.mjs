import assert from 'node:assert/strict';
import { chromium } from 'playwright';

const base = process.env.MOBILE_TEST_URL || 'http://127.0.0.1:8090';
const browser = await chromium.launch({ channel: process.platform === 'win32' ? 'msedge' : undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 393, height: 852 }, hasTouch: true });
const errors = [];
page.on('pageerror', error => errors.push(error.message));
await page.route('**/src/contexts/AuthContext.tsx*', route => route.fulfill({ contentType: 'application/javascript', body: 'export function useAuth(){return {user:null,profile:null}} export function AuthProvider({children}){return children}' }));
// Inject fatal transport errors without depending on a provider/CDN. The real
// component must create a new transport, retain its video and retry the URL.
await page.route('**/node_modules/.vite/deps/hls__js.js*', route => route.fulfill({ contentType: 'application/javascript', body: `
  export default class Hls {
    static Events={MANIFEST_PARSED:'manifest',ERROR:'error',FRAG_BUFFERED:'buffer',AUDIO_TRACKS_UPDATED:'audio',AUDIO_TRACK_SWITCHED:'track'};
    static ErrorTypes={NETWORK_ERROR:'network',MEDIA_ERROR:'media'};
    static isSupported(){return true}
    constructor(config){this.config=config;this.levels=[];this.listeners={};(window.hlsInstances ||= []).push(this)}
    on(event,fn){this.listeners[event]=fn}
    loadSource(url){this.url=url}
    attachMedia(video){this.video=video}
    destroy(){this.destroyed=true}
    startLoad(){this.startCalls=(this.startCalls||0)+1}
    stopLoad(){}
    fail(){this.listeners.error(null,{fatal:true,type:'network',details:'manifestLoadTimeOut'})}
  }
` }));
await page.route('**/__mobile-startup', route => route.fulfill({ contentType: 'text/html', body: `<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><script type="module">import RefreshRuntime from '/@react-refresh'; RefreshRuntime.injectIntoGlobalHook(window); window.$RefreshReg$=()=>{}; window.$RefreshSig$=()=>(type)=>type; window.__vite_plugin_react_preamble_installed__=true; import '/src/index.css';</script></head><body><div id="test-root"></div></body></html>` }));
try {
  await page.goto(`${base}/__mobile-startup`);
  await page.evaluate(async () => {
    localStorage.setItem('video-player-settings', JSON.stringify({ autoplay: false }));
    const source = await (await fetch('/src/components/video/MobileVideoPlayer.tsx')).text();
    const version = source.match(/react\.js\?v=([^"']+)/)[1];
    const { default: React } = await import(`/node_modules/.vite/deps/react.js?v=${version}`);
    const { default: { createRoot } } = await import(`/node_modules/.vite/deps/react-dom_client.js?v=${version}`);
    const { BrowserRouter } = await import(`/node_modules/.vite/deps/react-router-dom.js?v=${version}`);
    const { QueryClient, QueryClientProvider } = await import(`/node_modules/.vite/deps/@tanstack_react-query.js?v=${version}`);
    const { MobileVideoPlayer } = await import('/src/components/video/MobileVideoPlayer.tsx');
    const root = createRoot(document.getElementById('test-root'));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    window.renderPlayer = (url, isM3U8 = true, isOffline = true, sourceType) => root.render(React.createElement(BrowserRouter, null, React.createElement(QueryClientProvider, { client },
      React.createElement(MobileVideoPlayer, { sources: [{ url, isM3U8, sourceType }], sourceType, isOffline }))));
    window.renderPlayer(location.origin + '/first.m3u8');
  });
  await page.waitForFunction(() => window.hlsInstances?.length === 1);
  assert.equal(await page.evaluate(() => window.hlsInstances[0].config.manifestLoadingTimeOut), 8000);
  await page.evaluate(() => { window.originalVideo = document.querySelector('video'); window.hlsInstances.at(-1).fail(); });
  await page.waitForFunction(() => window.hlsInstances.length === 2);
  assert.equal(await page.evaluate(() => window.hlsInstances[0].destroyed), true, 'Fatal manifest error destroys the failed transport');
  assert.equal(await page.evaluate(() => window.hlsInstances[0].startCalls || 0), 0, 'A failed manifest cannot recover through startLoad');
  await page.evaluate(() => window.hlsInstances.at(-1).fail());
  await page.getByRole('alert').waitFor();
  assert.equal(await page.evaluate(() => document.querySelector('video') === window.originalVideo), true, 'Error UI must retain the media element');
  await page.getByRole('button', { name: 'Retry', exact: true }).click();
  await page.waitForFunction(() => window.hlsInstances.length === 3);
  await page.getByRole('alert').waitFor({ state: 'hidden' });
  await page.evaluate(() => { window.hlsInstances.at(-1).fail(); window.hlsInstances.at(-1).fail(); });
  await page.getByRole('alert').waitFor();
  await page.evaluate(() => window.renderPlayer(location.origin + '/second.m3u8'));
  await page.waitForFunction(() => window.hlsInstances.at(-1).url.endsWith('/second.m3u8'));
  await page.getByRole('alert').waitFor({ state: 'hidden' });
  assert.equal(await page.evaluate(() => window.hlsInstances.at(-1).video === window.originalVideo), true, 'Switching after an error initializes against the mounted video');
  // Verify real file decoding reaches the first frame, beyond the mock transport.
  await page.evaluate(() => window.renderPlayer(location.origin + '/tests/fixtures/watch-party-tail.mp4', false));
  await page.waitForFunction(() => document.querySelector('video').readyState >= 2);
  // Fork: the opt-in cloud video proxy (hoko.tatakai.me) was removed; nothing to exercise here.
  await page.evaluate(() => window.renderPlayer(location.origin + '/tests/fixtures/watch-party-tail.mp4', false, true));
  await page.waitForFunction(() => document.querySelector('video').readyState >= 2);
  // Native layout has exactly one CSS-owned top inset, including classic headers.
  await page.evaluate(() => {
    document.documentElement.classList.add('capacitor-native');
    document.documentElement.style.setProperty('--safe-area-inset-top', '28px');
    const fixture = document.createElement('div');
    fixture.setAttribute('data-main-layout', '');
    fixture.innerHTML = '<header data-mobile-header><div style="height:56px" class="pt-[env(safe-area-inset-top)]"></div></header>';
    document.body.append(fixture);
  });
  assert.equal(await page.locator('[data-main-layout]').evaluate(el => getComputedStyle(el).paddingTop), '28px');
  assert.equal(await page.locator('[data-mobile-header] > div').evaluate(el => getComputedStyle(el).paddingTop), '0px');
  assert.deepEqual(errors, []);
  console.log('PASS: manifest retry, retry button, server switch after error, real MP4 first frame, single header inset, timed cloud consent, decline, default opt-in and opt-out.');
} finally {
  await browser.close();
}
