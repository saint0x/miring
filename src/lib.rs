//! Pure-Rust io_uring emulator — Miri-compatible, API-compatible with io-uring 0.7.
//!
//! Instead of calling into the Linux kernel, this crate emulates the io_uring
//! ring buffer protocol entirely in userspace using heap allocations and atomic
//! operations. This allows code that depends on `io-uring` to be tested under
//! Miri for undefined behavior detection.
//!
//! # Usage
//!
//! ```rust,ignore
//! #[cfg(miri)]
//! use miring as io_uring;
//! ```

mod loom;
mod util;
pub mod cqueue;
pub mod opcode;
pub mod register;
pub mod squeue;
mod submit;
mod sys;
pub mod types;

use crate::loom::atomic::AtomicU32;
use crate::loom::cell::UnsafeCell;
use std::marker::PhantomData;
use std::mem;
use std::ptr;
use std::{cmp, io};

pub use cqueue::CompletionQueue;
pub use register::Probe;
pub use squeue::SubmissionQueue;
pub use submit::{EnterFlags, Submitter};

// ---------------------------------------------------------------------------
// Ring state — replaces mmap'd shared memory
// ---------------------------------------------------------------------------

/// All atomic metadata for the SQ and CQ rings.
///
/// Heap-allocated via `Box`, with raw pointers into it held by `Inner` structs.
/// This replaces the mmap'd shared memory region in real io_uring.
struct RingState {
    // Submission queue metadata
    sq_head: AtomicU32,
    sq_tail: AtomicU32,
    sq_flags: AtomicU32,
    sq_dropped: AtomicU32,

    // Completion queue metadata
    cq_head: AtomicU32,
    cq_tail: AtomicU32,
    cq_overflow: AtomicU32,
    cq_flags: AtomicU32,

    // Deferred completion: SQEs are captured at submit() time and dispatched
    // (buffer touching) on a background thread. Joining happens when completions
    // are reaped. This creates a temporal gap so Miri can detect:
    //   - Use-after-free: buffer freed between submit and completion
    //   - Data races: buffer modified while "kernel" is concurrently accessing it
    //   - Aliasing violations: mutable references created while pointers are in flight
    // With MIRIFLAGS="-Zmiri-preemption-rate=0.1", Miri explores different
    // thread schedules across runs for broader interleaving coverage.
    pending: UnsafeCell<submit::PendingBatches>,
}

/// Owns all heap-allocated ring memory via raw pointers.
///
/// Using raw pointers (from `Box::into_raw`) instead of `Box` avoids
/// Stacked Borrows violations under Miri: moving a `Box` into a struct
/// creates a Unique retag that invalidates any pointers derived before
/// the move. With raw pointers, provenance is preserved.
struct RingMemory<S, C> {
    state: *mut RingState,
    sqes: *mut [S],
    cqes: *mut [C],
}

impl<S, C> Drop for RingMemory<S, C> {
    fn drop(&mut self) {
        unsafe {
            // Join any pending dispatch threads before freeing ring memory.
            // If a thread panicked (e.g., accessing freed memory under Miri),
            // we ignore the panic here — Miri will have already reported the UB.
            let batches =
                &mut *UnsafeCell::raw_get(ptr::addr_of!((*self.state).pending));
            for handle in batches.drain(..) {
                let _ = handle.join();
            }
            drop(Box::from_raw(self.state));
            drop(Box::from_raw(self.sqes));
            drop(Box::from_raw(self.cqes));
        }
    }
}

// Safety: The raw pointers point to heap allocations that are exclusively
// owned by this RingMemory. Access is synchronized through AtomicU32 operations
// in the ring protocol.
unsafe impl<S: Send, C: Send> Send for RingMemory<S, C> {}
unsafe impl<S: Sync, C: Sync> Sync for RingMemory<S, C> {}

// ---------------------------------------------------------------------------
// IoUring
// ---------------------------------------------------------------------------

/// IoUring instance.
///
/// - `S`: The ring's submission queue entry (SQE) type, either [`squeue::Entry`] or
///   [`squeue::Entry128`];
/// - `C`: The ring's completion queue entry (CQE) type, either [`cqueue::Entry`] or
///   [`cqueue::Entry32`].
pub struct IoUring<S = squeue::Entry, C = cqueue::Entry>
where
    S: squeue::EntryMarker,
    C: cqueue::EntryMarker,
{
    sq: squeue::Inner<S>,
    cq: cqueue::Inner<C>,
    params: Parameters,
    // Must be dropped AFTER sq/cq (Rust drops fields in declaration order).
    // Never read directly — exists solely to own the heap allocations.
    #[allow(dead_code)]
    memory: RingMemory<S, C>,
}

/// IoUring build params.
#[derive(Clone)]
pub struct Builder<S = squeue::Entry, C = cqueue::Entry>
where
    S: squeue::EntryMarker,
    C: cqueue::EntryMarker,
{
    params: sys::io_uring_params,
    phantom: PhantomData<(S, C)>,
}

impl<S: squeue::EntryMarker, C: cqueue::EntryMarker> Default for Builder<S, C> {
    fn default() -> Self {
        Builder {
            params: sys::io_uring_params {
                flags: S::BUILD_FLAGS | C::BUILD_FLAGS,
                ..Default::default()
            },
            phantom: PhantomData,
        }
    }
}

/// The parameters that were used to construct an [`IoUring`].
///
/// This type is a transparent wrapper over the system structure `io_uring_params`.
#[derive(Clone)]
#[repr(transparent)]
pub struct Parameters(sys::io_uring_params);

unsafe impl<S: squeue::EntryMarker, C: cqueue::EntryMarker> Send for IoUring<S, C> {}
unsafe impl<S: squeue::EntryMarker, C: cqueue::EntryMarker> Sync for IoUring<S, C> {}

// ---------------------------------------------------------------------------
// IoUring — construction
// ---------------------------------------------------------------------------

impl IoUring<squeue::Entry, cqueue::Entry> {
    /// Create a new `IoUring` instance with default configuration parameters. See [`Builder`] to
    /// customize it further.
    ///
    /// The `entries` sets the size of queue,
    /// and its value should be the power of two.
    pub fn new(entries: u32) -> io::Result<Self> {
        Self::builder().build(entries)
    }
}

// ---------------------------------------------------------------------------
// IoUring — main API
// ---------------------------------------------------------------------------

impl<S: squeue::EntryMarker, C: cqueue::EntryMarker> IoUring<S, C> {
    /// Create a [`Builder`] for an `IoUring` instance.
    ///
    /// This allows for further customization than [`new`](Self::new).
    /// Unlike [`IoUring::new`], this function is available for any combination of
    /// submission queue entry (SQE) and completion queue entry (CQE) types.
    #[must_use]
    pub fn builder() -> Builder<S, C> {
        Builder::default()
    }

    /// Get the submitter of this io_uring instance, which can be used to submit submission queue
    /// events for execution and to register files or buffers with it.
    #[inline]
    pub fn submitter(&self) -> Submitter<'_> {
        Submitter::new(
            &self.params,
            self.sq.head,
            self.sq.tail,
            self.sq.flags,
            self.sq.ring_mask,
            self.sq.sqes as *const u8,
            mem::size_of::<S>(),
            self.cq.head,
            self.cq.tail,
            self.cq.ring_mask,
            self.cq.ring_entries,
            self.cq.overflow,
            self.cq.cqes as *mut u8,
            mem::size_of::<C>(),
            unsafe { ptr::addr_of!((*self.memory.state).pending) },
        )
    }

    /// Get the parameters that were used to construct this instance.
    #[inline]
    pub fn params(&self) -> &Parameters {
        &self.params
    }

    /// Initiate asynchronous I/O. See [`Submitter::submit`] for more details.
    #[inline]
    pub fn submit(&self) -> io::Result<usize> {
        self.submitter().submit()
    }

    /// Initiate and/or complete asynchronous I/O. See [`Submitter::submit_and_wait`] for more
    /// details.
    #[inline]
    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        self.submitter().submit_and_wait(want)
    }

    /// Get the submitter, submission queue and completion queue of the io_uring instance. This can
    /// be used to operate on the different parts of the io_uring instance independently.
    ///
    /// If you use this method to obtain `sq` and `cq`,
    /// please note that you need to `drop` or `sync` the queue before and after submit,
    /// otherwise the queue will not be updated.
    #[inline]
    pub fn split(
        &mut self,
    ) -> (
        Submitter<'_>,
        SubmissionQueue<'_, S>,
        CompletionQueue<'_, C>,
    ) {
        // Flush any pending dispatch threads before handing out the CQ,
        // so that completions from previous submits are visible.
        self.flush_pending();

        let pending_ptr = unsafe { ptr::addr_of!((*self.memory.state).pending) };
        let submit = Submitter::new(
            &self.params,
            self.sq.head,
            self.sq.tail,
            self.sq.flags,
            self.sq.ring_mask,
            self.sq.sqes as *const u8,
            mem::size_of::<S>(),
            self.cq.head,
            self.cq.tail,
            self.cq.ring_mask,
            self.cq.ring_entries,
            self.cq.overflow,
            self.cq.cqes as *mut u8,
            mem::size_of::<C>(),
            pending_ptr,
        );
        (submit, self.sq.borrow(), self.cq.borrow())
    }

    /// Get the submission queue of the io_uring instance. This is used to send I/O requests to the
    /// kernel.
    #[inline]
    pub fn submission(&mut self) -> SubmissionQueue<'_, S> {
        self.sq.borrow()
    }

    /// Get the submission queue of the io_uring instance from a shared reference.
    ///
    /// # Safety
    ///
    /// No other [`SubmissionQueue`]s may exist when calling this function.
    #[inline]
    pub unsafe fn submission_shared(&self) -> SubmissionQueue<'_, S> {
        self.sq.borrow_shared()
    }

    /// Get completion queue of the io_uring instance. This is used to receive I/O completion
    /// events from the kernel.
    #[inline]
    pub fn completion(&mut self) -> CompletionQueue<'_, C> {
        // Flush pending dispatch threads: join background threads, touch
        // user buffers, and post CQEs. This is the point where Miri can
        // detect use-after-free or data races on buffers freed/modified
        // between submit() and now.
        self.flush_pending();
        self.cq.borrow()
    }

    /// Get the completion queue of the io_uring instance from a shared reference.
    ///
    /// # Safety
    ///
    /// No other [`CompletionQueue`]s may exist when calling this function.
    #[inline]
    pub unsafe fn completion_shared(&self) -> CompletionQueue<'_, C> {
        self.flush_pending();
        self.cq.borrow_shared()
    }
}

impl<S: squeue::EntryMarker, C: cqueue::EntryMarker> IoUring<S, C> {
    /// Join any pending dispatch threads and post their CQEs.
    fn flush_pending(&self) {
        self.submitter().flush_pending();
    }
}

// Drop order: sq, cq, params, memory (Rust drops fields in declaration order).
// Inner<S>/Inner<C> don't have Drop impls, so memory is freed last — correct.

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

impl<S: squeue::EntryMarker, C: cqueue::EntryMarker> Builder<S, C> {
    /// Perform busy-waiting for I/O completion events, as opposed to getting notifications via an
    /// asynchronous IRQ (Interrupt Request).
    pub fn setup_iopoll(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_IOPOLL;
        self
    }

    /// Use a kernel thread to perform submission queue polling.
    pub fn setup_sqpoll(&mut self, idle: u32) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_SQPOLL;
        self.params.sq_thread_idle = idle;
        self
    }

    /// Bind the kernel's poll thread to the specified cpu.
    pub fn setup_sqpoll_cpu(&mut self, cpu: u32) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_SQ_AFF;
        self.params.sq_thread_cpu = cpu;
        self
    }

    /// Create the completion queue with the specified number of entries.
    pub fn setup_cqsize(&mut self, entries: u32) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_CQSIZE;
        self.params.cq_entries = entries;
        self
    }

    /// Clamp the sizes of the submission queue and completion queue at their maximum values.
    pub fn setup_clamp(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_CLAMP;
        self
    }

    /// Share the asynchronous worker thread backend of this io_uring with the specified io_uring
    /// file descriptor instead of creating a new thread pool.
    pub fn setup_attach_wq(&mut self, fd: i32) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_ATTACH_WQ;
        self.params.wq_fd = fd as u32;
        self
    }

    /// Start the io_uring instance with all its rings disabled.
    pub fn setup_r_disabled(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_R_DISABLED;
        self
    }

    /// Continue submitting requests even if one results in an error.
    pub fn setup_submit_all(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_SUBMIT_ALL;
        self
    }

    /// Reduce interrupt overhead from completion events.
    pub fn setup_coop_taskrun(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_COOP_TASKRUN;
        self
    }

    /// Used in conjunction with `setup_coop_taskrun`, provides a flag for pending completions.
    pub fn setup_taskrun_flag(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_TASKRUN_FLAG;
        self
    }

    /// Defer work until an explicit enter call with `IORING_ENTER_GETEVENTS`.
    pub fn setup_defer_taskrun(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_DEFER_TASKRUN;
        self
    }

    /// Hint that a single task will submit requests.
    pub fn setup_single_issuer(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_SINGLE_ISSUER;
        self
    }

    /// Do not make this io_uring instance accessible by child processes after a fork.
    /// (No-op in the emulator since there is no mmap to protect.)
    pub fn dontfork(&mut self) -> &mut Self {
        self
    }

    /// Build an [`IoUring`], with the specified number of entries in the submission queue and
    /// completion queue unless [`setup_cqsize`](Self::setup_cqsize) has been called.
    pub fn build(&self, entries: u32) -> io::Result<IoUring<S, C>> {
        if entries == 0 || entries > (1 << 15) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }

        // Round SQ entries up to next power of 2
        let sq_entries = entries.next_power_of_two();

        // CQ entries: if CQSIZE is set, use the requested value; otherwise 2 * sq_entries
        let cq_entries = if self.params.flags & sys::IORING_SETUP_CQSIZE != 0 {
            let cq = self.params.cq_entries;
            if cq == 0 {
                return Err(io::Error::from(io::ErrorKind::InvalidInput));
            }
            cq.next_power_of_two()
        } else {
            cmp::min(sq_entries * 2, 1 << 16)
        };

        let sq_ring_mask = sq_entries - 1;
        let cq_ring_mask = cq_entries - 1;

        // Allocate ring state and convert to raw pointer immediately.
        // Using Box::into_raw avoids Stacked Borrows issues: moving a Box
        // into a struct would create a Unique retag that invalidates any
        // pointers derived before the move. With raw pointers, provenance
        // is preserved and Miri is happy.
        let state = Box::into_raw(Box::new(RingState {
            sq_head: AtomicU32::new(0),
            sq_tail: AtomicU32::new(0),
            sq_flags: AtomicU32::new(0),
            sq_dropped: AtomicU32::new(0),
            cq_head: AtomicU32::new(0),
            cq_tail: AtomicU32::new(0),
            cq_overflow: AtomicU32::new(0),
            cq_flags: AtomicU32::new(0),
            pending: UnsafeCell::new(Vec::new()),
        }));

        // Allocate zero-initialized SQE and CQE arrays
        let sqes: Box<[S]> = (0..sq_entries)
            .map(|_| unsafe { mem::zeroed() })
            .collect();
        let sqes = Box::into_raw(sqes);

        let cqes: Box<[C]> = (0..cq_entries)
            .map(|_| unsafe { mem::zeroed() })
            .collect();
        let cqes = Box::into_raw(cqes);

        // Report all emulator-supported features
        let features = sys::IORING_FEAT_SINGLE_MMAP
            | sys::IORING_FEAT_NODROP
            | sys::IORING_FEAT_SUBMIT_STABLE
            | sys::IORING_FEAT_RW_CUR_POS
            | sys::IORING_FEAT_CUR_PERSONALITY
            | sys::IORING_FEAT_FAST_POLL
            | sys::IORING_FEAT_POLL_32BITS
            | sys::IORING_FEAT_SQPOLL_NONFIXED
            | sys::IORING_FEAT_EXT_ARG
            | sys::IORING_FEAT_NATIVE_WORKERS
            | sys::IORING_FEAT_RSRC_TAGS
            | sys::IORING_FEAT_CQE_SKIP
            | sys::IORING_FEAT_LINKED_FILE
            | sys::IORING_FEAT_REG_REG_RING;

        let params = Parameters(sys::io_uring_params {
            sq_entries,
            cq_entries,
            flags: self.params.flags,
            sq_thread_cpu: self.params.sq_thread_cpu,
            sq_thread_idle: self.params.sq_thread_idle,
            features,
            wq_fd: self.params.wq_fd,
            resv: [0; 3],
            sq_off: Default::default(),
            cq_off: Default::default(),
        });

        // Wire up Inner pointers using addr_of! to avoid creating
        // intermediate references (which would violate Stacked Borrows).
        let sq = unsafe {
            squeue::Inner::new(
                ptr::addr_of!((*state).sq_head),
                ptr::addr_of!((*state).sq_tail),
                sq_ring_mask,
                sq_entries,
                ptr::addr_of!((*state).sq_flags),
                ptr::addr_of!((*state).sq_dropped),
                sqes as *mut S,
            )
        };

        let cq = unsafe {
            cqueue::Inner::new(
                ptr::addr_of!((*state).cq_head),
                ptr::addr_of!((*state).cq_tail),
                cq_ring_mask,
                cq_entries,
                ptr::addr_of!((*state).cq_overflow),
                cqes as *mut C,
                ptr::addr_of!((*state).cq_flags),
            )
        };

        let memory = RingMemory { state, sqes, cqes };

        Ok(IoUring {
            sq,
            cq,
            params,
            memory,
        })
    }
}

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

impl Parameters {
    /// Whether a kernel thread is performing queue polling.
    pub fn is_setup_sqpoll(&self) -> bool {
        self.0.flags & sys::IORING_SETUP_SQPOLL != 0
    }

    /// Whether waiting for completion events is done with a busy loop.
    pub fn is_setup_iopoll(&self) -> bool {
        self.0.flags & sys::IORING_SETUP_IOPOLL != 0
    }

    /// Whether the single issuer hint is enabled.
    pub fn is_setup_single_issuer(&self) -> bool {
        self.0.flags & sys::IORING_SETUP_SINGLE_ISSUER != 0
    }

    /// If this flag is set, the SQ and CQ rings were mapped with a single mmap call.
    pub fn is_feature_single_mmap(&self) -> bool {
        self.0.features & sys::IORING_FEAT_SINGLE_MMAP != 0
    }

    /// If this flag is set, io_uring supports never dropping completion events.
    pub fn is_feature_nodrop(&self) -> bool {
        self.0.features & sys::IORING_FEAT_NODROP != 0
    }

    /// If this flag is set, data for async offload has been consumed when the SQE is consumed.
    pub fn is_feature_submit_stable(&self) -> bool {
        self.0.features & sys::IORING_FEAT_SUBMIT_STABLE != 0
    }

    /// If this flag is set, applications can specify offset == -1 with read/write ops.
    pub fn is_feature_rw_cur_pos(&self) -> bool {
        self.0.features & sys::IORING_FEAT_RW_CUR_POS != 0
    }

    /// If this flag is set, sync and async execution assume the credentials of the calling task.
    pub fn is_feature_cur_personality(&self) -> bool {
        self.0.features & sys::IORING_FEAT_CUR_PERSONALITY != 0
    }

    /// Whether async pollable I/O is fast.
    pub fn is_feature_fast_poll(&self) -> bool {
        self.0.features & sys::IORING_FEAT_FAST_POLL != 0
    }

    /// Whether poll events are stored using 32 bits instead of 16.
    pub fn is_feature_poll_32bits(&self) -> bool {
        self.0.features & sys::IORING_FEAT_POLL_32BITS != 0
    }

    /// Whether SQPOLL can use non-fixed files.
    pub fn is_feature_sqpoll_nonfixed(&self) -> bool {
        self.0.features & sys::IORING_FEAT_SQPOLL_NONFIXED != 0
    }

    /// Whether the extended argument to io_uring_enter is supported.
    pub fn is_feature_ext_arg(&self) -> bool {
        self.0.features & sys::IORING_FEAT_EXT_ARG != 0
    }

    /// Whether io_uring uses native workers.
    pub fn is_feature_native_workers(&self) -> bool {
        self.0.features & sys::IORING_FEAT_NATIVE_WORKERS != 0
    }

    /// Whether the kernel supports tagging resources.
    pub fn is_feature_resource_tagging(&self) -> bool {
        self.0.features & sys::IORING_FEAT_RSRC_TAGS != 0
    }

    /// Whether the kernel supports `IOSQE_CQE_SKIP_SUCCESS`.
    pub fn is_feature_skip_cqe_on_success(&self) -> bool {
        self.0.features & sys::IORING_FEAT_CQE_SKIP != 0
    }

    /// Whether the kernel supports deferred file assignment.
    pub fn is_feature_linked_file(&self) -> bool {
        self.0.features & sys::IORING_FEAT_LINKED_FILE != 0
    }

    /// Whether the kernel supports `IORING_RECVSEND_BUNDLE`.
    pub fn is_feature_recvsend_bundle(&self) -> bool {
        self.0.features & sys::IORING_FEAT_RECVSEND_BUNDLE != 0
    }

    /// Whether the kernel supports `min_wait_usec` in submit args.
    pub fn is_feature_min_timeout(&self) -> bool {
        self.0.features & sys::IORING_FEAT_MIN_TIMEOUT != 0
    }

    /// The number of submission queue entries allocated.
    pub fn sq_entries(&self) -> u32 {
        self.0.sq_entries
    }

    /// The idle time of the SQ poll thread in milliseconds.
    pub fn sq_thread_idle(&self) -> u32 {
        self.0.sq_thread_idle
    }

    /// The number of completion queue entries allocated.
    pub fn cq_entries(&self) -> u32 {
        self.0.cq_entries
    }
}

impl std::fmt::Debug for Parameters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Parameters")
            .field("is_setup_sqpoll", &self.is_setup_sqpoll())
            .field("is_setup_iopoll", &self.is_setup_iopoll())
            .field("is_setup_single_issuer", &self.is_setup_single_issuer())
            .field("is_feature_single_mmap", &self.is_feature_single_mmap())
            .field("is_feature_nodrop", &self.is_feature_nodrop())
            .field("is_feature_submit_stable", &self.is_feature_submit_stable())
            .field("is_feature_rw_cur_pos", &self.is_feature_rw_cur_pos())
            .field(
                "is_feature_cur_personality",
                &self.is_feature_cur_personality(),
            )
            .field("is_feature_poll_32bits", &self.is_feature_poll_32bits())
            .field("sq_entries", &self.0.sq_entries)
            .field("cq_entries", &self.0.cq_entries)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// AsRawFd — sentinel fd for API compatibility
// ---------------------------------------------------------------------------

#[cfg(unix)]
impl<S: squeue::EntryMarker, C: cqueue::EntryMarker> std::os::unix::io::AsRawFd
    for IoUring<S, C>
{
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        -1
    }
}

// ---------------------------------------------------------------------------
// Loom model tests — exhaustive interleaving verification
// ---------------------------------------------------------------------------
//
// Run with: RUSTFLAGS='--cfg loom' cargo test --release --lib loom_
//
// These tests use Loom to explore all possible thread interleavings of the
// ring protocol, verifying that atomic orderings are correct and no data
// races or lost updates can occur regardless of scheduling.

#[cfg(loom)]
#[cfg(test)]
mod loom_tests {
    use super::*;

    /// Verify the fundamental ring protocol: one thread pushes SQEs and
    /// submits, another thread reads completions. Loom explores all
    /// interleavings of the atomic operations on head/tail pointers.
    #[test]
    fn loom_push_submit_complete() {
        ::loom::model(|| {
            let mut ring = IoUring::new(4).unwrap();

            // Push a Nop SQE.
            let nop = opcode::Nop::new().build().user_data(42);
            unsafe { ring.submission().push(&nop).unwrap() };

            // Submit from the current thread (spawns dispatch thread internally).
            ring.submit().unwrap();

            // Completions are flushed when we access the CQ.
            let mut cq = ring.completion();
            let cqe = cq.next().unwrap();
            assert_eq!(cqe.user_data(), 42);
            assert_eq!(cqe.result(), 0);
        });
    }

    /// Verify that multiple SQEs in a batch are all completed and visible.
    #[test]
    fn loom_batch_submit() {
        ::loom::model(|| {
            let mut ring = IoUring::new(4).unwrap();

            let nop1 = opcode::Nop::new().build().user_data(1);
            let nop2 = opcode::Nop::new().build().user_data(2);
            unsafe {
                let mut sq = ring.submission();
                sq.push(&nop1).unwrap();
                sq.push(&nop2).unwrap();
            }

            ring.submit().unwrap();

            let mut cq = ring.completion();
            let cqe1 = cq.next().unwrap();
            let cqe2 = cq.next().unwrap();

            // Both completions should be visible with correct user_data.
            let mut user_datas = [cqe1.user_data(), cqe2.user_data()];
            user_datas.sort();
            assert_eq!(user_datas, [1, 2]);
        });
    }

    /// Verify the split() API: submitter and queues can be used independently
    /// with correct synchronization of head/tail pointers.
    #[test]
    fn loom_split_submit() {
        ::loom::model(|| {
            let mut ring = IoUring::new(4).unwrap();
            let (submitter, mut sq, mut cq) = ring.split();

            let nop = opcode::Nop::new().build().user_data(99);
            unsafe { sq.push(&nop).unwrap() };
            // sync() stores the tail — the submitter must see it.
            sq.sync();

            submitter.submit_and_wait(1).unwrap();

            cq.sync();
            let cqe = cq.next().unwrap();
            assert_eq!(cqe.user_data(), 99);
        });
    }

    /// Verify that the SQ head/tail protocol handles wrapping correctly
    /// across multiple submit/complete cycles.
    #[test]
    fn loom_sequential_submit_complete_cycles() {
        ::loom::model(|| {
            let mut ring = IoUring::new(2).unwrap();

            for i in 0u64..3 {
                let nop = opcode::Nop::new().build().user_data(i);
                unsafe { ring.submission().push(&nop).unwrap() };
                ring.submit().unwrap();

                let cqe = ring.completion().next().unwrap();
                assert_eq!(cqe.user_data(), i);
            }
        });
    }

    /// Verify that submit_and_wait correctly flushes completions
    /// (joins dispatch thread) before returning.
    #[test]
    fn loom_submit_and_wait_flushes() {
        ::loom::model(|| {
            let mut ring = IoUring::new(4).unwrap();

            let nop = opcode::Nop::new().build().user_data(7);
            unsafe { ring.submission().push(&nop).unwrap() };

            // submit_and_wait(1) should submit AND flush.
            ring.submit_and_wait(1).unwrap();

            // CQE should be ready immediately.
            let cqe = ring.completion().next().unwrap();
            assert_eq!(cqe.user_data(), 7);
        });
    }
}
