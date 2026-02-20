//! Submission Queue

use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

use crate::loom::atomic;
use crate::sys;
use crate::util::{private, unsync_load};

use bitflags::bitflags;

pub(crate) struct Inner<E: EntryMarker> {
    pub(crate) head: *const atomic::AtomicU32,
    pub(crate) tail: *const atomic::AtomicU32,
    pub(crate) ring_mask: u32,
    pub(crate) ring_entries: u32,
    pub(crate) flags: *const atomic::AtomicU32,
    pub(crate) dropped: *const atomic::AtomicU32,

    pub(crate) sqes: *mut E,
}

// Safety: Inner only contains raw pointers to heap-allocated ring memory
// owned by IoUring. The ring protocol uses atomic operations for thread safety.
unsafe impl<E: EntryMarker> Send for Inner<E> {}
unsafe impl<E: EntryMarker> Sync for Inner<E> {}

/// An io_uring instance's submission queue. This is used to send I/O requests to the kernel.
pub struct SubmissionQueue<'a, E: EntryMarker = Entry> {
    head: u32,
    tail: u32,
    queue: &'a Inner<E>,
}

/// A submission queue entry (SQE), representing a request for an I/O operation.
///
/// This is implemented for [`Entry`] and [`Entry128`].
pub trait EntryMarker: Clone + Debug + From<Entry> + private::Sealed {
    const BUILD_FLAGS: u32;
}

/// A 64-byte submission queue entry (SQE), representing a request for an I/O operation.
///
/// These can be created via opcodes in [`opcode`](crate::opcode).
#[repr(C)]
pub struct Entry(pub(crate) sys::io_uring_sqe);

/// A 128-byte submission queue entry (SQE), representing a request for an I/O operation.
///
/// These can be created via opcodes in [`opcode`](crate::opcode).
#[repr(C)]
#[derive(Clone)]
pub struct Entry128(pub(crate) Entry, pub(crate) [u8; 64]);

#[test]
fn test_entry_sizes() {
    assert_eq!(std::mem::size_of::<Entry>(), 64);
    assert_eq!(std::mem::size_of::<Entry128>(), 128);
}

bitflags! {
    /// Submission flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Flags: u8 {
        const FIXED_FILE = 1 << sys::IOSQE_FIXED_FILE_BIT;
        const IO_DRAIN = 1 << sys::IOSQE_IO_DRAIN_BIT;
        const IO_LINK = 1 << sys::IOSQE_IO_LINK_BIT;
        const IO_HARDLINK = 1 << sys::IOSQE_IO_HARDLINK_BIT;
        const ASYNC = 1 << sys::IOSQE_ASYNC_BIT;
        const BUFFER_SELECT = 1 << sys::IOSQE_BUFFER_SELECT_BIT;
        const SKIP_SUCCESS = 1 << sys::IOSQE_CQE_SKIP_SUCCESS_BIT;
    }
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
        flags: *const atomic::AtomicU32,
        dropped: *const atomic::AtomicU32,
        sqes: *mut E,
    ) -> Self {
        Self {
            head,
            tail,
            ring_mask,
            ring_entries,
            flags,
            dropped,
            sqes,
        }
    }

    #[inline]
    pub(crate) unsafe fn borrow_shared(&self) -> SubmissionQueue<'_, E> {
        SubmissionQueue {
            head: (*self.head).load(atomic::Ordering::Acquire),
            tail: unsync_load(self.tail),
            queue: self,
        }
    }

    #[inline]
    pub(crate) fn borrow(&mut self) -> SubmissionQueue<'_, E> {
        unsafe { self.borrow_shared() }
    }
}

impl<E: EntryMarker> SubmissionQueue<'_, E> {
    /// Synchronize this type with the real submission queue.
    #[inline]
    pub fn sync(&mut self) {
        unsafe {
            (*self.queue.tail).store(self.tail, atomic::Ordering::Release);
            self.head = (*self.queue.head).load(atomic::Ordering::Acquire);
        }
    }

    /// When [`is_setup_sqpoll`](crate::Parameters::is_setup_sqpoll) is set, whether the kernel
    /// thread has gone to sleep and requires a system call to wake it up.
    #[inline]
    pub fn need_wakeup(&self) -> bool {
        atomic::fence(atomic::Ordering::SeqCst);
        unsafe {
            (*self.queue.flags).load(atomic::Ordering::Relaxed) & sys::IORING_SQ_NEED_WAKEUP != 0
        }
    }

    /// The effect of [`Self::need_wakeup`], after synchronization work performed by the caller.
    #[inline]
    pub fn need_wakeup_after_intermittent_seqcst(&self) -> bool {
        unsafe {
            (*self.queue.flags).load(atomic::Ordering::Relaxed) & sys::IORING_SQ_NEED_WAKEUP != 0
        }
    }

    /// The number of invalid submission queue entries that have been encountered.
    pub fn dropped(&self) -> u32 {
        unsafe { (*self.queue.dropped).load(atomic::Ordering::Acquire) }
    }

    /// Returns `true` if the completion queue ring is overflown.
    pub fn cq_overflow(&self) -> bool {
        unsafe {
            (*self.queue.flags).load(atomic::Ordering::Acquire) & sys::IORING_SQ_CQ_OVERFLOW != 0
        }
    }

    /// Returns `true` if completions are pending that should be processed.
    pub fn taskrun(&self) -> bool {
        unsafe { (*self.queue.flags).load(atomic::Ordering::Acquire) & sys::IORING_SQ_TASKRUN != 0 }
    }

    /// Get the total number of entries in the submission queue ring buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.queue.ring_entries as usize
    }

    /// Get the number of submission queue events in the ring buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.tail.wrapping_sub(self.head) as usize
    }

    /// Returns `true` if the submission queue ring buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the submission queue ring buffer has reached capacity.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Attempts to push an entry into the queue.
    /// If the queue is full, an error is returned.
    ///
    /// # Safety
    ///
    /// Developers must ensure that parameters of the entry (such as buffer) are valid and will
    /// be valid for the entire duration of the operation, otherwise it may cause memory problems.
    #[inline]
    pub unsafe fn push(&mut self, entry: &E) -> Result<(), PushError> {
        if !self.is_full() {
            self.push_unchecked(entry);
            Ok(())
        } else {
            Err(PushError)
        }
    }

    /// Attempts to push several entries into the queue.
    ///
    /// # Safety
    ///
    /// Developers must ensure that parameters of all the entries (such as buffer) are valid and
    /// will be valid for the entire duration of the operation.
    #[inline]
    pub unsafe fn push_multiple(&mut self, entries: &[E]) -> Result<(), PushError> {
        if self.capacity() - self.len() < entries.len() {
            return Err(PushError);
        }

        for entry in entries {
            self.push_unchecked(entry);
        }

        Ok(())
    }

    #[inline]
    unsafe fn push_unchecked(&mut self, entry: &E) {
        *self
            .queue
            .sqes
            .add((self.tail & self.queue.ring_mask) as usize) = entry.clone();
        self.tail = self.tail.wrapping_add(1);
    }
}

impl<E: EntryMarker> Drop for SubmissionQueue<'_, E> {
    #[inline]
    fn drop(&mut self) {
        unsafe { &*self.queue.tail }.store(self.tail, atomic::Ordering::Release);
    }
}

impl Entry {
    /// Set the submission event's [flags](Flags).
    #[inline]
    pub fn flags(mut self, flags: Flags) -> Entry {
        self.0.flags |= flags.bits();
        self
    }

    /// Clear the submission event's [flags](Flags).
    #[inline]
    pub fn clear_flags(mut self) -> Entry {
        self.0.flags = 0;
        self
    }

    /// Set the user data.
    #[inline]
    pub fn user_data(mut self, user_data: u64) -> Entry {
        self.0.user_data = user_data;
        self
    }

    /// Set the user_data without consuming the entry.
    #[inline]
    pub fn set_user_data(&mut self, user_data: u64) {
        self.0.user_data = user_data;
    }

    /// Get the previously application-supplied user data.
    #[inline]
    pub fn get_user_data(&self) -> u64 {
        self.0.user_data
    }

    /// Get the opcode associated with this entry.
    #[inline]
    pub fn get_opcode(&self) -> u32 {
        self.0.opcode.into()
    }

    /// Set the personality of this event.
    pub fn personality(mut self, personality: u16) -> Entry {
        self.0.personality = personality;
        self
    }
}

impl private::Sealed for Entry {}

impl EntryMarker for Entry {
    const BUILD_FLAGS: u32 = 0;
}

impl Clone for Entry {
    #[inline(always)]
    fn clone(&self) -> Entry {
        Entry(unsafe { core::ptr::read(&self.0) })
    }
}

impl Debug for Entry {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("op_code", &self.0.opcode)
            .field("flags", &self.0.flags)
            .field("user_data", &self.0.user_data)
            .finish()
    }
}

impl Entry128 {
    /// Set the submission event's [flags](Flags).
    #[inline]
    pub fn flags(mut self, flags: Flags) -> Entry128 {
        self.0 .0.flags |= flags.bits();
        self
    }

    /// Clear the submission event's [flags](Flags).
    #[inline]
    pub fn clear_flags(mut self) -> Entry128 {
        self.0 .0.flags = 0;
        self
    }

    /// Set the user data.
    #[inline]
    pub fn user_data(mut self, user_data: u64) -> Entry128 {
        self.0 .0.user_data = user_data;
        self
    }

    /// Set the user data without consuming the entry.
    #[inline]
    pub fn set_user_data(&mut self, user_data: u64) {
        self.0 .0.user_data = user_data;
    }

    /// Set the personality of this event.
    #[inline]
    pub fn personality(mut self, personality: u16) -> Entry128 {
        self.0 .0.personality = personality;
        self
    }

    /// Get the opcode associated with this entry.
    #[inline]
    pub fn get_opcode(&self) -> u32 {
        self.0 .0.opcode.into()
    }
}

impl private::Sealed for Entry128 {}

impl EntryMarker for Entry128 {
    const BUILD_FLAGS: u32 = sys::IORING_SETUP_SQE128;
}

impl From<Entry> for Entry128 {
    fn from(entry: Entry) -> Entry128 {
        Entry128(entry, [0u8; 64])
    }
}

impl Debug for Entry128 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry128")
            .field("op_code", &self.0 .0.opcode)
            .field("flags", &self.0 .0.flags)
            .field("user_data", &self.0 .0.user_data)
            .finish()
    }
}

/// An error pushing to the submission queue due to it being full.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PushError;

impl Display for PushError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("submission queue is full")
    }
}

impl Error for PushError {}

impl<E: EntryMarker> Debug for SubmissionQueue<'_, E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_list();
        let mut pos = self.head;
        while pos != self.tail {
            let entry: &E = unsafe { &*self.queue.sqes.add((pos & self.queue.ring_mask) as usize) };
            d.entry(&entry);
            pos = pos.wrapping_add(1);
        }
        d.finish()
    }
}
