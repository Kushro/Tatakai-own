'use strict';
// Synthetic .kai bundle: uses require('fs'), which the native hosts do not provide -> compat = unsupported.
const fs = require('node:fs');

class SyntheticFs {
  async streamAll(opts, onChunk) {
    const cached = fs.existsSync('/tmp/cache') ? 'hit' : 'miss';
    onChunk({ provider: 'fs', results: [{ source: 'FS ' + cached, url: 'https://cdn.fs.example.invalid/a.m3u8', quality: '720p', headers: {}, subtitles: [], sourceType: 'hls' }] });
  }
}

module.exports = SyntheticFs;
