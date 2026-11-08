import { mkdir, readFile, writeFile, link, unlink } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import path from 'node:path';

/** Local Node defaults are random per installation and survive restarts. */
export async function resolveNodeSecrets(root, { secret, bootstrapSecret, hasState = false, warn = console.warn } = {}) {
  const filename = path.resolve(root, 'node-secrets.json');
  let saved;
  try { saved = JSON.parse(await readFile(filename, 'utf8')); }
  catch (error) { if (error.code !== 'ENOENT') throw Error('Cannot read node-secrets.json. Restore this file from backup; do not replace it with a new key.'); }
  if (!secret && !saved?.secret && hasState) throw Error('This server already has data encrypted with SERVER_SECRET. Restore its original SERVER_SECRET or node-secrets.json to keep existing credentials working.');
  if (!secret || !bootstrapSecret) {
    if (!saved) {
      await mkdir(root, { recursive: true });
      const generated = { secret: secret || randomBytes(48).toString('hex'), bootstrapSecret: bootstrapSecret || randomBytes(32).toString('hex') };
      const temporary = `${filename}.${randomBytes(12).toString('hex')}.tmp`;
      await writeFile(temporary, JSON.stringify(generated), { mode: 0o600, flag: 'wx' });
      try { await link(temporary, filename); saved = generated; }
      catch (error) { if (error.code !== 'EEXIST') throw error; saved = JSON.parse(await readFile(filename, 'utf8')); }
      finally { await unlink(temporary).catch(() => {}); }
    }
    if (!secret) warn(`[Tatakai Server] WARNING: SERVER_SECRET is not set. Using an automatically generated local secret in ${filename}. Keep this file private and back it up with your data. Set SERVER_SECRET explicitly for managed deployments.`);
    if (!bootstrapSecret) warn(`[Tatakai Server] BOOTSTRAP_SECRET is not set. The initial setup key is stored as bootstrapSecret in ${filename}.`);
  }
  const result = { secret: secret || saved.secret, bootstrapSecret: bootstrapSecret || saved.bootstrapSecret };
  if (typeof result.secret !== 'string' || result.secret.length < 32) throw Error('SERVER_SECRET must contain at least 32 characters');
  if (typeof result.bootstrapSecret !== 'string' || !result.bootstrapSecret) throw Error('Invalid bootstrap secret in node-secrets.json');
  return result;
}
