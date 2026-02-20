//! Completion Queue

use std::fmt::{self, Debug};
use std::mem::MaybeUninit;

use crate::loom::atomic;
use crate::sys;
use crate::util::{private, unsync_load};

pub(crate) struct Inner<E: EntryMarker> {
    pub(crate) head: *const atomic::AtomicU32,
    pub(crate) tail: *const atomic::AtomicU32,
    pub(crate) ring_mask: u32,
    pub(crate) ring_entries: u32,

    pub(crate) overflow: *const atomic::AtomicU32,

    pub(crate) cqes: *mut E,

    #[allow(dead_code)]
    pub(crate) flags: *const atomic::AtomicU32,
}

// Safety: Inner only contains raw pointers to heap-allocated ring memory
// owned by IoUring. The ring protocol uses atomic operations for thread safety.
unsafe impl<E: EntryMarker> Send for Inner<E> {}
unsafe impl<E: EntryMarker> Sync for Inner<E> {}

/// An io_uring instance's completion queue. This stores all the I/O operations that have completed.
pub struct CompletionQueue<'a, E: EntryMarker = Entry> {
    head: u32,
    tail: u32,
    queue: &'a Inner<E>,
}

/// A completion queue entry (CQE), representing a complete I/O operation.
///
/// This is implemented for [`Entry`] and [`Entry32`].
pub trait EntryMarker: Clone + Debug + Into<Entry> + private::Sealed {
    const BUILD_FLAGS: u32;
}

/// A 16-byte completion queue entry (CQE), representing a complete I/O operation.
#[repr(C)]
pub struct Entry(pub(crate) sys::io_uring_cqe);

/// A 32-byte completion queue entry (CQE), representing a complete I/O operation.
#[repr(C)]
#[derive(Clone)]
pub struct Entry32(pub(crate) Entry, pub(crate) [u64; 2]);

#[test]
fn test_entry_sizes() {
    assert_eq!(std::mem::size_of::<Entry>(), 16);
    assert_eq!(std::mem::size_of::<Entry32>(), 32);
}

impl<E: EntryMarker> Inner<E> {
    /// Create an Inner from raw pointers into the ring allocation.
    ///
    /// # Safety
    ///
    /// All pointers must be valid and point to properly initialized memory
    /// that outlives this Inner.
    pub(crate) unsafe fn new(
        head: *const atomic::AtomicU32,
        tail: *const atomic::AtomicU32,
        ring_mask: u32,
        ring_entries: u32,
        overflow: *const atomic::AtomicU32,
        cqes: *mut E,
        flags: *const atomic::AtomicU32,
    ) -> Self {
        Self {
            head,
            tail,
            ring_mask,
            ring_entries,
            overflow,
            cqes,
            flags,
        }
    }

    #[inline]
    pub(crate) unsafe fn borrow_shared(&self) -> CompletionQueue<'_, E> {
        CompletionQueue {
            head: unsync_load(self.head),
            tail: (*self.tail).load(atomic::Ordering::Acquire),
            queue: self,
        }
    }

    #[inline]
    pub(crate) fn borrow(&mut self) -> CompletionQueue<'_, E> {
        unsafe { self.borrow_shared() }
    }
}

impl<E: EntryMarker> CompletionQueue<'_, E> {
    /// Synchronize this type with the real completion queue.
    #[inline]
    pub fn sync(&mut self) {
        unsafe {
            (*self.queue.head).store(self.head, atomic::Ordering::Release);
            self.tail = (*self.queue.tail).load(atomic::Ordering::Acquire);
        }
    }

    /// If queue is full and [`is_feature_nodrop`](crate::Parameters::is_feature_nodrop) is not set,
    /// new events may be dropped. This records the number of dropped events.
    pub fn overflow(&self) -> u32 {
        unsafe { (*self.queue.overflow).load(atomic::Ordering::Acquire) }
    }

    /// Whether eventfd notifications are disabled.
    pub fn eventfd_disabled(&self) -> bool {
        unsafe {
            (*self.queue.flags).load(atomic::Ordering::Acquire) & sys::IORING_CQ_EVENTFD_DISABLED
                != 0
        }
    }

    /// Get the total number of entries in the completion queue ring buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.queue.ring_entries as usize
    }

    /// Returns `true` if there are no completion queue events to be processed.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the completion queue is at maximum capacity.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    #[inline]
    pub fn fill<'a>(&mut self, entries: &'a mut [MaybeUninit<E>]) -> &'a mut [E] {
        let len = std::cmp::min(self.len(), entries.len());

        for entry in &mut entries[..len] {
            entry.write(unsafe { self.pop() });
        }

        unsafe { std::slice::from_raw_parts_mut(entries as *mut _ as *mut E, len) }
    }

    #[inline]
    unsafe fn pop(&mut self) -> E {
        let entry = &*self
            .queue
            .cqes
            .add((self.head & self.queue.ring_mask) as usize);
        self.head = self.head.wrapping_add(1);
        entry.clone()
    }
}

impl<E: EntryMarker> Drop for CompletionQueue<'_, E> {
    #[inline]
    fn drop(&mut self) {
        unsafe { &*self.queue.head }.store(self.head, atomic::Ordering::Release);
    }
}

impl<E: EntryMarker> Iterator for CompletionQueue<'_, E> {
    type Item = E;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.head != self.tail {
            Some(unsafe { self.pop() })
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len(), Some(self.len()))
    }
}

impl<E: EntryMarker> ExactSizeIterator for CompletionQueue<'_, E> {
    #[inline]
    fn len(&self) -> usize {
        self.tail.wrapping_sub(self.head) as usize
    }
}

impl Entry {
    /// The operation-specific result code.
    #[inline]
    pub fn result(&self) -> i32 {
        self.0.res
    }

    /// The user data of the request.
    #[inline]
    pub fn user_data(&self) -> u64 {
        self.0.user_data
    }

    /// Metadata related to the operation.
    #[inline]
    pub fn flags(&self) -> u32 {
        self.0.flags
    }
}

impl private::Sealed for Entry {}

impl EntryMarker for Entry {
    const BUILD_FLAGS: u32 = 0;
}

impl Clone for Entry {
    fn clone(&self) -> Entry {
        Entry(unsafe { core::ptr::read(&self.0) })
    }
}

impl Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("result", &self.result())
            .field("user_data", &self.user_data())
            .field("flags", &self.flags())
            .finish()
    }
}

impl Entry32 {
    /// The operation-specific result code.
    #[inline]
    pub fn result(&self) -> i32 {
        self.0 .0.res
    }

    /// The user data of the request.
    #[inline]
    pub fn user_data(&self) -> u64 {
        self.0 .0.user_data
    }

    /// Metadata related to the operation.
    #[inline]
    pub fn flags(&self) -> u32 {
        self.0 .0.flags
    }

    /// Additional data available in 32-byte CQEs.
    #[inline]
    pub fn big_cqe(&self) -> &[u64; 2] {
        &self.1
    }
}

impl private::Sealed for Entry32 {}

impl EntryMarker for Entry32 {
    const BUILD_FLAGS: u32 = sys::IORING_SETUP_CQE32;
}

impl From<Entry32> for Entry {
    fn from(entry32: Entry32) -> Self {
        entry32.0
    }
}

impl Debug for Entry32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry32")
            .field("result", &self.result())
            .field("user_data", &self.user_data())
            .field("flags", &self.flags())
            .field("big_cqe", &self.big_cqe())
            .finish()
    }
}

/// Return whether the buffer will be reused by future CQE completions.
pub fn buffer_more(flags: u32) -> bool {
    flags & sys::IORING_CQE_F_BUF_MORE != 0
}

/// Return which dynamic buffer was used by this operation.
pub fn buffer_select(flags: u32) -> Option<u16> {
    if flags & sys::IORING_CQE_F_BUFFER != 0 {
        let id = flags >> sys::IORING_CQE_BUFFER_SHIFT;
        Some(id as u16)
    } else {
        None
    }
}

/// Return whether further completion events will be submitted for this same operation.
pub fn more(flags: u32) -> bool {
    flags & sys::IORING_CQE_F_MORE != 0
}

/// Return whether socket has more data ready to read.
pub fn sock_nonempty(flags: u32) -> bool {
    flags & sys::IORING_CQE_F_SOCK_NONEMPTY != 0
}

/// Returns whether this completion event is a notification.
pub fn notif(flags: u32) -> bool {
    flags & sys::IORING_CQE_F_NOTIF != 0
}
