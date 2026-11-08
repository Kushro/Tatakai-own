import { base64, unbase64 } from './security.mjs';

/** Private, bounded cache. Only metadata is held in the CAS document; bodies
 * live encrypted in the host's artifact store and survive serverless cold starts. */
export function createMediaCache({ store, artifacts, vault }) {
  return {
    async get(key, state) {
      const item = state.mediaCache?.[key];
      if (!state.settings.mediaCacheMb || !item || item.expires <= Date.now()) return null;
      try {
        const payload = await vault.open(await artifacts.get(item.artifact));
        return new Response(unbase64(payload.body), { headers: { ...payload.headers, 'x-tatakai-cache': 'HIT' } });
      } catch { return null; }
    },
    async put(key, data, headers, state) {
      const budget = (state.settings.mediaCacheMb || 0) * 1024 * 1024;
      if (!budget || data.length > Math.min(budget, 512 * 1024)) return;
      const artifact = `cache/${crypto.randomUUID()}`;
      await artifacts.put(artifact, await vault.seal({ headers: Object.fromEntries(headers), body: base64(data) }));
      let discarded = [];
      try {
        await store.mutate(current => {
          discarded = [];
          if (!current?.settings.mediaCacheMb) { discarded.push(artifact); return { state: current }; }
          current.mediaCache ||= {};
          if (current.mediaCache[key]) discarded.push(current.mediaCache[key].artifact);
          current.mediaCache[key] = { artifact, bytes: data.length, expires: Date.now() + (current.settings.mediaCacheSeconds || 300) * 1000 };
          for (const [id, item] of Object.entries(current.mediaCache)) if (item.expires <= Date.now()) { discarded.push(item.artifact); delete current.mediaCache[id]; }
          let total = Object.values(current.mediaCache).reduce((sum, item) => sum + item.bytes, 0);
          const limit = current.settings.mediaCacheMb * 1024 * 1024;
          for (const [id, item] of Object.entries(current.mediaCache)) {
            if (total <= limit && Object.keys(current.mediaCache).length <= 128) break;
            total -= item.bytes; discarded.push(item.artifact); delete current.mediaCache[id];
          }
          return { state: current };
        });
      } catch { discarded = [artifact]; }
      await Promise.all(discarded.map(id => artifacts.remove(id).catch(() => {})));
    },
    async clear() {
      const keys = await store.mutate(state => {
        const keys = Object.values(state.mediaCache || {}).map(item => item.artifact);
        state.mediaCache = {}; state.cache = {}; return { state, value: keys };
      });
      await Promise.all(keys.map(key => artifacts.remove(key).catch(() => {})));
    },
  };
}
