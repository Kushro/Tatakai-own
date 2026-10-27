#!/usr/bin/env node
// Zips each fixture source directory into dist/<name>.kai (deterministic: sorted entries, fixed timestamps, deflate).
// Only manifest.json and bundle.js are packed (expected*.json are test oracles, not part of the package).
//   node contracts/fixtures/extensions/build.mjs           write dist/*.kai
//   node contracts/fixtures/extensions/build.mjs --check   exit 1 if dist/*.kai are stale
import { readFileSync, writeFileSync, mkdirSync, existsSync, readdirSync, statSync } from 'node:fs';
import { deflateRawSync, crc32 } from 'node:zlib';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const distDir = join(here, 'dist');
const PACKED = ['manifest.json', 'bundle.js'];
const DOS_TIME = 0; // 00:00:00
const DOS_DATE = (0 << 9) | (1 << 5) | 1; // 1980-01-01

export function fixtureNames() {
  return readdirSync(here).filter((n) => statSync(join(here, n)).isDirectory() && n !== 'dist').sort();
}

export function buildKai(name) {
  // normalize CRLF (Windows autocrlf checkouts) so the archive bytes are identical everywhere
  const entries = PACKED.map((file) => ({
    file,
    data: Buffer.from(readFileSync(join(here, name, file), 'utf8').replaceAll('\r\n', '\n'), 'utf8'),
  }));
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const { file, data } of entries) {
    const nameBuf = Buffer.from(file, 'utf8');
    const comp = deflateRawSync(data, { level: 9 });
    const crc = crc32(data) >>> 0;
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4); // version needed
    local.writeUInt16LE(0x0800, 6); // UTF-8 names
    local.writeUInt16LE(8, 8); // deflate
    local.writeUInt16LE(DOS_TIME, 10);
    local.writeUInt16LE(DOS_DATE, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(comp.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    local.writeUInt16LE(0, 28);
    locals.push(local, nameBuf, comp);

    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4); // version made by
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(8, 10);
    central.writeUInt16LE(DOS_TIME, 12);
    central.writeUInt16LE(DOS_DATE, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(comp.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt32LE(offset, 42);
    centrals.push(central, nameBuf);
    offset += local.length + nameBuf.length + comp.length;
  }
  const centralBuf = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralBuf.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, centralBuf, end]);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const check = process.argv.includes('--check');
  let stale = false;
  if (!check) mkdirSync(distDir, { recursive: true });
  for (const name of fixtureNames()) {
    const out = join(distDir, `${name}.kai`);
    const buf = buildKai(name);
    if (check) {
      if (!existsSync(out) || !readFileSync(out).equals(buf)) {
        console.error(`stale: ${out}`);
        stale = true;
      }
    } else {
      writeFileSync(out, buf);
      console.log(`wrote ${out} (${buf.length} bytes)`);
    }
  }
  if (stale) process.exit(1);
}
