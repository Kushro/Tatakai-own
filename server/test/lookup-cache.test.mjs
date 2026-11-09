import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createLookupCache } from '../src/lookup-cache.mjs';

test('large chapter lists are cached and concurrent scrapes coalesce', async () => {
  const cache = createLookupCache(); let calls = 0, release;
  const data = Array.from({length: 2940}, (_,number) => ({number, sources:[{provider:'fixture',chapterKey:`chapter-${number}`}]}));
  const load = async () => { calls++; await new Promise(resolve => {release=resolve;}); return data; };
  const first=cache.get('same',load,300000), second=cache.get('same',load,300000);
  await new Promise(resolve => setImmediate(resolve)); release();
  assert.deepEqual(await first,await second); assert.equal(calls,1);
  const hit=await cache.get('same',()=>{throw Error('must not rescrape');},300000);
  assert.equal(hit.length,2940); hit.pop();
  assert.equal((await cache.get('same',load,300000)).length,2940);
});
test('failure and partial snapshots are retryable, clear prevents stale reinsertion', async () => {
  const cache=createLookupCache();let calls=0;
  await assert.rejects(cache.get('key',async()=>{throw Error('offline');},300000));
  const load=async()=>{calls++;return {chapters:[],partial:true};};
  await cache.get('key',load,300000);await cache.get('key',load,300000);assert.equal(calls,2);
  let release;const pending=cache.get('old',()=>new Promise(resolve=>{release=resolve;}),300000);
  await new Promise(resolve=>setImmediate(resolve));cache.clear();release(['old']);await pending;
  assert.deepEqual(await cache.get('old',async()=>['new'],300000),['new']);
});
test('cancelled clients do not abort other clients and cache storage is bounded', async () => {
  const cache=createLookupCache({maxBytes:80,maxEntryBytes:70,maxEntries:2});let release;
  const controller=new AbortController();
  const first=cache.get('shared',()=>new Promise(resolve=>{release=resolve;}),300000,controller.signal);
  const second=cache.get('shared',async()=>['wrong'],300000);
  controller.abort();await assert.rejects(first);
  await new Promise(resolve=>setImmediate(resolve));release(['shared']);assert.deepEqual(await second,['shared']);
  await cache.get('other',async()=>['other'],300000);await cache.get('third',async()=>['third'],300000);
  assert.deepEqual(await cache.get('shared',async()=>['reloaded'],300000),['reloaded']);
});

test('progressive discovery serves early chapters, joins one scan, and caches its complete result', async()=>{
 const cache=createLookupCache();let finish,calls=0;
 const load=async publish=>{calls++;publish([{number:1}]);await new Promise(resolve=>{finish=resolve;});return [{number:1},{number:2}];};
 const first=await cache.get('book',load,300000,undefined,true);
 assert.deepEqual(first,{chapters:[{number:1}],partial:true});
 finish();await new Promise(resolve=>setImmediate(resolve));
 assert.deepEqual(await cache.get('book',load,300000,undefined,true),{chapters:[{number:1},{number:2}],partial:false});
 assert.deepEqual(await cache.get('book',load,300000),[{number:1},{number:2}]);assert.equal(calls,1);
});
