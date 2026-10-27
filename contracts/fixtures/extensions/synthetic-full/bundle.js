'use strict';
// Synthetic .kai bundle used by the host conformance tests (Kotlin + Rust). No real network access is made
// except through the host-provided, permission-gated fetch (see probeNetwork).
const crypto = require('crypto');

const PROVIDERS = [
  { name: 'alpha', kind: 'stream', priority: 0 },
  { name: 'beta', kind: 'stream', priority: 1 },
];

function token(opts) {
  return crypto.createHash('sha1').update(opts.anilistId + ':' + opts.episode).digest('hex').slice(0, 8);
}

class SyntheticFull {
  listProviders() {
    return PROVIDERS.map((p) => ({ name: p.name, kind: p.kind }));
  }

  async runProvider(name, opts) {
    const provider = PROVIDERS.find((p) => p.name === name);
    if (!provider) throw new Error('Unknown provider: ' + name);
    const t = token(opts);
    const base = 'https://cdn.' + name + '.example.invalid';
    let results;
    if (name === 'alpha') {
      results = [{
        source: 'Alpha 1080p',
        url: base + '/hls/' + opts.anilistId + '/' + opts.episode + '/master.m3u8?t=' + t,
        quality: '1080p',
        headers: { Referer: 'https://alpha.example.invalid/', 'User-Agent': 'TatakaiFixture/1.0' },
        refererCandidates: ['https://alpha.example.invalid/', 'https://alpha-mirror.example.invalid/'],
        subtitles: [{ url: base + '/subs/en.vtt', label: 'English', language: 'en', default: true }],
        audioLanguage: 'ja',
        sourceType: 'hls',
        server: 'alpha-main',
        providerPriority: provider.priority,
      }];
    } else {
      results = [
        {
          source: 'Beta 720p',
          url: base + '/mp4/' + opts.anilistId + '/' + opts.episode + '_720.mp4?t=' + t,
          quality: '720p',
          headers: { Referer: 'https://beta.example.invalid/' },
          subtitles: [],
          audioLanguage: 'en',
          sourceType: 'mp4',
          server: 'beta-main',
          providerPriority: provider.priority,
        },
        {
          source: 'Beta 480p',
          url: base + '/mp4/' + opts.anilistId + '/' + opts.episode + '_480.mp4?t=' + t,
          quality: '480p',
          headers: { Referer: 'https://beta.example.invalid/' },
          subtitles: [],
          audioLanguage: 'en',
          sourceType: 'mp4',
          server: 'beta-main',
          providerPriority: provider.priority,
        },
      ];
    }
    return {
      provider: name,
      kind: provider.kind,
      results: results,
      diagnostic: { provider: name, status: 'ok', resultCount: results.length, durationMs: 0, attempts: 1 },
    };
  }

  async streamAll(opts, onChunk) {
    for (const p of PROVIDERS) onChunk(await this.runProvider(p.name, opts));
  }

  async sourcesAll(opts, onChunk) {
    return this.streamAll(opts, onChunk);
  }

  async customHome(sourceId) {
    return {
      sections: [{ title: 'Featured (' + sourceId + ')', items: [{ id: 'x1', title: 'Synthetic One', image: 'https://img.alpha.example.invalid/1.jpg' }] }],
    };
  }

  // The conformance test calls this to prove fetch is permission gated: the domain below is NOT in the manifest.
  async probeNetwork() {
    try {
      await fetch('https://blocked.example.invalid/x');
      return 'allowed';
    } catch (e) {
      return String(e && e.message);
    }
  }
}

module.exports = SyntheticFull;
