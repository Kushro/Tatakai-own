import http from 'node:http';
import https from 'node:https';
import { lookup } from 'node:dns/promises';
import { Readable } from 'node:stream';
import { createNetwork, publicUrl } from './security.mjs';

/** Validate the DNS address used by the socket, rather than a separate lookup. */
export function nodeNetwork() {
  const fetcher = (url, init = {}) => new Promise((resolve, reject) => {
    const target = new URL(url), transport = target.protocol === 'https:' ? https : http;
    const request = transport.request(target, { method: init.method || 'GET', headers: { 'accept-encoding': 'identity', ...init.headers }, signal: init.signal,
      lookup(hostname, options, callback) {
        lookup(hostname, { all: true }).then(addresses => {
          if (!addresses.length) throw Error('Upstream DNS returned no addresses');
          for (const item of addresses) publicUrl(`http://${item.family === 6 ? '[' + item.address + ']' : item.address}/`);
          callback(null, options.all ? addresses : addresses[0].address, addresses[0].family);
        }).catch(error => callback(error));
      },
    }, response => {
      const headers = new Headers();
      for (let i = 0; i < response.rawHeaders.length; i += 2) headers.append(response.rawHeaders[i], response.rawHeaders[i + 1]);
      const status = response.statusCode;
      resolve(new Response(['HEAD'].includes(init.method) || [204, 205, 304].includes(status) ? null : Readable.toWeb(response), { status, headers }));
      if (init.method === 'HEAD' || [204, 205, 304].includes(status)) response.resume();
    });
    request.on('error', reject); request.end(init.body);
  });
  return createNetwork({ fetch: fetcher });
}
