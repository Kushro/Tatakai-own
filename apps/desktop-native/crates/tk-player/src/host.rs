//! Lazily-created shared player. The HWND of the main window only exists once the UI is up, and the
//! child window must be created on the UI thread, so the first screen that needs playback calls
//! [`PlayerHost::get_or_init`].

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::{Player, PlayerError};

pub struct PlayerHost {
    shaders_dir: PathBuf,
    inner: Mutex<Option<Arc<Player>>>,
}

impl PlayerHost {
    pub fn new(shaders_dir: PathBuf) -> Self {
        Self { shaders_dir, inner: Mutex::new(None) }
    }

    pub fn shaders_dir(&self) -> &std::path::Path {
        &self.shaders_dir
    }

    /// The player, creating it (and its child window under `parent_hwnd`) on first use.
    pub fn get_or_init(&self, parent_hwnd: isize) -> Result<Arc<Player>, PlayerError> {
        let mut guard = self.inner.lock().map_err(|_| PlayerError::Unavailable)?;
        if let Some(p) = guard.as_ref() {
            return Ok(p.clone());
        }
        let player = Arc::new(Player::new(parent_hwnd)?);
        player.set_shaders_dir(self.shaders_dir.clone());
        *guard = Some(player.clone());
        Ok(player)
    }

    /// The player if it was already created.
    pub fn current(&self) -> Option<Arc<Player>> {
        self.inner.lock().ok().and_then(|g| g.clone())
    }
}
