// WebKit remote inspector driver. usage: node wk.mjs <outdir> step...
// step: name=CLICK:<text>[@ms] | name=EVAL:<js>[@ms] | name=SHOT | name=FULL (expand scrollers then shot)
import fs from 'node:fs';
const [out, ...steps] = process.argv.slice(2);
const ws = new WebSocket('ws://127.0.0.1:9334/socket/1/1/WebPage');
let id = 0, tid = null; const pend = new Map(); let targetResolve; const targetP = new Promise(r => targetResolve = r);
ws.onmessage = e => { const m = JSON.parse(e.data);
  if (m.method === 'Target.targetCreated') { tid = m.params.targetInfo.targetId; targetResolve(); }
  if (m.method === 'Target.dispatchMessageFromTarget') { const inner = JSON.parse(m.params.message); if (inner.id && pend.has(inner.id)) { pend.get(inner.id)(inner); pend.delete(inner.id); } }
  if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); } };
await new Promise(r => ws.onopen = r);
await Promise.race([targetP, new Promise(r => setTimeout(r, 3000))]);
const send = (method, params = {}) => new Promise(r => { const i = ++id; pend.set(i, r);
  const msg = JSON.stringify({ id: i, method, params });
  if (tid) ws.send(JSON.stringify({ id: 100000 + i, method: 'Target.sendMessageToTarget', params: { targetId: tid, message: msg } })); else ws.send(msg); });
const ev = async (expr) => { const r = await send('Runtime.evaluate', { expression: expr, returnByValue: true }); return r.result?.result?.value ?? r.error ?? r.result?.wasThrown; };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const EXPAND = `(()=>{const els=[...document.querySelectorAll('*')].filter(el=>{const s=getComputedStyle(el);return (s.overflowY==='auto'||s.overflowY==='scroll')&&el.scrollHeight>el.clientHeight+20&&el.clientHeight>300});let H=document.documentElement.clientHeight;window.__exp=[];for(const el of els){const extra=el.scrollHeight-el.clientHeight;H=Math.max(H,document.documentElement.clientHeight+extra);}
document.documentElement.style.height=H+'px';document.body.style.height=H+'px';
for(const el of els){window.__exp.push(el);if(el.dataset.oh===undefined)el.dataset.oh=el.style.cssText;el.style.overflow='visible';el.style.height=el.scrollHeight+'px';el.style.maxHeight='none';let p=el.parentElement;while(p&&p!==document.documentElement){if(p.dataset.oh2===undefined)p.dataset.oh2=p.style.cssText;p.style.height='auto';p.style.maxHeight='none';p.style.overflow='visible';p=p.parentElement;}}
return Math.min(H,9000)})()`;
const RESTORE = `(()=>{document.querySelectorAll('[data-oh]').forEach(e=>{e.style.cssText=e.dataset.oh;delete e.dataset.oh});document.querySelectorAll('[data-oh2]').forEach(e=>{e.style.cssText=e.dataset.oh2;delete e.dataset.oh2});document.documentElement.style.height='';document.body.style.height='';return 1})()`;
async function shot(name, full) {
  let W = await ev('document.documentElement.clientWidth'), H = await ev('document.documentElement.clientHeight');
  if (full) { H = await ev(EXPAND); await sleep(800); }
  const r = await send('Page.snapshotRect', { x: 0, y: 0, width: W, height: H, coordinateSystem: 'Page' });
  if (full) await ev(RESTORE);
  const url = r.result?.dataURL; if (!url) { console.log('fail', name, JSON.stringify(r).slice(0, 300)); return; }
  fs.writeFileSync(`${out}/${name}.png`, Buffer.from(url.split(',')[1], 'base64')); console.log('saved', name, W + 'x' + H);
}
for (const s of steps) {
  const eq = s.indexOf('='); const name = s.slice(0, eq); let val = s.slice(eq + 1);
  let wait = 3000; const at = val.lastIndexOf('@'); if (at > 0 && /^\d+$/.test(val.slice(at + 1))) { wait = +val.slice(at + 1); val = val.slice(0, at); }
  if (val.startsWith('EVAL:')) { console.log(name, JSON.stringify(await ev(val.slice(5))).slice(0, 3000)); await sleep(wait); continue; }
  if (val.startsWith('CLICK:')) { const t = val.slice(6); console.log('click', t, await ev(`(()=>{const c=[...document.querySelectorAll('button,a,[role=tab],[role=button],[role=link],li,[onclick]')];const b=c.find(b=>(b.innerText||'').trim()===${JSON.stringify(t)})||c.find(b=>(b.getAttribute('aria-label')||b.title||'')===${JSON.stringify(t)})||c.find(b=>(b.innerText||'').trim().startsWith(${JSON.stringify(t)}));if(!b)return false;b.click();return true})()`)); await sleep(wait); }
  if (name !== '_') await shot(name, val !== 'SHOT');
}
ws.close(); process.exit(0);
