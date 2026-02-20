use std::num::NonZeroU32;

use crate::loom::atomic;

pub(crate) mod private {
    pub trait Sealed {}
}

/// Unsynchronized load — caller guarantees exclusive access.
///
/// Under Loom, this uses a Relaxed atomic load instead of a raw pointer cast,
/// because Loom must see all memory accesses to verify orderings.
#[inline(always)]
pub(crate) unsafe fn unsync_load(u: *const atomic::AtomicU32) -> u32 {
    #[cfg(not(loom))]
    {
        *u.cast::<u32>()
    }
    #[cfg(loom)]
    {
        (*u).load(atomic::Ordering::Relaxed)
    }
}

#[inline]
pub(crate) const fn cast_ptr<T>(n: &T) -> *const T {
    n
}

#[allow(unconditional_panic, clippy::out_of_bounds_indexing)]
pub(crate) const fn unwrap_u32(t: Option<u32>) -> u32 {
    match t {
        Some(v) => v,
        None => [][1],
    }
}

#[allow(unconditional_panic, clippy::out_of_bounds_indexing)]
pub(crate) const fn unwrap_nonzero(t: Option<NonZeroU32>) -> NonZeroU32 {
    match t {
        Some(v) => v,
        None => [][1],
    }
}
