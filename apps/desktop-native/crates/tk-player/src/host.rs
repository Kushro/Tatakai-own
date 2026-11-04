//! Lazily-created shared player. The native window only exists once the UI is up, and the video surface must be
//! created on the UI thread, so the first screen that needs playback calls [`PlayerHost::get_or_init`].
//!
//! The engine (docs/redesign/09 section 3) is chosen with [`PlayerHost::set_engine`] before `get_or_init`; a live
//! player built with another engine is dropped and recreated on the next `get_or_init` (screens that still hold
//! the old `Arc<Player>` keep it until they unmount).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::embed::EmbedTarget;
use crate::engine::{EngineConfig, EngineMode};
use crate::{Player, PlayerError};

pub struct PlayerHost {
    shaders_dir: PathBuf,
    mode: Mutex<EngineMode>,
    inner: Mutex<Option<(EngineConfig, Arc<Player>)>>,
}

impl PlayerHost {
    pub fn new(shaders_dir: PathBuf) -> Self {
        Self { shaders_dir, mode: Mutex::new(EngineMode::Embedded), inner: Mutex::new(None) }
    }

    pub fn shaders_dir(&self) -> &std::path::Path {
        &self.shaders_dir
    }

    /// Engine for the next [`PlayerHost::get_or_init`]. Cheap; does not touch the live player by itself.
    pub fn set_engine(&self, mode: EngineMode) {
        if let Ok(mut m) = self.mode.lock() {
            *m = mode;
        }
    }

    /// The configuration the next `get_or_init` builds with.
    pub fn engine(&self) -> EngineConfig {
        let mode = self.mode.lock().map(|m| m.clone()).unwrap_or_default();
        EngineConfig { mode, shaders_dir: self.shaders_dir.clone() }
    }

    /// The player, creating it (and its video surface in `target`) on first use or when the engine changed.
    /// Call on the UI thread.
    pub fn get_or_init(&self, target: &EmbedTarget) -> Result<Arc<Player>, PlayerError> {
        let cfg = self.engine();
        let mut guard = self.inner.lock().map_err(|_| PlayerError::Unavailable)?;
        if let Some((built, p)) = guard.as_ref()
            && *built == cfg
        {
            return Ok(p.clone());
        }
        // Drop the old one first: two libmpv instances drawing into one surface is never wanted.
        *guard = None;
        let player = Arc::new(Player::new(target, &cfg)?);
        player.set_shaders_dir(self.shaders_dir.clone());
        *guard = Some((cfg, player.clone()));
        Ok(player)
    }

    /// The player if it was already created.
    pub fn current(&self) -> Option<Arc<Player>> {
        self.inner.lock().ok().and_then(|g| g.as_ref().map(|(_, p)| p.clone()))
    }

    /// Forget the live player (External mode, or to free the GPU). Holders of an `Arc` keep theirs until dropped.
    pub fn release(&self) {
        if let Ok(mut g) = self.inner.lock() {
            *g = None;
        }
    }
}
