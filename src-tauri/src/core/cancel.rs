//! Cooperative cancellation primitive.
//!
//! Hand-rolled on top of `tokio::sync::watch` so the core keeps a small
//! dependency surface. Cloning a token shares the same cancellation state,
//! which is what long-running jobs (installs, downloads, device scans) need.

use std::sync::Arc;

use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct CancelToken {
    tx: Arc<watch::Sender<bool>>,
    rx: watch::Receiver<bool>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self {
            tx: Arc::new(tx),
            rx,
        }
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    /// Resolves as soon as the token is cancelled. Safe to call many times and
    /// from many tasks; resolves immediately if cancellation already happened.
    pub async fn cancelled(&self) {
        let mut rx = self.rx.clone();
        if *rx.borrow_and_update() {
            return;
        }
        // `changed()` only errors when every sender is gone; treat that as
        // "will never be cancelled" and park forever rather than busy-loop.
        while rx.changed().await.is_ok() {
            if *rx.borrow_and_update() {
                return;
            }
        }
        std::future::pending::<()>().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_is_observed_by_clones() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled());
        // must resolve, not hang
        b.cancelled().await;
    }

    #[tokio::test]
    async fn cancelled_resolves_when_cancelled_later() {
        let token = CancelToken::new();
        let waiter = token.clone();
        let handle = tokio::spawn(async move {
            waiter.cancelled().await;
            true
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        token.cancel();
        assert!(handle.await.unwrap());
    }
}
