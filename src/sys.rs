//! Kernel-compatible io_uring type definitions and constants.
//!
//! Hand-written pure Rust — no bindgen, no libc, no FFI.
//! Values match Linux 6.19 UAPI and io-uring crate v0.7.11 bindgen output.

#![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

// ---------------------------------------------------------------------------
// Primitive type aliases (matching kernel __u8 etc.)
// ---------------------------------------------------------------------------

pub type __u8 = u8;
pub type __u16 = u16;
pub type __s32 = i32;
pub type __u32 = u32;
pub type __u64 = u64;
pub type __kernel_time64_t = i64;

// ---------------------------------------------------------------------------
// IORING_SETUP_* flags (io_uring_setup params.flags)
// ---------------------------------------------------------------------------

pub const IORING_SETUP_IOPOLL: u32 = 1;
pub const IORING_SETUP_SQPOLL: u32 = 2;
pub const IORING_SETUP_SQ_AFF: u32 = 4;
pub const IORING_SETUP_CQSIZE: u32 = 8;
pub const IORING_SETUP_CLAMP: u32 = 16;
pub const IORING_SETUP_ATTACH_WQ: u32 = 32;
pub const IORING_SETUP_R_DISABLED: u32 = 64;
pub const IORING_SETUP_SUBMIT_ALL: u32 = 128;
pub const IORING_SETUP_COOP_TASKRUN: u32 = 256;
pub const IORING_SETUP_TASKRUN_FLAG: u32 = 512;
pub const IORING_SETUP_SQE128: u32 = 1024;
pub const IORING_SETUP_CQE32: u32 = 2048;
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 4096;
pub const IORING_SETUP_DEFER_TASKRUN: u32 = 8192;
pub const IORING_SETUP_NO_MMAP: u32 = 16384;
pub const IORING_SETUP_REGISTERED_FD_ONLY: u32 = 32768;
pub const IORING_SETUP_NO_SQARRAY: u32 = 65536;
pub const IORING_SETUP_HYBRID_IOPOLL: u32 = 131072;

// ---------------------------------------------------------------------------
// IOSQE_* per-SQE flags (bit positions)
// ---------------------------------------------------------------------------

pub type io_uring_sqe_flags_bit = u32;

pub const IOSQE_FIXED_FILE_BIT: io_uring_sqe_flags_bit = 0;
pub const IOSQE_IO_DRAIN_BIT: io_uring_sqe_flags_bit = 1;
pub const IOSQE_IO_LINK_BIT: io_uring_sqe_flags_bit = 2;
pub const IOSQE_IO_HARDLINK_BIT: io_uring_sqe_flags_bit = 3;
pub const IOSQE_ASYNC_BIT: io_uring_sqe_flags_bit = 4;
pub const IOSQE_BUFFER_SELECT_BIT: io_uring_sqe_flags_bit = 5;
pub const IOSQE_CQE_SKIP_SUCCESS_BIT: io_uring_sqe_flags_bit = 6;

// ---------------------------------------------------------------------------
// IORING_OP_* opcodes
// ---------------------------------------------------------------------------

pub type io_uring_op = u32;

pub const IORING_OP_NOP: io_uring_op = 0;
pub const IORING_OP_READV: io_uring_op = 1;
pub const IORING_OP_WRITEV: io_uring_op = 2;
pub const IORING_OP_FSYNC: io_uring_op = 3;
pub const IORING_OP_READ_FIXED: io_uring_op = 4;
pub const IORING_OP_WRITE_FIXED: io_uring_op = 5;
pub const IORING_OP_POLL_ADD: io_uring_op = 6;
pub const IORING_OP_POLL_REMOVE: io_uring_op = 7;
pub const IORING_OP_SYNC_FILE_RANGE: io_uring_op = 8;
pub const IORING_OP_SENDMSG: io_uring_op = 9;
pub const IORING_OP_RECVMSG: io_uring_op = 10;
pub const IORING_OP_TIMEOUT: io_uring_op = 11;
pub const IORING_OP_TIMEOUT_REMOVE: io_uring_op = 12;
pub const IORING_OP_ACCEPT: io_uring_op = 13;
pub const IORING_OP_ASYNC_CANCEL: io_uring_op = 14;
pub const IORING_OP_LINK_TIMEOUT: io_uring_op = 15;
pub const IORING_OP_CONNECT: io_uring_op = 16;
pub const IORING_OP_FALLOCATE: io_uring_op = 17;
pub const IORING_OP_OPENAT: io_uring_op = 18;
pub const IORING_OP_CLOSE: io_uring_op = 19;
pub const IORING_OP_FILES_UPDATE: io_uring_op = 20;
pub const IORING_OP_STATX: io_uring_op = 21;
pub const IORING_OP_READ: io_uring_op = 22;
pub const IORING_OP_WRITE: io_uring_op = 23;
pub const IORING_OP_FADVISE: io_uring_op = 24;
pub const IORING_OP_MADVISE: io_uring_op = 25;
pub const IORING_OP_SEND: io_uring_op = 26;
pub const IORING_OP_RECV: io_uring_op = 27;
pub const IORING_OP_OPENAT2: io_uring_op = 28;
pub const IORING_OP_EPOLL_CTL: io_uring_op = 29;
pub const IORING_OP_SPLICE: io_uring_op = 30;
pub const IORING_OP_PROVIDE_BUFFERS: io_uring_op = 31;
pub const IORING_OP_REMOVE_BUFFERS: io_uring_op = 32;
pub const IORING_OP_TEE: io_uring_op = 33;
pub const IORING_OP_SHUTDOWN: io_uring_op = 34;
pub const IORING_OP_RENAMEAT: io_uring_op = 35;
pub const IORING_OP_UNLINKAT: io_uring_op = 36;
pub const IORING_OP_MKDIRAT: io_uring_op = 37;
pub const IORING_OP_SYMLINKAT: io_uring_op = 38;
pub const IORING_OP_LINKAT: io_uring_op = 39;
pub const IORING_OP_MSG_RING: io_uring_op = 40;
pub const IORING_OP_FSETXATTR: io_uring_op = 41;
pub const IORING_OP_SETXATTR: io_uring_op = 42;
pub const IORING_OP_FGETXATTR: io_uring_op = 43;
pub const IORING_OP_GETXATTR: io_uring_op = 44;
pub const IORING_OP_SOCKET: io_uring_op = 45;
pub const IORING_OP_URING_CMD: io_uring_op = 46;
pub const IORING_OP_SEND_ZC: io_uring_op = 47;
pub const IORING_OP_SENDMSG_ZC: io_uring_op = 48;
pub const IORING_OP_READ_MULTISHOT: io_uring_op = 49;
pub const IORING_OP_WAITID: io_uring_op = 50;
pub const IORING_OP_FUTEX_WAIT: io_uring_op = 51;
pub const IORING_OP_FUTEX_WAKE: io_uring_op = 52;
pub const IORING_OP_FUTEX_WAITV: io_uring_op = 53;
pub const IORING_OP_FIXED_FD_INSTALL: io_uring_op = 54;
pub const IORING_OP_FTRUNCATE: io_uring_op = 55;
pub const IORING_OP_BIND: io_uring_op = 56;
pub const IORING_OP_LISTEN: io_uring_op = 57;
pub const IORING_OP_RECV_ZC: io_uring_op = 58;
pub const IORING_OP_EPOLL_WAIT: io_uring_op = 59;
pub const IORING_OP_READV_FIXED: io_uring_op = 60;
pub const IORING_OP_WRITEV_FIXED: io_uring_op = 61;
pub const IORING_OP_PIPE: io_uring_op = 62;
pub const IORING_OP_LAST: io_uring_op = 63;

// ---------------------------------------------------------------------------
// IORING_FSYNC_* / IORING_TIMEOUT_* / IORING_ASYNC_CANCEL_* etc.
// ---------------------------------------------------------------------------

pub const IORING_FSYNC_DATASYNC: u32 = 1;

pub const IORING_TIMEOUT_ABS: u32 = 1;
pub const IORING_TIMEOUT_UPDATE: u32 = 2;
pub const IORING_TIMEOUT_BOOTTIME: u32 = 4;
pub const IORING_TIMEOUT_REALTIME: u32 = 8;
pub const IORING_LINK_TIMEOUT_UPDATE: u32 = 16;
pub const IORING_TIMEOUT_ETIME_SUCCESS: u32 = 32;
pub const IORING_TIMEOUT_MULTISHOT: u32 = 64;
pub const IORING_TIMEOUT_CLOCK_MASK: u32 = 12;
pub const IORING_TIMEOUT_UPDATE_MASK: u32 = 18;

pub const SPLICE_F_FD_IN_FIXED: u32 = 2147483648;

pub const IORING_POLL_ADD_MULTI: u32 = 1;
pub const IORING_POLL_UPDATE_EVENTS: u32 = 2;
pub const IORING_POLL_UPDATE_USER_DATA: u32 = 4;
pub const IORING_POLL_ADD_LEVEL: u32 = 8;

pub const IORING_ASYNC_CANCEL_ALL: u32 = 1;
pub const IORING_ASYNC_CANCEL_FD: u32 = 2;
pub const IORING_ASYNC_CANCEL_ANY: u32 = 4;
pub const IORING_ASYNC_CANCEL_FD_FIXED: u32 = 8;
pub const IORING_ASYNC_CANCEL_USERDATA: u32 = 16;
pub const IORING_ASYNC_CANCEL_OP: u32 = 32;

pub const IORING_RECVSEND_POLL_FIRST: u32 = 1;
pub const IORING_RECV_MULTISHOT: u32 = 2;
pub const IORING_RECVSEND_FIXED_BUF: u32 = 4;
pub const IORING_SEND_ZC_REPORT_USAGE: u32 = 8;
pub const IORING_RECVSEND_BUNDLE: u32 = 16;
pub const IORING_NOTIF_USAGE_ZC_COPIED: u32 = 2147483648;

pub const IORING_ACCEPT_MULTISHOT: u32 = 1;
pub const IORING_ACCEPT_DONTWAIT: u32 = 2;
pub const IORING_ACCEPT_POLL_FIRST: u32 = 4;

pub const IORING_MSG_RING_CQE_SKIP: u32 = 1;
pub const IORING_MSG_RING_FLAGS_PASS: u32 = 2;

pub const IORING_FIXED_FD_NO_CLOEXEC: u32 = 1;

pub const IORING_NOP_INJECT_RESULT: u32 = 1;
pub const IORING_NOP_FILE: u32 = 2;
pub const IORING_NOP_FIXED_FILE: u32 = 4;
pub const IORING_NOP_FIXED_BUFFER: u32 = 8;

pub const IORING_URING_CMD_FIXED: u32 = 1;
pub const IORING_URING_CMD_MASK: u32 = 1;

pub const SOCKET_URING_OP_SETSOCKOPT: u32 = 0;
pub const SOCKET_URING_OP_GETSOCKOPT: u32 = 1;

pub const IORING_RW_ATTR_FLAG_PI: u32 = 1;

// ---------------------------------------------------------------------------
// CQE flags
// ---------------------------------------------------------------------------

pub const IORING_CQE_F_BUFFER: u32 = 1;
pub const IORING_CQE_F_MORE: u32 = 2;
pub const IORING_CQE_F_SOCK_NONEMPTY: u32 = 4;
pub const IORING_CQE_F_NOTIF: u32 = 8;
pub const IORING_CQE_F_BUF_MORE: u32 = 16;
pub const IORING_CQE_BUFFER_SHIFT: u32 = 16;

// ---------------------------------------------------------------------------
// mmap offsets (used as identifiers, not real offsets in the emulator)
// ---------------------------------------------------------------------------

pub const IORING_OFF_SQ_RING: u64 = 0;
pub const IORING_OFF_CQ_RING: u64 = 0x0800_0000;
pub const IORING_OFF_SQES: u64 = 0x1000_0000;
pub const IORING_OFF_PBUF_RING: u64 = 0x8000_0000;
pub const IORING_OFF_PBUF_SHIFT: u32 = 16;
pub const IORING_OFF_MMAP_MASK: u64 = 0xf800_0000;

// ---------------------------------------------------------------------------
// SQ ring flags
// ---------------------------------------------------------------------------

pub const IORING_SQ_NEED_WAKEUP: u32 = 1;
pub const IORING_SQ_CQ_OVERFLOW: u32 = 2;
pub const IORING_SQ_TASKRUN: u32 = 4;

// ---------------------------------------------------------------------------
// CQ ring flags
// ---------------------------------------------------------------------------

pub const IORING_CQ_EVENTFD_DISABLED: u32 = 1;

// ---------------------------------------------------------------------------
// io_uring_enter flags
// ---------------------------------------------------------------------------

pub const IORING_ENTER_GETEVENTS: u32 = 1;
pub const IORING_ENTER_SQ_WAKEUP: u32 = 2;
pub const IORING_ENTER_SQ_WAIT: u32 = 4;
pub const IORING_ENTER_EXT_ARG: u32 = 8;
pub const IORING_ENTER_REGISTERED_RING: u32 = 16;
pub const IORING_ENTER_ABS_TIMER: u32 = 32;
pub const IORING_ENTER_EXT_ARG_REG: u32 = 64;
pub const IORING_ENTER_NO_IOWAIT: u32 = 128;

// ---------------------------------------------------------------------------
// Feature flags (returned in io_uring_params.features)
// ---------------------------------------------------------------------------

pub const IORING_FEAT_SINGLE_MMAP: u32 = 1;
pub const IORING_FEAT_NODROP: u32 = 2;
pub const IORING_FEAT_SUBMIT_STABLE: u32 = 4;
pub const IORING_FEAT_RW_CUR_POS: u32 = 8;
pub const IORING_FEAT_CUR_PERSONALITY: u32 = 16;
pub const IORING_FEAT_FAST_POLL: u32 = 32;
pub const IORING_FEAT_POLL_32BITS: u32 = 64;
pub const IORING_FEAT_SQPOLL_NONFIXED: u32 = 128;
pub const IORING_FEAT_EXT_ARG: u32 = 256;
pub const IORING_FEAT_NATIVE_WORKERS: u32 = 512;
pub const IORING_FEAT_RSRC_TAGS: u32 = 1024;
pub const IORING_FEAT_CQE_SKIP: u32 = 2048;
pub const IORING_FEAT_LINKED_FILE: u32 = 4096;
pub const IORING_FEAT_REG_REG_RING: u32 = 8192;
pub const IORING_FEAT_RECVSEND_BUNDLE: u32 = 16384;
pub const IORING_FEAT_MIN_TIMEOUT: u32 = 32768;
pub const IORING_FEAT_RW_ATTR: u32 = 65536;
pub const IORING_FEAT_NO_IOWAIT: u32 = 131072;

// ---------------------------------------------------------------------------
// Register opcodes
// ---------------------------------------------------------------------------

pub type io_uring_register_op = u32;

pub const IORING_REGISTER_BUFFERS: io_uring_register_op = 0;
pub const IORING_UNREGISTER_BUFFERS: io_uring_register_op = 1;
pub const IORING_REGISTER_FILES: io_uring_register_op = 2;
pub const IORING_UNREGISTER_FILES: io_uring_register_op = 3;
pub const IORING_REGISTER_EVENTFD: io_uring_register_op = 4;
pub const IORING_UNREGISTER_EVENTFD: io_uring_register_op = 5;
pub const IORING_REGISTER_FILES_UPDATE: io_uring_register_op = 6;
pub const IORING_REGISTER_EVENTFD_ASYNC: io_uring_register_op = 7;
pub const IORING_REGISTER_PROBE: io_uring_register_op = 8;
pub const IORING_REGISTER_PERSONALITY: io_uring_register_op = 9;
pub const IORING_UNREGISTER_PERSONALITY: io_uring_register_op = 10;
pub const IORING_REGISTER_RESTRICTIONS: io_uring_register_op = 11;
pub const IORING_REGISTER_ENABLE_RINGS: io_uring_register_op = 12;
pub const IORING_REGISTER_FILES2: io_uring_register_op = 13;
pub const IORING_REGISTER_FILES_UPDATE2: io_uring_register_op = 14;
pub const IORING_REGISTER_BUFFERS2: io_uring_register_op = 15;
pub const IORING_REGISTER_BUFFERS_UPDATE: io_uring_register_op = 16;
pub const IORING_REGISTER_IOWQ_AFF: io_uring_register_op = 17;
pub const IORING_UNREGISTER_IOWQ_AFF: io_uring_register_op = 18;
pub const IORING_REGISTER_IOWQ_MAX_WORKERS: io_uring_register_op = 19;
pub const IORING_REGISTER_RING_FDS: io_uring_register_op = 20;
pub const IORING_UNREGISTER_RING_FDS: io_uring_register_op = 21;
pub const IORING_REGISTER_PBUF_RING: io_uring_register_op = 22;
pub const IORING_UNREGISTER_PBUF_RING: io_uring_register_op = 23;
pub const IORING_REGISTER_SYNC_CANCEL: io_uring_register_op = 24;
pub const IORING_REGISTER_FILE_ALLOC_RANGE: io_uring_register_op = 25;
pub const IORING_REGISTER_PBUF_STATUS: io_uring_register_op = 26;
pub const IORING_REGISTER_NAPI: io_uring_register_op = 27;
pub const IORING_UNREGISTER_NAPI: io_uring_register_op = 28;
pub const IORING_REGISTER_CLOCK: io_uring_register_op = 29;
pub const IORING_REGISTER_CLONE_BUFFERS: io_uring_register_op = 30;
pub const IORING_REGISTER_SEND_MSG_RING: io_uring_register_op = 31;
pub const IORING_REGISTER_ZCRX_IFQ: io_uring_register_op = 32;
pub const IORING_REGISTER_RESIZE_RINGS: io_uring_register_op = 33;
pub const IORING_REGISTER_MEM_REGION: io_uring_register_op = 34;
pub const IORING_REGISTER_LAST: io_uring_register_op = 35;
pub const IORING_REGISTER_USE_REGISTERED_RING: io_uring_register_op = 2147483648;

pub const IORING_RSRC_REGISTER_SPARSE: u32 = 1;
pub const IORING_REGISTER_FILES_SKIP: i32 = -2;
pub const IO_URING_OP_SUPPORTED: u32 = 1;

pub const IORING_FILE_INDEX_ALLOC: i32 = -1;

// ---------------------------------------------------------------------------
// Provided buffer ring
// ---------------------------------------------------------------------------

pub const IOU_PBUF_RING_INC: u32 = 1;
pub const IOU_PBUF_RING_MMAP: u32 = 2;
pub const IORING_MEM_REGION_TYPE_USER: u32 = 1;
pub const IORING_ZCRX_AREA_SHIFT: u32 = 48;

// ---------------------------------------------------------------------------
// MSG_RING commands
// ---------------------------------------------------------------------------

pub const IORING_MSG_DATA: u32 = 0;
pub const IORING_MSG_SEND_FD: u32 = 1;

// ---------------------------------------------------------------------------
// Core structures
// ---------------------------------------------------------------------------

/// Submission queue entry — 64 bytes, matching kernel layout exactly.
///
/// Union fields are flattened: the opcode-specific field names are documented in comments.
/// Access the appropriate field based on the opcode being constructed.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct io_uring_sqe {
    pub opcode: u8,
    pub flags: u8,
    pub ioprio: u16,
    pub fd: i32,
    /// Union: `off` | `addr2` | `{ cmd_op, __pad1 }`
    pub off: u64,
    /// Union: `addr` | `splice_off_in` | `{ level, optname }`
    pub addr: u64,
    pub len: u32,
    /// Union: `rw_flags` | `fsync_flags` | `poll_events` | `poll32_events` |
    /// `sync_range_flags` | `msg_flags` | `timeout_flags` | `accept_flags` |
    /// `cancel_flags` | `open_flags` | `statx_flags` | `fadvise_advice` |
    /// `splice_flags` | `rename_flags` | `unlink_flags` | `hardlink_flags` |
    /// `xattr_flags` | `msg_ring_flags` | `uring_cmd_flags` | `waitid_flags` |
    /// `futex_flags` | `install_fd_flags` | `nop_flags`
    pub op_flags: u32,
    pub user_data: u64,
    /// Union: `buf_index` | `buf_group`
    pub buf_index: u16,
    pub personality: u16,
    /// Union: `splice_fd_in` | `file_index` | `optlen` | `addr_len`
    pub splice_fd_in: i32,
    /// Union: `addr3` | `optval` | `cmd[0]`
    pub addr3: u64,
    pub __pad2: [u64; 1],
}

impl Default for io_uring_sqe {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

impl core::fmt::Debug for io_uring_sqe {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("io_uring_sqe")
            .field("opcode", &self.opcode)
            .field("flags", &self.flags)
            .field("fd", &self.fd)
            .field("user_data", &self.user_data)
            .finish_non_exhaustive()
    }
}

/// Completion queue entry — 16 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct io_uring_cqe {
    pub user_data: u64,
    pub res: i32,
    pub flags: u32,
}

impl Default for io_uring_cqe {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

impl core::fmt::Debug for io_uring_cqe {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("io_uring_cqe")
            .field("user_data", &self.user_data)
            .field("res", &self.res)
            .field("flags", &self.flags)
            .finish()
    }
}

/// SQ ring offset table.
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct io_sqring_offsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub flags: u32,
    pub dropped: u32,
    pub array: u32,
    pub resv1: u32,
    pub user_addr: u64,
}

/// CQ ring offset table.
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct io_cqring_offsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub overflow: u32,
    pub cqes: u32,
    pub flags: u32,
    pub resv1: u32,
    pub user_addr: u64,
}

/// Parameters for io_uring setup.
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct io_uring_params {
    pub sq_entries: u32,
    pub cq_entries: u32,
    pub flags: u32,
    pub sq_thread_cpu: u32,
    pub sq_thread_idle: u32,
    pub features: u32,
    pub wq_fd: u32,
    pub resv: [u32; 3],
    pub sq_off: io_sqring_offsets,
    pub cq_off: io_cqring_offsets,
}

/// Kernel timespec.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct __kernel_timespec {
    pub tv_sec: __kernel_time64_t,
    pub tv_nsec: i64,
}

/// openat2 how structure.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct open_how {
    pub flags: u64,
    pub mode: u64,
    pub resolve: u64,
}

/// io_uring_getevents_arg for extended enter.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_getevents_arg {
    pub sigmask: u64,
    pub sigmask_sz: u32,
    pub min_wait_usec: u32,
    pub ts: u64,
}

/// Provided buffer entry.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_buf {
    pub addr: u64,
    pub len: u32,
    pub bid: u16,
    pub resv: u16,
}

/// Buffer ring registration.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_buf_reg {
    pub ring_addr: u64,
    pub ring_entries: u32,
    pub bgid: u16,
    pub flags: u16,
    pub resv: [u64; 3],
}

/// Resource registration.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_rsrc_register {
    pub nr: u32,
    pub flags: u32,
    pub resv2: u64,
    pub data: u64,
    pub tags: u64,
}

/// Resource update (v2).
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_rsrc_update2 {
    pub offset: u32,
    pub resv: u32,
    pub data: u64,
    pub tags: u64,
    pub nr: u32,
    pub resv2: u32,
}

/// Files update.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_files_update {
    pub offset: u32,
    pub resv: u32,
    pub fds: u64,
}

/// Synchronous cancel registration.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_sync_cancel_reg {
    pub addr: u64,
    pub fd: i32,
    pub flags: u32,
    pub timeout: __kernel_timespec,
    pub opcode: u8,
    pub pad: [u8; 7],
    pub pad2: [u64; 3],
}

/// Recvmsg out header (for multishot recvmsg).
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_recvmsg_out {
    pub namelen: u32,
    pub controllen: u32,
    pub payloadlen: u32,
    pub flags: u32,
}

/// futex_waitv entry.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct futex_waitv {
    pub val: u64,
    pub uaddr: u64,
    pub flags: u32,
    pub __reserved: u32,
}

/// Probe entry.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_probe_op {
    pub op: u8,
    pub resv: u8,
    pub flags: u16,
    pub resv2: u32,
}

/// Probe header.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_probe {
    pub last_op: u8,
    pub ops_len: u8,
    pub resv: u16,
    pub resv2: [u32; 3],
    // followed by io_uring_probe_op[] (variable-length)
}

/// Restriction entry.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_restriction {
    pub opcode: u16,
    pub register_op: u8,
    pub sqe_op: u8,
    pub sqe_flags: u8,
    pub resv: u8,
    pub resv2: [u32; 3],
}

/// Memory region descriptor (for IORING_REGISTER_MEM_REGION).
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_region_desc {
    pub user_addr: u64,
    pub size: u64,
    pub flags: u32,
    pub id: u32,
    pub mmap_offset: u64,
    pub __resv: [u64; 4],
}

/// Zero-copy RX interface queue registration.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_zcrx_ifq_reg {
    pub if_idx: u32,
    pub if_rxq: u32,
    pub rq_entries: u32,
    pub flags: u32,
    pub area_id: u32,
    pub resv: [u32; 3],
    pub rq_off: io_uring_zcrx_offsets,
    pub __resv: [u64; 4],
}

/// Zero-copy RX offsets.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_zcrx_offsets {
    pub head: u32,
    pub tail: u32,
    pub rqes: u32,
    pub mmap_sz: u32,
    pub __resv: [u64; 2],
}

/// Zero-copy RX area registration.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_zcrx_area_reg {
    pub addr: u64,
    pub len: u64,
    pub rq_area_token: u64,
    pub flags: u32,
    pub area_id: u32,
    pub __resv: [u64; 2],
}

/// Zero-copy RX CQE.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_zcrx_cqe {
    pub off: u64,
    pub __pad: u64,
}

/// Zero-copy RX RQE.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct io_uring_zcrx_rqe {
    pub off: u64,
    pub len: u32,
    pub __pad: u32,
}

// ---------------------------------------------------------------------------
// Compile-time layout checks
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    #[test]
    fn sqe_layout() {
        assert_eq!(mem::size_of::<io_uring_sqe>(), 64);
        assert_eq!(mem::align_of::<io_uring_sqe>(), 8);
    }

    #[test]
    fn cqe_layout() {
        assert_eq!(mem::size_of::<io_uring_cqe>(), 16);
        assert_eq!(mem::align_of::<io_uring_cqe>(), 8);
    }

    #[test]
    fn params_layout() {
        assert_eq!(mem::size_of::<io_uring_params>(), 120);
    }

    #[test]
    fn timespec_layout() {
        assert_eq!(mem::size_of::<__kernel_timespec>(), 16);
    }

    #[test]
    fn open_how_layout() {
        assert_eq!(mem::size_of::<open_how>(), 24);
    }
}
