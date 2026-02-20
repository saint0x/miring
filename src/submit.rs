use crate::loom::atomic;
use crate::loom::cell::UnsafeCell;
use crate::loom::thread;
use std::{io, ptr};

use crate::register::{Probe, Restriction};
use crate::sys;
use crate::types::{CancelBuilder, Timespec};
use crate::Parameters;
use bitflags::bitflags;

use crate::types;

/// Result of dispatching a single SQE on the background thread.
pub(crate) struct CompletionData {
    user_data: u64,
    result: i32,
    skip_success: bool,
}

/// Queue of background dispatch threads waiting to be joined.
/// Each submit() call spawns one thread that processes all SQEs in the batch.
pub(crate) type PendingBatches = Vec<thread::JoinHandle<Vec<CompletionData>>>;

bitflags!(
    /// io_uring_enter flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct EnterFlags: u32 {
        const GETEVENTS = sys::IORING_ENTER_GETEVENTS;
        const SQ_WAKEUP = sys::IORING_ENTER_SQ_WAKEUP;
        const SQ_WAIT = sys::IORING_ENTER_SQ_WAIT;
        const EXT_ARG = sys::IORING_ENTER_EXT_ARG;
        const REGISTERED_RING = sys::IORING_ENTER_REGISTERED_RING;
        const ABS_TIMER = sys::IORING_ENTER_ABS_TIMER;
        const EXT_ARG_REG = sys::IORING_ENTER_EXT_ARG_REG;
        const NO_IOWAIT = sys::IORING_ENTER_NO_IOWAIT;
    }
);

/// Submitter for the emulated io_uring ring.
///
/// In the real io-uring crate, this wraps `io_uring_enter()` syscalls.
/// In miring, it directly processes submission queue entries and posts completions.
pub struct Submitter<'a> {
    params: &'a Parameters,

    // SQ ring pointers
    sq_head: *const atomic::AtomicU32,
    sq_tail: *const atomic::AtomicU32,
    sq_flags: *const atomic::AtomicU32,
    sq_ring_mask: u32,

    // SQE array (type-erased: first 64 bytes are always io_uring_sqe)
    sqe_array: *const u8,
    sqe_stride: usize,

    // CQ ring pointers
    cq_head: *const atomic::AtomicU32,
    cq_tail: *const atomic::AtomicU32,
    cq_ring_mask: u32,
    cq_ring_entries: u32,
    cq_overflow: *const atomic::AtomicU32,

    // CQE array (type-erased: first 16 bytes are always io_uring_cqe)
    cqe_array: *mut u8,
    cqe_stride: usize,

    // Deferred completion: submit() spawns a background thread that dispatches
    // (touches buffers). Completion reaping joins the thread.
    pending: *const UnsafeCell<PendingBatches>,
}

impl<'a> Submitter<'a> {
    #[inline]
    pub(crate) fn new(
        params: &'a Parameters,
        sq_head: *const atomic::AtomicU32,
        sq_tail: *const atomic::AtomicU32,
        sq_flags: *const atomic::AtomicU32,
        sq_ring_mask: u32,
        sqe_array: *const u8,
        sqe_stride: usize,
        cq_head: *const atomic::AtomicU32,
        cq_tail: *const atomic::AtomicU32,
        cq_ring_mask: u32,
        cq_ring_entries: u32,
        cq_overflow: *const atomic::AtomicU32,
        cqe_array: *mut u8,
        cqe_stride: usize,
        pending: *const UnsafeCell<PendingBatches>,
    ) -> Self {
        Self {
            params,
            sq_head,
            sq_tail,
            sq_flags,
            sq_ring_mask,
            sqe_array,
            sqe_stride,
            cq_head,
            cq_tail,
            cq_ring_mask,
            cq_ring_entries,
            cq_overflow,
            cqe_array,
            cqe_stride,
            pending,
        }
    }

    #[inline]
    fn sq_len(&self) -> usize {
        unsafe {
            let head = (*self.sq_head).load(atomic::Ordering::Acquire);
            let tail = (*self.sq_tail).load(atomic::Ordering::Acquire);
            tail.wrapping_sub(head) as usize
        }
    }

    fn sq_need_wakeup(&self) -> bool {
        unsafe {
            (*self.sq_flags).load(atomic::Ordering::Relaxed) & sys::IORING_SQ_NEED_WAKEUP != 0
        }
    }

    fn sq_cq_overflow(&self) -> bool {
        unsafe {
            (*self.sq_flags).load(atomic::Ordering::Relaxed) & sys::IORING_SQ_CQ_OVERFLOW != 0
        }
    }

    /// Emulated io_uring_enter.
    ///
    /// # Safety
    ///
    /// This provides a raw interface so developer must ensure that parameters are correct.
    pub unsafe fn enter<T: Sized>(
        &self,
        to_submit: u32,
        min_complete: u32,
        _flag: u32,
        _arg: Option<&T>,
    ) -> io::Result<usize> {
        let submitted = self.enqueue_sqes(to_submit)?;
        if min_complete > 0 {
            self.flush_pending();
        }
        Ok(submitted)
    }

    /// Submit all queued submission queue events.
    #[inline]
    pub fn submit(&self) -> io::Result<usize> {
        self.submit_and_wait(0)
    }

    /// Submit all queued submission queue events and wait for at least `want` completions.
    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        let len = self.sq_len();
        let submitted = unsafe { self.enqueue_sqes(len as u32)? };
        if want > 0 {
            self.flush_pending();
        }
        Ok(submitted)
    }

    /// Submit with extended arguments.
    pub fn submit_with_args(
        &self,
        want: usize,
        _args: &types::SubmitArgs<'_, '_>,
    ) -> io::Result<usize> {
        let len = self.sq_len();
        let submitted = unsafe { self.enqueue_sqes(len as u32)? };
        if want > 0 {
            self.flush_pending();
        }
        Ok(submitted)
    }

    /// Wait for the submission queue to have free entries.
    pub fn squeue_wait(&self) -> io::Result<usize> {
        // In the emulator, the SQ is always immediately available
        Ok(0)
    }

    // -----------------------------------------------------------------------
    // Core ring processing — this is the "kernel" of miring
    // -----------------------------------------------------------------------

    /// Read SQEs from the submission queue, advance the SQ head, and spawn a
    /// background thread to dispatch the operations (touching user buffers).
    ///
    /// The background thread is NOT joined here — it runs concurrently with
    /// user code. Buffer touches happen on that thread, so if the user modifies
    /// or frees a buffer between submit() and completion(), Miri detects the
    /// data race or use-after-free.
    ///
    /// Call [`flush_pending`] to join the thread and post CQEs.
    unsafe fn enqueue_sqes(&self, to_submit: u32) -> io::Result<usize> {
        let mut sq_head = (*self.sq_head).load(atomic::Ordering::Acquire);
        let sq_tail = (*self.sq_tail).load(atomic::Ordering::Acquire);

        let pending = sq_tail.wrapping_sub(sq_head);
        let to_process = (to_submit).min(pending);

        if to_process == 0 {
            return Ok(0);
        }

        // Snapshot all SQEs to dispatch
        let mut sqes = Vec::with_capacity(to_process as usize);

        for _ in 0..to_process {
            let sq_idx = (sq_head & self.sq_ring_mask) as usize;
            let sqe_ptr = self.sqe_array.add(sq_idx * self.sqe_stride) as *const sys::io_uring_sqe;
            sqes.push(ptr::read(sqe_ptr));
            sq_head = sq_head.wrapping_add(1);
        }

        // Advance the SQ head — from the user's perspective, the SQEs are consumed.
        (*self.sq_head).store(sq_head, atomic::Ordering::Release);

        // Spawn a background thread that dispatches each operation.
        // Buffer touches happen HERE, on a different thread than the user code.
        // This means Miri's data-race detector will flag any unsynchronized
        // concurrent access to the same buffers by the user.
        //
        // Linked operations: SQEs with IOSQE_IO_LINK or IOSQE_IO_HARDLINK form
        // chains. If a non-hardlinked predecessor fails (result < 0), subsequent
        // linked SQEs get -ECANCELED (-125) without dispatch. Hardlinked SQEs
        // always dispatch regardless of predecessor result.
        let handle = thread::spawn(move || {
            const ECANCELED: i32 = -125;
            let link_bit = crate::squeue::Flags::IO_LINK.bits();
            let hardlink_bit = crate::squeue::Flags::IO_HARDLINK.bits();

            let mut results = Vec::with_capacity(sqes.len());
            // Track whether we're inside a link chain that has been cancelled.
            let mut cancel_chain = false;

            for sqe in &sqes {
                // Yield to give the main thread a chance to run interleaved
                // user code. Under Loom, this is an explicit interleaving
                // point that Loom will explore. Under Miri with
                // -Zmiri-preemption-rate>0, random preemption occurs here.
                thread::yield_now();

                let skip_success =
                    sqe.flags & crate::squeue::Flags::SKIP_SUCCESS.bits() != 0;

                let result = if cancel_chain {
                    // This SQE is part of a cancelled link chain — the
                    // predecessor had IO_LINK (not IO_HARDLINK) and failed.
                    ECANCELED
                } else {
                    dispatch_op(sqe)
                };

                // Determine if this SQE links to the next one.
                let links_to_next = (sqe.flags & link_bit != 0)
                    || (sqe.flags & hardlink_bit != 0);

                // If this SQE failed and it links to the next (non-hardlink),
                // mark the chain as cancelled.
                if links_to_next && result < 0 {
                    // The hardlink flag is on the *predecessor* — it means
                    // "my successor should still run even if I fail."
                    // Only propagate cancellation for soft links (IO_LINK).
                    let current_is_hardlink = sqe.flags & hardlink_bit != 0;
                    if !current_is_hardlink {
                        cancel_chain = true;
                    }
                }

                // If this SQE doesn't link to the next, the chain ends.
                if !links_to_next {
                    cancel_chain = false;
                }

                results.push(CompletionData {
                    user_data: sqe.user_data,
                    result,
                    skip_success,
                });
            }
            results
        });

        // Store the join handle — thread will be joined at completion-reap time.
        let batches = &mut *UnsafeCell::raw_get(self.pending);
        batches.push(handle);

        Ok(to_process as usize)
    }

    /// Join all pending dispatch threads and post their CQEs.
    ///
    /// This is the "completion" side of the deferred model. Called from:
    /// - `IoUring::completion()` / `completion_shared()`
    /// - `submit_and_wait(n)` with n > 0
    /// - `IoUring::split()`
    pub(crate) fn flush_pending(&self) {
        unsafe {
            let batches = &mut *UnsafeCell::raw_get(self.pending);
            for handle in batches.drain(..) {
                let results = handle.join().expect("dispatch thread panicked");
                for cd in results {
                    if !(cd.skip_success && cd.result >= 0) {
                        self.post_cqe(cd.user_data, cd.result, 0);
                    }
                }
            }
        }
    }

    /// Post a completion queue entry.
    unsafe fn post_cqe(&self, user_data: u64, res: i32, flags: u32) {
        let cq_tail = (*self.cq_tail).load(atomic::Ordering::Acquire);
        let cq_head = (*self.cq_head).load(atomic::Ordering::Acquire);

        let cq_len = cq_tail.wrapping_sub(cq_head);
        if cq_len >= self.cq_ring_entries {
            // CQ is full — increment overflow counter
            let prev = (*self.cq_overflow).fetch_add(1, atomic::Ordering::Relaxed);
            if prev == 0 {
                // Set the CQ overflow flag on the SQ flags
                (*self.sq_flags).fetch_or(sys::IORING_SQ_CQ_OVERFLOW, atomic::Ordering::Relaxed);
            }
            return;
        }

        let cq_idx = (cq_tail & self.cq_ring_mask) as usize;
        let cqe_ptr = self.cqe_array.add(cq_idx * self.cqe_stride) as *mut sys::io_uring_cqe;

        // Write the base CQE fields (first 16 bytes)
        ptr::write(
            cqe_ptr,
            sys::io_uring_cqe {
                user_data,
                res,
                flags,
            },
        );

        // If using 32-byte CQEs, zero the extra 16 bytes
        if self.cqe_stride > core::mem::size_of::<sys::io_uring_cqe>() {
            let extra = cqe_ptr.add(1) as *mut u64;
            ptr::write(extra, 0u64);
            ptr::write(extra.add(1), 0u64);
        }

        // Advance CQ tail
        (*self.cq_tail).store(cq_tail.wrapping_add(1), atomic::Ordering::Release);
    }

    // -----------------------------------------------------------------------
    // Register operations — emulated as no-ops or simple state
    // -----------------------------------------------------------------------

    pub unsafe fn register_buffers(&self, _bufs: &[IoVec]) -> io::Result<()> {
        Ok(())
    }

    pub fn unregister_buffers(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn register_files(&self, _fds: &[i32]) -> io::Result<()> {
        Ok(())
    }

    pub fn register_files_update(&self, _offset: u32, _fds: &[i32]) -> io::Result<usize> {
        Ok(0)
    }

    pub fn register_files_sparse(&self, _nr: u32) -> io::Result<()> {
        Ok(())
    }

    pub fn unregister_files(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn register_eventfd(&self, _eventfd: i32) -> io::Result<()> {
        Ok(())
    }

    pub fn register_eventfd_async(&self, _eventfd: i32) -> io::Result<()> {
        Ok(())
    }

    pub fn unregister_eventfd(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn register_probe(&self, probe: &mut Probe) -> io::Result<()> {
        // Report only opcodes that dispatch_op actually handles (not -ENOSYS).
        // This mirrors the dispatch_op match arms exactly.
        for &op in HANDLED_OPCODES {
            probe.set_supported(op as u8);
        }
        Ok(())
    }

    pub fn register_personality(&self) -> io::Result<u16> {
        Ok(0)
    }

    pub fn unregister_personality(&self, _personality: u16) -> io::Result<()> {
        Ok(())
    }

    pub fn register_restrictions(&self, _res: &mut [Restriction]) -> io::Result<()> {
        Ok(())
    }

    pub fn register_enable_rings(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn register_iowq_aff(&self, _cpu_set: &[u8]) -> io::Result<()> {
        Ok(())
    }

    pub fn unregister_iowq_aff(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn register_iowq_max_workers(&self, _max: &mut [u32; 2]) -> io::Result<()> {
        Ok(())
    }

    pub unsafe fn register_buf_ring_with_flags(
        &self,
        _ring_addr: u64,
        _ring_entries: u16,
        _bgid: u16,
        _flags: u16,
    ) -> io::Result<()> {
        Ok(())
    }

    pub fn unregister_buf_ring(&self, _bgid: u16) -> io::Result<()> {
        Ok(())
    }

    pub fn register_sync_cancel(
        &self,
        _timeout: Option<Timespec>,
        _builder: CancelBuilder,
    ) -> io::Result<()> {
        Ok(())
    }

    pub unsafe fn register_buffers_update(
        &self,
        _offset: u32,
        _bufs: &[IoVec],
        _tags: Option<&[u64]>,
    ) -> io::Result<()> {
        Ok(())
    }

    pub unsafe fn register_buffers2(
        &self,
        _bufs: &[IoVec],
        _tags: &[u64],
    ) -> io::Result<()> {
        Ok(())
    }

    pub fn register_buffers_sparse(&self, _nr: u32) -> io::Result<()> {
        Ok(())
    }

    pub fn register_ifq(&self, _reg: &sys::io_uring_zcrx_ifq_reg) -> io::Result<()> {
        Ok(())
    }
}

/// Placeholder for iovec (Miri-compatible, no libc dependency).
#[repr(C)]
pub struct IoVec {
    pub iov_base: *mut std::ffi::c_void,
    pub iov_len: usize,
}

// ---------------------------------------------------------------------------
// iovec for vectored I/O dispatch
// ---------------------------------------------------------------------------

/// Layout-compatible with `struct iovec` / libc::iovec.
#[repr(C)]
struct Iovec {
    iov_base: *mut u8,
    iov_len: usize,
}

// ---------------------------------------------------------------------------
// Operation dispatch
// ---------------------------------------------------------------------------

/// All opcodes handled by dispatch_op (not returning -ENOSYS).
/// Used by register_probe to accurately report supported ops.
const HANDLED_OPCODES: &[u32] = &[
    sys::IORING_OP_NOP,
    sys::IORING_OP_READ,
    sys::IORING_OP_READ_FIXED,
    sys::IORING_OP_RECV,
    sys::IORING_OP_WRITE,
    sys::IORING_OP_WRITE_FIXED,
    sys::IORING_OP_SEND,
    sys::IORING_OP_SEND_ZC,
    sys::IORING_OP_READV,
    sys::IORING_OP_WRITEV,
    sys::IORING_OP_RECVMSG,
    sys::IORING_OP_SENDMSG,
    sys::IORING_OP_SENDMSG_ZC,
    sys::IORING_OP_TIMEOUT,
    sys::IORING_OP_LINK_TIMEOUT,
    sys::IORING_OP_CONNECT,
    sys::IORING_OP_ACCEPT,
    sys::IORING_OP_OPENAT,
    sys::IORING_OP_OPENAT2,
    sys::IORING_OP_STATX,
    sys::IORING_OP_UNLINKAT,
    sys::IORING_OP_MKDIRAT,
    sys::IORING_OP_RENAMEAT,
    sys::IORING_OP_SYMLINKAT,
    sys::IORING_OP_LINKAT,
    sys::IORING_OP_BIND,
    sys::IORING_OP_EPOLL_CTL,
    sys::IORING_OP_FILES_UPDATE,
    sys::IORING_OP_PROVIDE_BUFFERS,
    sys::IORING_OP_FSETXATTR,
    sys::IORING_OP_SETXATTR,
    sys::IORING_OP_FGETXATTR,
    sys::IORING_OP_GETXATTR,
    sys::IORING_OP_EPOLL_WAIT,
    sys::IORING_OP_READV_FIXED,
    sys::IORING_OP_WRITEV_FIXED,
    sys::IORING_OP_READ_MULTISHOT,
    sys::IORING_OP_FUTEX_WAIT,
    sys::IORING_OP_FUTEX_WAKE,
    sys::IORING_OP_FUTEX_WAITV,
    sys::IORING_OP_WAITID,
    sys::IORING_OP_PIPE,
    // No-op success ops (truly don't touch user buffers):
    sys::IORING_OP_CLOSE,
    sys::IORING_OP_FSYNC,
    sys::IORING_OP_SYNC_FILE_RANGE,
    sys::IORING_OP_FALLOCATE,
    sys::IORING_OP_FTRUNCATE,
    sys::IORING_OP_SHUTDOWN,
    sys::IORING_OP_POLL_ADD,
    sys::IORING_OP_POLL_REMOVE,
    sys::IORING_OP_ASYNC_CANCEL,
    sys::IORING_OP_TIMEOUT_REMOVE,
    sys::IORING_OP_SPLICE,
    sys::IORING_OP_TEE,
    sys::IORING_OP_REMOVE_BUFFERS,
    sys::IORING_OP_FADVISE,
    sys::IORING_OP_MADVISE,
    sys::IORING_OP_MSG_RING,
    sys::IORING_OP_LISTEN,
    sys::IORING_OP_URING_CMD,
    sys::IORING_OP_RECV_ZC,
    // Fd-returning ops:
    sys::IORING_OP_SOCKET,
    sys::IORING_OP_FIXED_FD_INSTALL,
];

/// Dispatch an SQE to the appropriate handler and return the result code.
///
/// This is the core of the emulator. Each handler touches user-supplied
/// buffers in the same way the kernel would, so that Miri can detect UB
/// (use-after-free, buffer overflow, uninitialized reads, etc.).
fn dispatch_op(sqe: &sys::io_uring_sqe) -> i32 {
    match sqe.opcode as u32 {
        sys::IORING_OP_NOP => dispatch_nop(sqe),

        // -- Buffer read ops: kernel WRITES to user buffer --
        // These validate the buffer is writable and within a live allocation.
        sys::IORING_OP_READ | sys::IORING_OP_READ_FIXED | sys::IORING_OP_RECV => {
            unsafe { dispatch_buf_fill(sqe) }
        }

        // -- Buffer write ops: kernel READS from user buffer --
        // These validate the buffer is readable and initialized.
        sys::IORING_OP_WRITE
        | sys::IORING_OP_WRITE_FIXED
        | sys::IORING_OP_SEND
        | sys::IORING_OP_SEND_ZC => unsafe { dispatch_buf_sink(sqe) },

        // -- Vectored read: kernel WRITES to scatter buffers --
        sys::IORING_OP_READV => unsafe { dispatch_readv(sqe) },

        // -- Vectored write: kernel READS from gather buffers --
        sys::IORING_OP_WRITEV => unsafe { dispatch_writev(sqe) },

        // -- RecvMsg: kernel reads msghdr, writes to scatter buffers --
        sys::IORING_OP_RECVMSG => unsafe { dispatch_recvmsg(sqe) },

        // -- SendMsg: kernel reads msghdr, reads from gather buffers --
        sys::IORING_OP_SENDMSG | sys::IORING_OP_SENDMSG_ZC => {
            unsafe { dispatch_sendmsg(sqe) }
        }

        // -- Timeout: kernel reads __kernel_timespec --
        sys::IORING_OP_TIMEOUT | sys::IORING_OP_LINK_TIMEOUT => {
            unsafe { dispatch_timeout(sqe) }
        }

        // -- Connect: kernel reads sockaddr --
        sys::IORING_OP_CONNECT => unsafe { dispatch_connect(sqe) },

        // -- Accept: kernel writes sockaddr + addrlen --
        sys::IORING_OP_ACCEPT => unsafe { dispatch_accept(sqe) },

        // -- OpenAt: kernel reads pathname --
        sys::IORING_OP_OPENAT => unsafe { dispatch_openat(sqe) },

        // -- OpenAt2: kernel reads pathname + open_how --
        sys::IORING_OP_OPENAT2 => unsafe { dispatch_openat2(sqe) },

        // -- Statx: kernel reads pathname, writes statx buffer --
        sys::IORING_OP_STATX => unsafe { dispatch_statx(sqe) },

        // -- Pathname ops: kernel reads NUL-terminated path string(s) --
        sys::IORING_OP_UNLINKAT | sys::IORING_OP_MKDIRAT => {
            unsafe { dispatch_pathname(sqe) }
        }

        // -- Two-pathname ops: kernel reads two NUL-terminated path strings --
        sys::IORING_OP_RENAMEAT
        | sys::IORING_OP_SYMLINKAT
        | sys::IORING_OP_LINKAT => unsafe { dispatch_pathname2(sqe) },

        // -- Bind: kernel reads sockaddr (same as connect) --
        sys::IORING_OP_BIND => unsafe { dispatch_connect(sqe) },

        // -- EpollCtl: kernel reads epoll_event struct --
        sys::IORING_OP_EPOLL_CTL => unsafe { dispatch_epoll_ctl(sqe) },

        // -- FilesUpdate: kernel reads fd array --
        sys::IORING_OP_FILES_UPDATE => unsafe { dispatch_files_update(sqe) },

        // -- ProvideBuffers: kernel reads buffer address range --
        sys::IORING_OP_PROVIDE_BUFFERS => unsafe { dispatch_provide_buffers(sqe) },

        // -- Xattr ops: kernel reads name + value/path --
        sys::IORING_OP_FSETXATTR | sys::IORING_OP_SETXATTR => {
            unsafe { dispatch_xattr_set(sqe) }
        }
        sys::IORING_OP_FGETXATTR | sys::IORING_OP_GETXATTR => {
            unsafe { dispatch_xattr_get(sqe) }
        }

        // -- EpollWait: kernel writes epoll_event array --
        sys::IORING_OP_EPOLL_WAIT => unsafe { dispatch_epoll_wait(sqe) },

        // -- Readv/Writev Fixed: same as their non-fixed variants --
        sys::IORING_OP_READV_FIXED => unsafe { dispatch_readv(sqe) },
        sys::IORING_OP_WRITEV_FIXED => unsafe { dispatch_writev(sqe) },

        // -- ReadMultishot: same buffer touching as Read --
        sys::IORING_OP_READ_MULTISHOT => unsafe { dispatch_buf_fill(sqe) },

        // -- Futex ops: kernel reads/writes futex word --
        sys::IORING_OP_FUTEX_WAIT | sys::IORING_OP_FUTEX_WAKE => {
            unsafe { dispatch_futex_read(sqe) }
        }
        sys::IORING_OP_FUTEX_WAITV => unsafe { dispatch_futex_waitv(sqe) },

        // -- WaitId: kernel writes siginfo buffer --
        sys::IORING_OP_WAITID => unsafe { dispatch_waitid(sqe) },

        // -- Pipe: kernel writes two fds to user array --
        sys::IORING_OP_PIPE => unsafe { dispatch_pipe(sqe) },

        // -- No-op success: operations that truly don't touch user buffers --
        sys::IORING_OP_CLOSE
        | sys::IORING_OP_FSYNC
        | sys::IORING_OP_SYNC_FILE_RANGE
        | sys::IORING_OP_FALLOCATE
        | sys::IORING_OP_FTRUNCATE
        | sys::IORING_OP_SHUTDOWN
        | sys::IORING_OP_POLL_ADD
        | sys::IORING_OP_POLL_REMOVE
        | sys::IORING_OP_ASYNC_CANCEL
        | sys::IORING_OP_TIMEOUT_REMOVE
        | sys::IORING_OP_SPLICE
        | sys::IORING_OP_TEE
        | sys::IORING_OP_REMOVE_BUFFERS
        | sys::IORING_OP_FADVISE
        | sys::IORING_OP_MADVISE
        | sys::IORING_OP_MSG_RING
        | sys::IORING_OP_LISTEN
        | sys::IORING_OP_URING_CMD
        | sys::IORING_OP_RECV_ZC => 0,

        // -- Fd-returning operations (return a sentinel fd) --
        sys::IORING_OP_SOCKET | sys::IORING_OP_FIXED_FD_INSTALL => 42,

        _ => -38, // -ENOSYS
    }
}

// ---------------------------------------------------------------------------
// NOP
// ---------------------------------------------------------------------------

fn dispatch_nop(sqe: &sys::io_uring_sqe) -> i32 {
    if sqe.op_flags & sys::IORING_NOP_INJECT_RESULT != 0 {
        sqe.len as i32
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Single-buffer ops
// ---------------------------------------------------------------------------

/// Read-like: kernel writes data INTO the user buffer.
/// Fills the buffer with zeros — Miri validates writability.
unsafe fn dispatch_buf_fill(sqe: &sys::io_uring_sqe) -> i32 {
    let addr = sqe.addr as *mut u8;
    let len = sqe.len as usize;
    if addr.is_null() || len == 0 {
        return 0;
    }
    let buf = core::slice::from_raw_parts_mut(addr, len);
    buf.fill(0);
    len as i32
}

/// Write-like: kernel reads data FROM the user buffer.
/// Reads every byte — Miri validates readability and initialization.
unsafe fn dispatch_buf_sink(sqe: &sys::io_uring_sqe) -> i32 {
    let addr = sqe.addr as *const u8;
    let len = sqe.len as usize;
    if addr.is_null() || len == 0 {
        return 0;
    }
    let buf = core::slice::from_raw_parts(addr, len);
    // Read every byte so Miri can check for uninitialized memory.
    let mut sink: u8 = 0;
    for &b in buf {
        sink = sink.wrapping_add(b);
    }
    core::hint::black_box(sink);
    len as i32
}

// ---------------------------------------------------------------------------
// Vectored I/O
// ---------------------------------------------------------------------------

/// Readv: kernel writes to scatter buffers described by an iovec array.
unsafe fn dispatch_readv(sqe: &sys::io_uring_sqe) -> i32 {
    let iov_ptr = sqe.addr as *const Iovec;
    let nr_vecs = sqe.len as usize;
    if iov_ptr.is_null() || nr_vecs == 0 {
        return 0;
    }
    let iovs = core::slice::from_raw_parts(iov_ptr, nr_vecs);
    let mut total: usize = 0;
    for iov in iovs {
        if !iov.iov_base.is_null() && iov.iov_len > 0 {
            let buf = core::slice::from_raw_parts_mut(iov.iov_base, iov.iov_len);
            buf.fill(0);
            total = total.saturating_add(iov.iov_len);
        }
    }
    total as i32
}

/// Writev: kernel reads from gather buffers described by an iovec array.
unsafe fn dispatch_writev(sqe: &sys::io_uring_sqe) -> i32 {
    let iov_ptr = sqe.addr as *const Iovec;
    let nr_vecs = sqe.len as usize;
    if iov_ptr.is_null() || nr_vecs == 0 {
        return 0;
    }
    let iovs = core::slice::from_raw_parts(iov_ptr, nr_vecs);
    let mut total: usize = 0;
    let mut sink: u8 = 0;
    for iov in iovs {
        if !iov.iov_base.is_null() && iov.iov_len > 0 {
            let buf = core::slice::from_raw_parts(iov.iov_base, iov.iov_len);
            for &b in buf {
                sink = sink.wrapping_add(b);
            }
            total = total.saturating_add(iov.iov_len);
        }
    }
    core::hint::black_box(sink);
    total as i32
}

// ---------------------------------------------------------------------------
// RecvMsg / SendMsg — validates msghdr + iovec chain
// ---------------------------------------------------------------------------

/// Portable msghdr layout (matches libc::msghdr on all unix platforms).
#[repr(C)]
struct MsgHdr {
    msg_name: *mut u8,
    msg_namelen: u32,
    // 4 bytes padding on 64-bit (inserted by C ABI)
    _pad0: u32,
    msg_iov: *mut Iovec,
    msg_iovlen: usize,
    msg_control: *mut u8,
    msg_controllen: usize,
    msg_flags: i32,
    _pad1: u32,
}

/// RecvMsg: kernel reads msghdr metadata, writes to scatter buffers.
unsafe fn dispatch_recvmsg(sqe: &sys::io_uring_sqe) -> i32 {
    let hdr_ptr = sqe.addr as *const MsgHdr;
    if hdr_ptr.is_null() {
        return -14; // -EFAULT
    }
    let hdr = &*hdr_ptr;

    // Touch msg_name (kernel writes sender address)
    if !hdr.msg_name.is_null() && hdr.msg_namelen > 0 {
        let name = core::slice::from_raw_parts_mut(hdr.msg_name, hdr.msg_namelen as usize);
        name.fill(0);
    }

    // Touch msg_control (kernel writes ancillary data)
    if !hdr.msg_control.is_null() && hdr.msg_controllen > 0 {
        let ctl = core::slice::from_raw_parts_mut(hdr.msg_control, hdr.msg_controllen);
        ctl.fill(0);
    }

    // Touch scatter buffers (kernel writes received data)
    let mut total: usize = 0;
    if !hdr.msg_iov.is_null() && hdr.msg_iovlen > 0 {
        let iovs = core::slice::from_raw_parts(hdr.msg_iov, hdr.msg_iovlen);
        for iov in iovs {
            if !iov.iov_base.is_null() && iov.iov_len > 0 {
                let buf = core::slice::from_raw_parts_mut(iov.iov_base, iov.iov_len);
                buf.fill(0);
                total = total.saturating_add(iov.iov_len);
            }
        }
    }
    total as i32
}

/// SendMsg: kernel reads msghdr metadata + gather buffers.
unsafe fn dispatch_sendmsg(sqe: &sys::io_uring_sqe) -> i32 {
    let hdr_ptr = sqe.addr as *const MsgHdr;
    if hdr_ptr.is_null() {
        return -14; // -EFAULT
    }
    let hdr = &*hdr_ptr;

    // Touch msg_name (kernel reads destination address)
    if !hdr.msg_name.is_null() && hdr.msg_namelen > 0 {
        let name = core::slice::from_raw_parts(hdr.msg_name, hdr.msg_namelen as usize);
        let mut sink: u8 = 0;
        for &b in name {
            sink = sink.wrapping_add(b);
        }
        core::hint::black_box(sink);
    }

    // Touch msg_control (kernel reads ancillary data)
    if !hdr.msg_control.is_null() && hdr.msg_controllen > 0 {
        let ctl = core::slice::from_raw_parts(hdr.msg_control, hdr.msg_controllen);
        let mut sink: u8 = 0;
        for &b in ctl {
            sink = sink.wrapping_add(b);
        }
        core::hint::black_box(sink);
    }

    // Touch gather buffers (kernel reads data to send)
    let mut total: usize = 0;
    let mut sink: u8 = 0;
    if !hdr.msg_iov.is_null() && hdr.msg_iovlen > 0 {
        let iovs = core::slice::from_raw_parts(hdr.msg_iov, hdr.msg_iovlen);
        for iov in iovs {
            if !iov.iov_base.is_null() && iov.iov_len > 0 {
                let buf = core::slice::from_raw_parts(iov.iov_base as *const u8, iov.iov_len);
                for &b in buf {
                    sink = sink.wrapping_add(b);
                }
                total = total.saturating_add(iov.iov_len);
            }
        }
    }
    core::hint::black_box(sink);
    total as i32
}

// ---------------------------------------------------------------------------
// Timeout — reads __kernel_timespec
// ---------------------------------------------------------------------------

unsafe fn dispatch_timeout(sqe: &sys::io_uring_sqe) -> i32 {
    let ts_ptr = sqe.addr as *const sys::__kernel_timespec;
    if ts_ptr.is_null() {
        return -14; // -EFAULT
    }
    // Read the timespec to validate the pointer
    let ts = core::ptr::read(ts_ptr);
    core::hint::black_box(ts);
    // Return -ETIME (the normal timeout completion)
    -62
}

// ---------------------------------------------------------------------------
// Connect — reads sockaddr
// ---------------------------------------------------------------------------

unsafe fn dispatch_connect(sqe: &sys::io_uring_sqe) -> i32 {
    let addr = sqe.addr as *const u8;
    // addr_len is stored in the off field for connect
    let addr_len = sqe.off as usize;
    if addr.is_null() || addr_len == 0 {
        return -14; // -EFAULT
    }
    // Read the sockaddr bytes to validate
    let buf = core::slice::from_raw_parts(addr, addr_len);
    let mut sink: u8 = 0;
    for &b in buf {
        sink = sink.wrapping_add(b);
    }
    core::hint::black_box(sink);
    0
}

// ---------------------------------------------------------------------------
// Accept — writes sockaddr + addrlen
// ---------------------------------------------------------------------------

unsafe fn dispatch_accept(sqe: &sys::io_uring_sqe) -> i32 {
    let addr = sqe.addr as *mut u8;
    let addrlen_ptr = sqe.off as *mut u32;

    // If sockaddr buffer provided, zero it and update addrlen
    if !addr.is_null() && !addrlen_ptr.is_null() {
        let addrlen = core::ptr::read(addrlen_ptr) as usize;
        if addrlen > 0 {
            let buf = core::slice::from_raw_parts_mut(addr, addrlen);
            buf.fill(0);
        }
        // Write back the "actual" length
        core::ptr::write(addrlen_ptr, 0);
    }
    // Return a fake fd
    42
}

// ---------------------------------------------------------------------------
// OpenAt — reads pathname string
// ---------------------------------------------------------------------------

unsafe fn dispatch_openat(sqe: &sys::io_uring_sqe) -> i32 {
    let path = sqe.addr as *const u8;
    if path.is_null() {
        return -14; // -EFAULT
    }
    walk_cstr(path);
    42 // fake fd
}

// ---------------------------------------------------------------------------
// OpenAt2 — reads pathname + open_how struct
// ---------------------------------------------------------------------------

unsafe fn dispatch_openat2(sqe: &sys::io_uring_sqe) -> i32 {
    let path = sqe.addr as *const u8;
    if path.is_null() {
        return -14; // -EFAULT
    }
    walk_cstr(path);
    // open_how struct pointer in off field
    let how_ptr = sqe.off as *const sys::open_how;
    if !how_ptr.is_null() {
        let how = core::ptr::read(how_ptr);
        core::hint::black_box(how);
    }
    42 // fake fd
}

// ---------------------------------------------------------------------------
// Statx — reads pathname, writes statx buffer
// ---------------------------------------------------------------------------

unsafe fn dispatch_statx(sqe: &sys::io_uring_sqe) -> i32 {
    // Pathname in addr
    let path = sqe.addr as *const u8;
    if !path.is_null() {
        walk_cstr(path);
    }
    // Statx buffer pointer in off field
    let statx_ptr = sqe.off as *mut u8;
    // statx struct is 256 bytes on Linux
    const STATX_SIZE: usize = 256;
    if !statx_ptr.is_null() {
        let buf = core::slice::from_raw_parts_mut(statx_ptr, STATX_SIZE);
        buf.fill(0);
    }
    0
}

// ---------------------------------------------------------------------------
// Shared pathname helpers
// ---------------------------------------------------------------------------

/// Walk a NUL-terminated C string to validate the pointer under Miri.
unsafe fn walk_cstr(mut p: *const u8) {
    while core::ptr::read(p) != 0 {
        p = p.add(1);
    }
}

/// Single-pathname ops: kernel reads a NUL-terminated path from `addr`.
/// Used by: UnlinkAt, MkDirAt
unsafe fn dispatch_pathname(sqe: &sys::io_uring_sqe) -> i32 {
    let path = sqe.addr as *const u8;
    if path.is_null() {
        return -14; // -EFAULT
    }
    walk_cstr(path);
    0
}

/// Two-pathname ops: kernel reads NUL-terminated paths from `addr` and `off`.
/// Used by: RenameAt (oldpath/newpath), SymlinkAt (target/linkpath), LinkAt (oldpath/newpath)
unsafe fn dispatch_pathname2(sqe: &sys::io_uring_sqe) -> i32 {
    let path1 = sqe.addr as *const u8;
    let path2 = sqe.off as *const u8;
    if path1.is_null() || path2.is_null() {
        return -14; // -EFAULT
    }
    walk_cstr(path1);
    walk_cstr(path2);
    0
}

// ---------------------------------------------------------------------------
// EpollCtl — reads epoll_event struct
// ---------------------------------------------------------------------------

unsafe fn dispatch_epoll_ctl(sqe: &sys::io_uring_sqe) -> i32 {
    let ev_ptr = sqe.addr as *const u8;
    if ev_ptr.is_null() {
        return 0; // epoll_ctl with NULL event is valid for EPOLL_CTL_DEL
    }
    // epoll_event is 12 bytes on Linux (4-byte events + 8-byte data)
    let buf = core::slice::from_raw_parts(ev_ptr, 12);
    let mut sink: u8 = 0;
    for &b in buf {
        sink = sink.wrapping_add(b);
    }
    core::hint::black_box(sink);
    0
}

// ---------------------------------------------------------------------------
// FilesUpdate — reads i32 fd array
// ---------------------------------------------------------------------------

unsafe fn dispatch_files_update(sqe: &sys::io_uring_sqe) -> i32 {
    let fds = sqe.addr as *const i32;
    let len = sqe.len as usize;
    if fds.is_null() || len == 0 {
        return 0;
    }
    let buf = core::slice::from_raw_parts(fds, len);
    let mut sink: i32 = 0;
    for &fd in buf {
        sink = sink.wrapping_add(fd);
    }
    core::hint::black_box(sink);
    len as i32
}

// ---------------------------------------------------------------------------
// ProvideBuffers — validates buffer address range
// ---------------------------------------------------------------------------

unsafe fn dispatch_provide_buffers(sqe: &sys::io_uring_sqe) -> i32 {
    let addr = sqe.addr as *mut u8;
    let buf_len = sqe.len as usize;
    let nbufs = sqe.fd as usize; // fd field holds nbufs for PROVIDE_BUFFERS
    if addr.is_null() || buf_len == 0 || nbufs == 0 {
        return 0;
    }
    // Validate entire range is writable: nbufs * buf_len bytes
    let total = nbufs.saturating_mul(buf_len);
    let buf = core::slice::from_raw_parts_mut(addr, total);
    // Touch first and last byte to validate the range without filling everything
    core::hint::black_box(buf[0]);
    core::hint::black_box(buf[total - 1]);
    0
}

// ---------------------------------------------------------------------------
// Xattr set — reads name (C string) + value buffer
// ---------------------------------------------------------------------------

unsafe fn dispatch_xattr_set(sqe: &sys::io_uring_sqe) -> i32 {
    // name is in addr field
    let name = sqe.addr as *const u8;
    if !name.is_null() {
        walk_cstr(name);
    }
    // value is in off field, length in len
    let value = sqe.off as *const u8;
    let len = sqe.len as usize;
    if !value.is_null() && len > 0 {
        let buf = core::slice::from_raw_parts(value, len);
        let mut sink: u8 = 0;
        for &b in buf {
            sink = sink.wrapping_add(b);
        }
        core::hint::black_box(sink);
    }
    // For SETXATTR (non-f variant), path is in addr3
    if sqe.opcode as u32 == sys::IORING_OP_SETXATTR {
        let path = sqe.addr3 as *const u8;
        if !path.is_null() {
            walk_cstr(path);
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Xattr get — reads name (C string), writes value buffer
// ---------------------------------------------------------------------------

unsafe fn dispatch_xattr_get(sqe: &sys::io_uring_sqe) -> i32 {
    // name is in addr field
    let name = sqe.addr as *const u8;
    if !name.is_null() {
        walk_cstr(name);
    }
    // value buffer is in off field, length in len
    let value = sqe.off as *mut u8;
    let len = sqe.len as usize;
    if !value.is_null() && len > 0 {
        let buf = core::slice::from_raw_parts_mut(value, len);
        buf.fill(0);
    }
    // For GETXATTR (non-f variant), path is in addr3
    if sqe.opcode as u32 == sys::IORING_OP_GETXATTR {
        let path = sqe.addr3 as *const u8;
        if !path.is_null() {
            walk_cstr(path);
        }
    }
    len as i32
}

// ---------------------------------------------------------------------------
// EpollWait — writes epoll_event array
// ---------------------------------------------------------------------------

unsafe fn dispatch_epoll_wait(sqe: &sys::io_uring_sqe) -> i32 {
    let events = sqe.addr as *mut u8;
    let max_events = sqe.len as usize;
    if events.is_null() || max_events == 0 {
        return 0;
    }
    // Each epoll_event is 12 bytes on Linux
    let total = max_events * 12;
    let buf = core::slice::from_raw_parts_mut(events, total);
    buf.fill(0);
    0 // return 0 events
}

// ---------------------------------------------------------------------------
// Futex — reads futex word
// ---------------------------------------------------------------------------

unsafe fn dispatch_futex_read(sqe: &sys::io_uring_sqe) -> i32 {
    let futex = sqe.addr as *const u32;
    if futex.is_null() {
        return -14; // -EFAULT
    }
    let val = core::ptr::read(futex);
    core::hint::black_box(val);
    0
}

// ---------------------------------------------------------------------------
// FutexWaitV — reads futex_waitv array
// ---------------------------------------------------------------------------

unsafe fn dispatch_futex_waitv(sqe: &sys::io_uring_sqe) -> i32 {
    let arr = sqe.addr as *const sys::futex_waitv;
    let nr = sqe.len as usize;
    if arr.is_null() || nr == 0 {
        return -14; // -EFAULT
    }
    let entries = core::slice::from_raw_parts(arr, nr);
    for entry in entries {
        core::hint::black_box(entry.val);
        core::hint::black_box(entry.uaddr);
        core::hint::black_box(entry.flags);
    }
    0
}

// ---------------------------------------------------------------------------
// WaitId — writes siginfo buffer
// ---------------------------------------------------------------------------

unsafe fn dispatch_waitid(sqe: &sys::io_uring_sqe) -> i32 {
    // siginfo_t pointer is in off (addr2) field
    let infop = sqe.off as *mut u8;
    if !infop.is_null() {
        // siginfo_t is 128 bytes on Linux
        let buf = core::slice::from_raw_parts_mut(infop, 128);
        buf.fill(0);
    }
    0
}

// ---------------------------------------------------------------------------
// Pipe — writes two fds to user array
// ---------------------------------------------------------------------------

unsafe fn dispatch_pipe(sqe: &sys::io_uring_sqe) -> i32 {
    let fds = sqe.addr as *mut i32;
    if fds.is_null() {
        return -14; // -EFAULT
    }
    // Write two fake pipe fds
    core::ptr::write(fds, 42);
    core::ptr::write(fds.add(1), 43);
    0
}
