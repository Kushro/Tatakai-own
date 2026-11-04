//! Run closures on the UI thread from any thread (docs/redesign/09 section 6).
//!
//! Some app services need the window system but live off the UI thread: the extension browser runtime (tk-app
//! `live/browser.rs`) drives hidden web views from a tokio task. tk-app creates the pair with [`channel`], keeps
//! the [`UiThread`] sender and passes the [`UiJobs`] receiver to the UI in `Services::ui_jobs`; the root `App`
//! component drains it. A job runs inside the root component's task, so `dioxus::desktop::window()`,
//! `spawn(...)` and `document::eval` work in it. Keep jobs short; anything long goes into a `spawn`ed future.
//! UI-thread-only state a job creates (web views, `DesktopContext`s) stays in a `thread_local!` of the caller.

use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};

pub type UiJob = Box<dyn FnOnce() + Send + 'static>;

/// Sending half: cheap to clone, `Send + Sync`.
#[derive(Clone)]
pub struct UiThread {
    tx: mpsc::UnboundedSender<UiJob>,
}

/// Receiving half; [`UiJobs::take`] hands it out once (to `App`).
pub struct UiJobs(Mutex<Option<mpsc::UnboundedReceiver<UiJob>>>);

pub fn channel() -> (UiThread, Arc<UiJobs>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (UiThread { tx }, Arc::new(UiJobs(Mutex::new(Some(rx)))))
}

impl UiThread {
    /// Queue `job`; false when the UI is gone.
    pub fn post(&self, job: impl FnOnce() + Send + 'static) -> bool {
        self.tx.send(Box::new(job)).is_ok()
    }

    /// Run `f` on the UI thread and wait for its result; `None` when the UI is gone (or `f` panicked).
    pub async fn run<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
        let (reply, rx) = oneshot::channel();
        if !self.post(move || {
            let _ = reply.send(f());
        }) {
            return None;
        }
        rx.await.ok()
    }
}

impl UiJobs {
    pub fn take(&self) -> Option<mpsc::UnboundedReceiver<UiJob>> {
        self.0.lock().ok().and_then(|mut g| g.take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_run_where_the_receiver_drains_them() {
        let (ui, jobs) = channel();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let mut rx = jobs.take().unwrap();
        assert!(jobs.take().is_none());
        let got = rt.block_on(async move {
            let caller = tokio::spawn(async move { ui.run(|| 40 + 2).await });
            let job = rx.recv().await.unwrap();
            job();
            caller.await.unwrap()
        });
        assert_eq!(got, Some(42));
    }
}
