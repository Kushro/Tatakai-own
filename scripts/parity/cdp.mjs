// Parity harness for the classic Electron app (relaunch it with --remote-debugging-port=<port>).
// usage: node cdp.mjs <port> <outdir> <name>=<hash-route>[@waitMs] ...
// special values: name=EVAL:<js>  runs JS; name=SHOT captures current page without navigation
import fs from 'node:fs';
const [port, out, ...steps] = process.argv.slice(2);
const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = list.find(t => t.type === 'page');
const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0; const pend = new Map();
ws.onmessage = e => { const m = JSON.parse(e.data); if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); } };
await new Promise(r => ws.onopen = r);
const send = (method, params = {}) => new Promise(r => { const i = ++id; pend.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const ev = async (expr) => (await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true })).result?.result?.value;
const sleep = ms => new Promise(r => setTimeout(r, ms));
async function shot(name, maxH = 4500) {
  const h = await ev(`(()=>{let m=Math.max(document.documentElement.scrollHeight,document.body.scrollHeight);document.querySelectorAll('*').forEach(el=>{const s=getComputedStyle(el);if((s.overflowY==='auto'||s.overflowY==='scroll')&&el.scrollHeight>el.clientHeight+50&&el.clientHeight>400)m=Math.max(m,el.scrollHeight+(el.getBoundingClientRect().top||0));});return m})()`);
  const H = Math.min(Math.max(h || 900, 900), maxH);
  await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: H, deviceScaleFactor: 1, mobile: false });
  await sleep(1500);
  const r = await send('Page.captureScreenshot', { format: 'jpeg', quality: 72, captureBeyondViewport: true });
  await send('Emulation.clearDeviceMetricsOverride');
  fs.writeFileSync(`${out}/${name}.jpg`, Buffer.from(r.result.data, 'base64'));
  console.log('saved', name, 'h=', h, 'title=', await ev('document.title'), 'url=', await ev('location.hash'));
}
for (const s of steps) {
  const eq = s.indexOf('='); const name = s.slice(0, eq); let val = s.slice(eq + 1);
  let wait = 3500; const at = val.lastIndexOf('@'); if (at > 0 && /^\d+$/.test(val.slice(at + 1))) { wait = +val.slice(at + 1); val = val.slice(0, at); }
  if (val.startsWith('EVAL:')) { console.log(name, JSON.stringify(await ev(val.slice(5))).slice(0, 2000)); await sleep(wait); continue; }
  if (val.startsWith('CLICK:')) { const t=val.slice(6); console.log('click', t, await ev(`(()=>{const b=[...document.querySelectorAll('button,a,[role=tab],[role=button]')].find(b=>b.innerText.trim()===${JSON.stringify(t)}); if(!b) return false; b.click(); return true})()`)); await sleep(wait); }
  else if (val !== 'SHOT') { await ev(`location.hash = ${JSON.stringify('#' + val)}`); await sleep(wait); }
  await shot(name);
}
ws.close();
