// All adapters use a single versioned document. CAS retries serialize mutations
// across cold starts and processes without making process memory authoritative.
export class DocumentStore {
  constructor(adapter) { this.adapter = adapter; }
  async read() { const row = await this.adapter.read(); return row ? JSON.parse(row.data) : null; }
  async mutate(change) {
    for (let retry = 0; retry < 12; retry++) {
      const row = await this.adapter.read();
      const state = row ? JSON.parse(row.data) : null;
      const result = await change(state);
      const encoded = JSON.stringify(result.state);
      if (new TextEncoder().encode(encoded).length > 1800000) throw Object.assign(Error('Server configuration storage limit reached'), { status: 413, code: 'configuration_limit' });
      if (await this.adapter.cas(row?.version ?? -1, encoded)) return result.value;
    }
    throw Object.assign(Error('Configuration changed concurrently; retry'), { status: 409, code: 'configuration_conflict' });
  }
}
export function memoryStore() {
  let row = null;
  return new DocumentStore({ read: async () => row && { ...row }, cas: async (version, data) => {
    if ((row?.version ?? -1) !== version) return false;
    row = { version: version + 1, data }; return true;
  } });
}
export function d1Store(db) {
  return new DocumentStore({
    read: () => db.prepare('SELECT version, data FROM server_state WHERE id = 1').first(),
    cas: async (version, data) => {
      const result = version < 0
        ? await db.prepare('INSERT OR IGNORE INTO server_state(id, version, data) VALUES(1, 0, ?)').bind(data).run()
        : await db.prepare('UPDATE server_state SET version = version + 1, data = ? WHERE id = 1 AND version = ?').bind(data, version).run();
      return result.meta.changes === 1;
    },
  });
}
export function postgresStore(sql) {
  return new DocumentStore({
    read: async () => (await sql`SELECT version, data FROM server_state WHERE id = 1`)[0] ?? null,
    cas: async (version, data) => {
      const rows = version < 0
        ? await sql`INSERT INTO server_state(id, version, data) VALUES(1, 0, ${data}) ON CONFLICT DO NOTHING RETURNING id`
        : await sql`UPDATE server_state SET version = version + 1, data = ${data} WHERE id = 1 AND version = ${version} RETURNING id`;
      return rows.length === 1;
    },
  });
}
