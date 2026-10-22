//! Background work.

/// Run `f` on a background thread.
pub fn spawn(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f);
}
