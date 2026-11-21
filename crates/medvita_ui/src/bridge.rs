//! Tokio ↔ GPUI bridge, adapted from Zed's `gpui_tokio` (Apache-2.0, see NOTICE).
//!
//! GPUI runs its own executors; `reqwest` needs tokio. One tokio runtime lives in a GPUI global.
//! [`bridged`] spawns a future on it and returns a future any executor can await (the GPUI
//! foreground task, typically). Dropping that future — e.g. dropping the GPUI `Task` that awaits it
//! when the person clicks "Cancelar" — aborts the tokio task: dropping a `JoinHandle` alone would not.

use std::future::Future;

use gpui_kit::{App, Global};
use tokio::runtime::{Handle, Runtime};
use tokio::task::AbortHandle;

pub struct GlobalTokio {
    runtime: Option<Runtime>,
    handle: Handle,
}

impl Global for GlobalTokio {}

impl Drop for GlobalTokio {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// Puts the runtime in the app. Call once, before any [`handle`].
pub fn init(cx: &mut App, runtime: Runtime) {
    let handle = runtime.handle().clone();
    cx.set_global(GlobalTokio {
        runtime: Some(runtime),
        handle,
    });
}

pub fn handle(cx: &App) -> Handle {
    cx.global::<GlobalTokio>().handle.clone()
}

/// The runtime the app uses: two workers are plenty for an HTTP client.
pub fn new_runtime() -> std::io::Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("medvita-tokio")
        .enable_all()
        .build()
}

struct AbortOnDrop(AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Runs `future` on tokio; the returned future yields its output and aborts it when dropped.
pub fn bridged<F>(
    handle: &Handle,
    future: F,
) -> impl Future<Output = anyhow::Result<F::Output>> + Send + 'static
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let join = handle.spawn(future);
    let guard = AbortOnDrop(join.abort_handle());
    async move {
        let result = join.await;
        drop(guard);
        result.map_err(anyhow::Error::from)
    }
}

/// Runs blocking work (Velopack calls) on tokio's blocking pool. Not abortable once started.
pub fn bridged_blocking<F, R>(
    handle: &Handle,
    work: F,
) -> impl Future<Output = anyhow::Result<R>> + Send + 'static
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let join = handle.spawn_blocking(work);
    async move { join.await.map_err(anyhow::Error::from) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bridged_future_returns_the_output() {
        let runtime = new_runtime().unwrap();
        let handle = runtime.handle().clone();
        let value = futures_lite_block_on(bridged(&handle, async { 21 * 2 }));
        assert_eq!(value.unwrap(), 42);
    }

    #[test]
    fn dropping_the_bridge_aborts_the_tokio_task() {
        struct Flag(std::sync::mpsc::Sender<()>);
        impl Drop for Flag {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let runtime = new_runtime().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let bridge = bridged(runtime.handle(), async move {
            let _flag = Flag(sender);
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        drop(bridge);
        // The tokio task was aborted: its future was dropped long before 60 s. Either the flag
        // reported it (the task had started) or the sender went away unused (it had not).
        let outcome = receiver.recv_timeout(Duration::from_secs(5));
        assert!(
            !matches!(outcome, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
            "the tokio task kept running after the bridge was dropped"
        );
    }

    /// A tiny executor for the tests, so they do not depend on GPUI's.
    fn futures_lite_block_on<F: Future>(future: F) -> F::Output {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(future)
    }
}
