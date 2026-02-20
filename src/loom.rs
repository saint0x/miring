//! Loom compatibility shim.
//!
//! Under `cfg(loom)`, swaps in Loom's model-checked atomics and threads
//! so the ring protocol can be exhaustively verified for ordering bugs.
//!
//! Compile with `RUSTFLAGS='--cfg loom'` to activate.

// Re-export atomic types — Loom intercepts load/store/fence to explore all orderings.
#[cfg(loom)]
pub(crate) mod atomic {
    pub(crate) use ::loom::sync::atomic::{fence, AtomicU32, Ordering};
}
#[cfg(not(loom))]
pub(crate) mod atomic {
    pub(crate) use std::sync::atomic::{fence, AtomicU32, Ordering};
}

// Re-export thread primitives — Loom explores all thread interleavings.
#[cfg(loom)]
pub(crate) mod thread {
    pub(crate) use ::loom::thread::{spawn, yield_now, JoinHandle};
}
#[cfg(not(loom))]
pub(crate) mod thread {
    pub(crate) use std::thread::{spawn, yield_now, JoinHandle};
}

// UnsafeCell: always use std's version. The pending batches queue is internal
// and doesn't need Loom's access tracking — only the ring protocol atomics
// and dispatch threads need Loom verification. Loom's UnsafeCell has a
// different API (closure-based with_mut instead of raw_get), so using std's
// avoids unnecessary refactoring.
pub(crate) mod cell {
    pub(crate) use std::cell::UnsafeCell;
}
