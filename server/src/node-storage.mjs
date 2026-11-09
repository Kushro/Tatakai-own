import { DatabaseSync } from 'node:sqlite';
import { mkdir, readFile, writeFile, unlink } from 'node:fs/promises';
import path from 'node:path';
import { DocumentStore } from './storage.mjs';
import { digest } from './security.mjs';
export async function nodeStorage(root) {
  await mkdir(root, { recursive: true });
  const db = new DatabaseSync(path.join(root, 'server.sqlite'));
  db.exec('PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; CREATE TABLE IF NOT EXISTS server_state(id INTEGER PRIMARY KEY, version INTEGER NOT NULL, data TEXT NOT NULL)');
  const store = new DocumentStore({
    read: async () => db.prepare('SELECT version, data FROM server_state WHERE id=1').get() || null,
    cas: async (version, data) => {
      const result = version < 0 ? db.prepare('INSERT OR IGNORE INTO server_state(id,version,data) VALUES(1,0,?)').run(data)
        : db.prepare('UPDATE server_state SET version=version+1,data=? WHERE id=1 AND version=?').run(data, version);
      return result.changes === 1;
    },
  });
  const directory = path.join(root, 'artifacts'); await mkdir(directory, { recursive: true });
  const filename = async key => path.join(directory, await digest(key));
  const artifacts = { put: async (key, value) => writeFile(await filename(key), value, { mode: 0o600, flag: 'wx' }),
    get: async key => readFile(await filename(key), 'utf8'), remove: async key => { try { await unlink(await filename(key)); } catch (error) { if (error.code !== 'ENOENT') throw error; } } };
  return { store, artifacts, close: () => db.close() };
}
