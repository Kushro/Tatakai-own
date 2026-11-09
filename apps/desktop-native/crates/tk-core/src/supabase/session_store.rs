//! Persisted Supabase session (wave A, worker **f**): the KNOWN-ISSUES item "session not persisted". The session
//! (`Session` as JSON) lives in `crate::secrets::Secrets` under `keys::SUPABASE_SESSION`; at start-up tk-app
//! loads it, calls `Supabase::set_session`, and refreshes when `expires_at` is near (`Supabase::refresh`).
//! Contract stubs below: load returns `None`, save / clear do nothing (worker f implements).

use crate::error::SecretError;
use crate::secrets::{Secrets, keys};
use crate::supabase::Session;

pub fn load(secrets: &Secrets) -> Result<Option<Session>, SecretError> {
    let raw = secrets.get(keys::SUPABASE_SESSION)?;
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

pub fn save(secrets: &Secrets, session: &Session) -> Result<(), SecretError> {
    secrets.set(keys::SUPABASE_SESSION, &serde_json::to_string(session)?)
}

pub fn clear(secrets: &Secrets) -> Result<(), SecretError> {
    secrets.delete(keys::SUPABASE_SESSION)
}
