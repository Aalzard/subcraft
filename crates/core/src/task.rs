//! Background work that also runs in the browser.

/// Run `f` on a background thread. The web build has no threads, so there it runs right away on
/// the calling thread; callers that collect results through a channel work unchanged.
pub fn spawn(f: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(f);
    #[cfg(target_arch = "wasm32")]
    f();
}
