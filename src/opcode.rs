//! Operation codes that can be used to construct [`squeue::Entry`](crate::squeue::Entry)s.

#![allow(clippy::new_without_default)]

use std::os::fd::RawFd;

use crate::squeue::{Entry, Entry128};
use crate::sys;
use crate::types::{self, sealed};

macro_rules! assign_fd {
    ( $sqe:ident . fd = $opfd:expr ) => {
        match $opfd {
            sealed::Target::Fd(fd) => $sqe.fd = fd,
            sealed::Target::Fixed(idx) => {
                $sqe.fd = idx as _;
                $sqe.flags |= crate::squeue::Flags::FIXED_FILE.bits();
            }
        }
    };
}

macro_rules! opcode {
    (@type impl sealed::UseFixed ) => {
        sealed::Target
    };
    (@type impl sealed::UseFd ) => {
        i32
    };
    (@type $name:ty ) => {
        $name
    };
    (
        $( #[$outer:meta] )*
        pub struct $name:ident {
            $( #[$new_meta:meta] )*

            $( $field:ident : { $( $tnt:tt )+ } ),*

            $(,)?

            ;;

            $(
                $( #[$opt_meta:meta] )*
                $opt_field:ident : $opt_tname:ty = $default:expr
            ),*

            $(,)?
        }

        pub const CODE = $opcode:expr;

        $( #[$build_meta:meta] )*
        pub fn build($self:ident) -> $entry:ty $build_block:block
    ) => {
        $( #[$outer] )*
        pub struct $name {
            $( $field : opcode!(@type $( $tnt )*), )*
            $( $opt_field : $opt_tname, )*
        }

        impl $name {
            $( #[$new_meta] )*
            #[inline]
            pub fn new($( $field : $( $tnt )* ),*) -> Self {
                $name {
                    $( $field: $field.into(), )*
                    $( $opt_field: $default, )*
                }
            }

            pub const CODE: u8 = $opcode as _;

            $(
                $( #[$opt_meta] )*
                #[inline]
                pub const fn $opt_field(mut self, $opt_field: $opt_tname) -> Self {
                    self.$opt_field = $opt_field;
                    self
                }
            )*

            $( #[$build_meta] )*
            #[inline]
            pub fn build($self) -> $entry $build_block
        }
    }
}

#[inline(always)]
fn sqe_zeroed() -> sys::io_uring_sqe {
    unsafe { core::mem::zeroed() }
}

// ---------------------------------------------------------------------------
// Opcodes
// ---------------------------------------------------------------------------

opcode! {
    /// Do not perform any I/O.
    #[derive(Debug)]
    pub struct Nop {
        ;;
        nop_flags: u32 = 0,
        nop_result: u32 = 0,
    }

    pub const CODE = sys::IORING_OP_NOP;

    pub fn build(self) -> Entry {
        let Nop { nop_flags, nop_result } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = -1;
        sqe.op_flags = nop_flags;
        sqe.len = nop_result;
        Entry(sqe)
    }
}

impl Nop {
    /// Set the NOP to inject the given value as the operation result.
    ///
    /// When this is set, the completion will report `result` as `i32` instead of `0`.
    /// Requires kernel 6.3+ (emulated unconditionally in miring).
    #[inline]
    pub fn inject_result(self, result: u32) -> Self {
        self.nop_flags(sys::IORING_NOP_INJECT_RESULT)
            .nop_result(result)
    }
}

opcode! {
    /// Vectored read, equivalent to `preadv2(2)`.
    #[derive(Debug)]
    pub struct Readv {
        fd: { impl sealed::UseFixed },
        iovec: { *const libc_iovec },
        len: { u32 },
        ;;
        ioprio: u16 = 0,
        offset: u64 = 0,
        rw_flags: i32 = 0,
        buf_group: u16 = 0
    }

    pub const CODE = sys::IORING_OP_READV;

    pub fn build(self) -> Entry {
        let Readv { fd, iovec, len, offset, ioprio, rw_flags, buf_group } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = iovec as u64;
        sqe.len = len;
        sqe.off = offset;
        sqe.op_flags = rw_flags as u32;
        sqe.buf_index = buf_group;
        Entry(sqe)
    }
}

opcode! {
    /// Vectored write, equivalent to `pwritev2(2)`.
    #[derive(Debug)]
    pub struct Writev {
        fd: { impl sealed::UseFixed },
        iovec: { *const libc_iovec },
        len: { u32 },
        ;;
        ioprio: u16 = 0,
        offset: u64 = 0,
        rw_flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_WRITEV;

    pub fn build(self) -> Entry {
        let Writev { fd, iovec, len, offset, ioprio, rw_flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = iovec as u64;
        sqe.len = len;
        sqe.off = offset;
        sqe.op_flags = rw_flags as u32;
        Entry(sqe)
    }
}

opcode! {
    /// File sync, equivalent to `fsync(2)`.
    #[derive(Debug)]
    pub struct Fsync {
        fd: { impl sealed::UseFixed },
        ;;
        flags: types::FsyncFlags = types::FsyncFlags::empty()
    }

    pub const CODE = sys::IORING_OP_FSYNC;

    pub fn build(self) -> Entry {
        let Fsync { fd, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.op_flags = flags.bits();
        Entry(sqe)
    }
}

opcode! {
    /// Read from a file into a registered fixed buffer.
    #[derive(Debug)]
    pub struct ReadFixed {
        fd: { impl sealed::UseFixed },
        buf: { *mut u8 },
        len: { u32 },
        buf_index: { u16 },
        ;;
        ioprio: u16 = 0,
        offset: u64 = 0,
        rw_flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_READ_FIXED;

    pub fn build(self) -> Entry {
        let ReadFixed { fd, buf, len, offset, buf_index, ioprio, rw_flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = buf as u64;
        sqe.len = len;
        sqe.off = offset;
        sqe.op_flags = rw_flags as u32;
        sqe.buf_index = buf_index;
        Entry(sqe)
    }
}

opcode! {
    /// Write to a file from a registered fixed buffer.
    #[derive(Debug)]
    pub struct WriteFixed {
        fd: { impl sealed::UseFixed },
        buf: { *const u8 },
        len: { u32 },
        buf_index: { u16 },
        ;;
        ioprio: u16 = 0,
        offset: u64 = 0,
        rw_flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_WRITE_FIXED;

    pub fn build(self) -> Entry {
        let WriteFixed { fd, buf, len, offset, buf_index, ioprio, rw_flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = buf as u64;
        sqe.len = len;
        sqe.off = offset;
        sqe.op_flags = rw_flags as u32;
        sqe.buf_index = buf_index;
        Entry(sqe)
    }
}

opcode! {
    /// Poll for events on a file descriptor.
    #[derive(Debug)]
    pub struct PollAdd {
        fd: { impl sealed::UseFixed },
        flags: { u32 },
        ;;
        multi: bool = false
    }

    pub const CODE = sys::IORING_OP_POLL_ADD;

    pub fn build(self) -> Entry {
        let PollAdd { fd, flags, multi } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.op_flags = flags;
        if multi {
            sqe.len = sys::IORING_POLL_ADD_MULTI;
        }
        Entry(sqe)
    }
}

opcode! {
    /// Remove a previously added poll request.
    #[derive(Debug)]
    pub struct PollRemove {
        user_data: { u64 },
        ;;
    }

    pub const CODE = sys::IORING_OP_POLL_REMOVE;

    pub fn build(self) -> Entry {
        let PollRemove { user_data } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = -1;
        sqe.addr = user_data;
        Entry(sqe)
    }
}

opcode! {
    /// Sync a file range, equivalent to `sync_file_range(2)`.
    #[derive(Debug)]
    pub struct SyncFileRange {
        fd: { impl sealed::UseFixed },
        len: { u32 },
        ;;
        offset: u64 = 0,
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_SYNC_FILE_RANGE;

    pub fn build(self) -> Entry {
        let SyncFileRange { fd, len, offset, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.len = len;
        sqe.off = offset;
        sqe.op_flags = flags;
        Entry(sqe)
    }
}

opcode! {
    /// Send a message on a socket, equivalent to `sendmsg(2)`.
    #[derive(Debug)]
    pub struct SendMsg {
        fd: { impl sealed::UseFixed },
        msg: { *const std::ffi::c_void },
        ;;
        ioprio: u16 = 0,
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_SENDMSG;

    pub fn build(self) -> Entry {
        let SendMsg { fd, msg, ioprio, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = msg as u64;
        sqe.op_flags = flags;
        sqe.len = 1;
        Entry(sqe)
    }
}

opcode! {
    /// Receive a message on a socket, equivalent to `recvmsg(2)`.
    #[derive(Debug)]
    pub struct RecvMsg {
        fd: { impl sealed::UseFixed },
        msg: { *mut std::ffi::c_void },
        ;;
        ioprio: u16 = 0,
        flags: u32 = 0,
        buf_group: u16 = 0
    }

    pub const CODE = sys::IORING_OP_RECVMSG;

    pub fn build(self) -> Entry {
        let RecvMsg { fd, msg, ioprio, flags, buf_group } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = msg as u64;
        sqe.op_flags = flags;
        sqe.len = 1;
        sqe.buf_index = buf_group;
        Entry(sqe)
    }
}

opcode! {
    /// Register a timeout operation.
    #[derive(Debug)]
    pub struct Timeout {
        timespec: { *const types::Timespec },
        ;;
        count: u32 = 0,
        flags: types::TimeoutFlags = types::TimeoutFlags::empty()
    }

    pub const CODE = sys::IORING_OP_TIMEOUT;

    pub fn build(self) -> Entry {
        let Timeout { timespec, count, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = -1;
        sqe.addr = timespec as u64;
        sqe.len = count;
        sqe.op_flags = flags.bits();
        sqe.off = 1; // count
        Entry(sqe)
    }
}

opcode! {
    /// Remove a previously registered timeout.
    #[derive(Debug)]
    pub struct TimeoutRemove {
        user_data: { u64 },
        ;;
        flags: types::TimeoutFlags = types::TimeoutFlags::empty()
    }

    pub const CODE = sys::IORING_OP_TIMEOUT_REMOVE;

    pub fn build(self) -> Entry {
        let TimeoutRemove { user_data, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = -1;
        sqe.addr = user_data;
        sqe.op_flags = flags.bits();
        Entry(sqe)
    }
}

opcode! {
    /// Accept a new connection on a socket, equivalent to `accept4(2)`.
    #[derive(Debug)]
    pub struct Accept {
        fd: { impl sealed::UseFixed },
        ;;
        addr: *mut std::ffi::c_void = std::ptr::null_mut(),
        addrlen: *mut u32 = std::ptr::null_mut(),
        flags: u32 = 0,
        file_index: Option<u32> = None
    }

    pub const CODE = sys::IORING_OP_ACCEPT;

    pub fn build(self) -> Entry {
        let Accept { fd, addr, addrlen, flags, file_index } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = addr as u64;
        sqe.off = addrlen as u64;
        sqe.op_flags = flags;
        if let Some(idx) = file_index {
            sqe.splice_fd_in = idx as i32 + 1;
        }
        Entry(sqe)
    }
}

opcode! {
    /// Cancel an in-flight async operation.
    #[derive(Debug)]
    pub struct AsyncCancel {
        user_data: { u64 },
        ;;
    }

    pub const CODE = sys::IORING_OP_ASYNC_CANCEL;

    pub fn build(self) -> Entry {
        let AsyncCancel { user_data } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = -1;
        sqe.addr = user_data;
        Entry(sqe)
    }
}

opcode! {
    /// Link timeout — timeout for linked operations.
    #[derive(Debug)]
    pub struct LinkTimeout {
        timespec: { *const types::Timespec },
        ;;
        flags: types::TimeoutFlags = types::TimeoutFlags::empty()
    }

    pub const CODE = sys::IORING_OP_LINK_TIMEOUT;

    pub fn build(self) -> Entry {
        let LinkTimeout { timespec, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = -1;
        sqe.addr = timespec as u64;
        sqe.op_flags = flags.bits();
        Entry(sqe)
    }
}

opcode! {
    /// Connect a socket, equivalent to `connect(2)`.
    #[derive(Debug)]
    pub struct Connect {
        fd: { impl sealed::UseFixed },
        addr: { *const std::ffi::c_void },
        addrlen: { u32 },
        ;;
    }

    pub const CODE = sys::IORING_OP_CONNECT;

    pub fn build(self) -> Entry {
        let Connect { fd, addr, addrlen } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = addr as u64;
        sqe.off = addrlen as u64;
        Entry(sqe)
    }
}

opcode! {
    /// Pre-allocate or deallocate space to a file, equivalent to `fallocate(2)`.
    #[derive(Debug)]
    pub struct Fallocate {
        fd: { impl sealed::UseFixed },
        len: { u64 },
        ;;
        offset: u64 = 0,
        mode: i32 = 0
    }

    pub const CODE = sys::IORING_OP_FALLOCATE;

    pub fn build(self) -> Entry {
        let Fallocate { fd, len, offset, mode } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = len;
        sqe.off = offset;
        sqe.len = mode as u32;
        Entry(sqe)
    }
}

opcode! {
    /// Open a file, equivalent to `openat(2)`.
    #[derive(Debug)]
    pub struct OpenAt {
        dirfd: { impl sealed::UseFixed },
        pathname: { *const u8 },
        ;;
        flags: i32 = 0,
        mode: u32 = 0,
        file_index: Option<u32> = None
    }

    pub const CODE = sys::IORING_OP_OPENAT;

    pub fn build(self) -> Entry {
        let OpenAt { dirfd, pathname, flags, mode, file_index } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = dirfd);
        sqe.addr = pathname as u64;
        sqe.op_flags = flags as u32;
        sqe.len = mode;
        if let Some(idx) = file_index {
            sqe.splice_fd_in = idx as i32 + 1;
        }
        Entry(sqe)
    }
}

opcode! {
    /// Close a file descriptor, equivalent to `close(2)`.
    #[derive(Debug)]
    pub struct Close {
        fd: { impl sealed::UseFixed },
        ;;
    }

    pub const CODE = sys::IORING_OP_CLOSE;

    pub fn build(self) -> Entry {
        let Close { fd } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        Entry(sqe)
    }
}

opcode! {
    /// Update the registered file set.
    #[derive(Debug)]
    pub struct FilesUpdate {
        fds: { *const i32 },
        len: { u32 },
        ;;
        offset: i32 = 0
    }

    pub const CODE = sys::IORING_OP_FILES_UPDATE;

    pub fn build(self) -> Entry {
        let FilesUpdate { fds, len, offset } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = -1;
        sqe.addr = fds as u64;
        sqe.len = len;
        sqe.off = offset as u64;
        Entry(sqe)
    }
}

opcode! {
    /// Get file status, equivalent to `statx(2)`.
    #[derive(Debug)]
    pub struct Statx {
        dirfd: { impl sealed::UseFixed },
        pathname: { *const u8 },
        statxbuf: { *mut types::statx },
        ;;
        flags: i32 = 0,
        mask: u32 = 0
    }

    pub const CODE = sys::IORING_OP_STATX;

    pub fn build(self) -> Entry {
        let Statx { dirfd, pathname, statxbuf, flags, mask } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = dirfd);
        sqe.addr = pathname as u64;
        sqe.off = statxbuf as u64;
        sqe.len = mask;
        sqe.op_flags = flags as u32;
        Entry(sqe)
    }
}

opcode! {
    /// Read from a file, equivalent to `pread(2)`.
    #[derive(Debug)]
    pub struct Read {
        fd: { impl sealed::UseFixed },
        buf: { *mut u8 },
        len: { u32 },
        ;;
        ioprio: u16 = 0,
        offset: u64 = 0,
        rw_flags: i32 = 0,
        buf_group: u16 = 0
    }

    pub const CODE = sys::IORING_OP_READ;

    pub fn build(self) -> Entry {
        let Read { fd, buf, len, offset, ioprio, rw_flags, buf_group } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = buf as u64;
        sqe.len = len;
        sqe.off = offset;
        sqe.op_flags = rw_flags as u32;
        sqe.buf_index = buf_group;
        Entry(sqe)
    }
}

opcode! {
    /// Write to a file, equivalent to `pwrite(2)`.
    #[derive(Debug)]
    pub struct Write {
        fd: { impl sealed::UseFixed },
        buf: { *const u8 },
        len: { u32 },
        ;;
        ioprio: u16 = 0,
        offset: u64 = 0,
        rw_flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_WRITE;

    pub fn build(self) -> Entry {
        let Write { fd, buf, len, offset, ioprio, rw_flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = buf as u64;
        sqe.len = len;
        sqe.off = offset;
        sqe.op_flags = rw_flags as u32;
        Entry(sqe)
    }
}

opcode! {
    /// Send data on a socket, equivalent to `send(2)`.
    #[derive(Debug)]
    pub struct Send {
        fd: { impl sealed::UseFixed },
        buf: { *const u8 },
        len: { u32 },
        ;;
        flags: u32 = 0,
        dest_addr: *const std::ffi::c_void = std::ptr::null(),
        dest_addr_len: u16 = 0
    }

    pub const CODE = sys::IORING_OP_SEND;

    pub fn build(self) -> Entry {
        let Send { fd, buf, len, flags, dest_addr, dest_addr_len } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = buf as u64;
        sqe.len = len;
        sqe.op_flags = flags;
        if !dest_addr.is_null() {
            sqe.addr3 = dest_addr as u64;
            sqe.splice_fd_in = dest_addr_len as i32;
        }
        Entry(sqe)
    }
}

opcode! {
    /// Receive data from a socket, equivalent to `recv(2)`.
    #[derive(Debug)]
    pub struct Recv {
        fd: { impl sealed::UseFixed },
        buf: { *mut u8 },
        len: { u32 },
        ;;
        flags: u32 = 0,
        buf_group: u16 = 0
    }

    pub const CODE = sys::IORING_OP_RECV;

    pub fn build(self) -> Entry {
        let Recv { fd, buf, len, flags, buf_group } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = buf as u64;
        sqe.len = len;
        sqe.op_flags = flags;
        sqe.buf_index = buf_group;
        Entry(sqe)
    }
}

opcode! {
    /// Open a file, equivalent to `openat2(2)`.
    #[derive(Debug)]
    pub struct OpenAt2 {
        dirfd: { impl sealed::UseFixed },
        pathname: { *const u8 },
        how: { *const types::OpenHow },
        ;;
        file_index: Option<u32> = None
    }

    pub const CODE = sys::IORING_OP_OPENAT2;

    pub fn build(self) -> Entry {
        let OpenAt2 { dirfd, pathname, how, file_index } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = dirfd);
        sqe.addr = pathname as u64;
        sqe.off = how as u64;
        sqe.len = std::mem::size_of::<types::OpenHow>() as u32;
        if let Some(idx) = file_index {
            sqe.splice_fd_in = idx as i32 + 1;
        }
        Entry(sqe)
    }
}

opcode! {
    /// Modify an epoll file descriptor, equivalent to `epoll_ctl(2)`.
    #[derive(Debug)]
    pub struct EpollCtl {
        epfd: { impl sealed::UseFixed },
        fd: { i32 },
        op: { i32 },
        ev: { *const types::epoll_event },
        ;;
    }

    pub const CODE = sys::IORING_OP_EPOLL_CTL;

    pub fn build(self) -> Entry {
        let EpollCtl { epfd, fd, op, ev } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = epfd);
        sqe.off = fd as u64;
        sqe.len = op as u32;
        sqe.addr = ev as u64;
        Entry(sqe)
    }
}

opcode! {
    /// Splice data between two file descriptors, equivalent to `splice(2)`.
    #[derive(Debug)]
    pub struct Splice {
        fd_in: { impl sealed::UseFixed },
        off_in: { i64 },
        fd_out: { impl sealed::UseFixed },
        off_out: { i64 },
        len: { u32 },
        ;;
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_SPLICE;

    pub fn build(self) -> Entry {
        let Splice { fd_in, off_in, fd_out, off_out, len, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        match fd_out {
            sealed::Target::Fd(fd) => sqe.fd = fd,
            sealed::Target::Fixed(idx) => {
                sqe.fd = idx as _;
                sqe.flags |= crate::squeue::Flags::FIXED_FILE.bits();
            }
        }
        sqe.off = off_out as u64;
        sqe.addr = off_in as u64;
        sqe.len = len;
        sqe.op_flags = flags;
        match fd_in {
            sealed::Target::Fd(fd) => sqe.splice_fd_in = fd,
            sealed::Target::Fixed(idx) => {
                sqe.splice_fd_in = idx as i32;
                sqe.op_flags |= sys::SPLICE_F_FD_IN_FIXED;
            }
        }
        Entry(sqe)
    }
}

opcode! {
    /// Provide buffers to the kernel for buffer selection.
    #[derive(Debug)]
    pub struct ProvideBuffers {
        addr: { *mut u8 },
        len: { i32 },
        nbufs: { u16 },
        bgid: { u16 },
        bid: { u16 },
        ;;
    }

    pub const CODE = sys::IORING_OP_PROVIDE_BUFFERS;

    pub fn build(self) -> Entry {
        let ProvideBuffers { addr, len, nbufs, bgid, bid } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = nbufs as i32;
        sqe.addr = addr as u64;
        sqe.len = len as u32;
        sqe.off = bid as u64;
        sqe.buf_index = bgid;
        Entry(sqe)
    }
}

opcode! {
    /// Remove provided buffers.
    #[derive(Debug)]
    pub struct RemoveBuffers {
        nbufs: { u16 },
        bgid: { u16 },
        ;;
    }

    pub const CODE = sys::IORING_OP_REMOVE_BUFFERS;

    pub fn build(self) -> Entry {
        let RemoveBuffers { nbufs, bgid } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = nbufs as i32;
        sqe.buf_index = bgid;
        Entry(sqe)
    }
}

opcode! {
    /// Duplicate pipe content, equivalent to `tee(2)`.
    #[derive(Debug)]
    pub struct Tee {
        fd_in: { impl sealed::UseFixed },
        fd_out: { impl sealed::UseFixed },
        len: { u32 },
        ;;
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_TEE;

    pub fn build(self) -> Entry {
        let Tee { fd_in, fd_out, len, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        match fd_out {
            sealed::Target::Fd(fd) => sqe.fd = fd,
            sealed::Target::Fixed(idx) => {
                sqe.fd = idx as _;
                sqe.flags |= crate::squeue::Flags::FIXED_FILE.bits();
            }
        }
        sqe.len = len;
        sqe.op_flags = flags;
        match fd_in {
            sealed::Target::Fd(fd) => sqe.splice_fd_in = fd,
            sealed::Target::Fixed(idx) => {
                sqe.splice_fd_in = idx as i32;
                sqe.op_flags |= sys::SPLICE_F_FD_IN_FIXED;
            }
        }
        Entry(sqe)
    }
}

opcode! {
    /// Shutdown a socket, equivalent to `shutdown(2)`.
    #[derive(Debug)]
    pub struct Shutdown {
        fd: { impl sealed::UseFixed },
        how: { i32 },
        ;;
    }

    pub const CODE = sys::IORING_OP_SHUTDOWN;

    pub fn build(self) -> Entry {
        let Shutdown { fd, how } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.len = how as u32;
        Entry(sqe)
    }
}

opcode! {
    /// Rename a file, equivalent to `renameat2(2)`.
    #[derive(Debug)]
    pub struct RenameAt {
        olddirfd: { impl sealed::UseFixed },
        oldpath: { *const u8 },
        newdirfd: { i32 },
        newpath: { *const u8 },
        ;;
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_RENAMEAT;

    pub fn build(self) -> Entry {
        let RenameAt { olddirfd, oldpath, newdirfd, newpath, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = olddirfd);
        sqe.addr = oldpath as u64;
        sqe.off = newpath as u64;
        sqe.len = newdirfd as u32;
        sqe.op_flags = flags;
        Entry(sqe)
    }
}

opcode! {
    /// Unlink a file, equivalent to `unlinkat(2)`.
    #[derive(Debug)]
    pub struct UnlinkAt {
        dirfd: { impl sealed::UseFixed },
        pathname: { *const u8 },
        ;;
        flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_UNLINKAT;

    pub fn build(self) -> Entry {
        let UnlinkAt { dirfd, pathname, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = dirfd);
        sqe.addr = pathname as u64;
        sqe.op_flags = flags as u32;
        Entry(sqe)
    }
}

opcode! {
    /// Make a directory, equivalent to `mkdirat(2)`.
    #[derive(Debug)]
    pub struct MkDirAt {
        dirfd: { impl sealed::UseFixed },
        pathname: { *const u8 },
        ;;
        mode: u32 = 0
    }

    pub const CODE = sys::IORING_OP_MKDIRAT;

    pub fn build(self) -> Entry {
        let MkDirAt { dirfd, pathname, mode } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = dirfd);
        sqe.addr = pathname as u64;
        sqe.len = mode;
        Entry(sqe)
    }
}

opcode! {
    /// Create a symlink, equivalent to `symlinkat(2)`.
    #[derive(Debug)]
    pub struct SymlinkAt {
        newdirfd: { impl sealed::UseFixed },
        target: { *const u8 },
        linkpath: { *const u8 },
        ;;
    }

    pub const CODE = sys::IORING_OP_SYMLINKAT;

    pub fn build(self) -> Entry {
        let SymlinkAt { newdirfd, target, linkpath } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = newdirfd);
        sqe.addr = target as u64;
        sqe.off = linkpath as u64;
        Entry(sqe)
    }
}

opcode! {
    /// Create a hard link, equivalent to `linkat(2)`.
    #[derive(Debug)]
    pub struct LinkAt {
        olddirfd: { impl sealed::UseFixed },
        oldpath: { *const u8 },
        newdirfd: { i32 },
        newpath: { *const u8 },
        ;;
        flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_LINKAT;

    pub fn build(self) -> Entry {
        let LinkAt { olddirfd, oldpath, newdirfd, newpath, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = olddirfd);
        sqe.addr = oldpath as u64;
        sqe.off = newpath as u64;
        sqe.len = newdirfd as u32;
        sqe.op_flags = flags as u32;
        Entry(sqe)
    }
}

opcode! {
    /// Send a message to another io_uring instance.
    #[derive(Debug)]
    pub struct MsgRing {
        fd: { impl sealed::UseFd },
        result: { i32 },
        user_data: { u64 },
        user_flags: { u32 },
        ;;
        opcode: u32 = sys::IORING_MSG_DATA,
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_MSG_RING;

    pub fn build(self) -> Entry {
        let MsgRing { fd, result, user_data, user_flags, opcode, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = fd;
        sqe.len = result as u32;
        sqe.off = user_data;
        sqe.op_flags = flags;
        sqe.splice_fd_in = opcode as i32;
        sqe.buf_index = user_flags as u16;
        Entry(sqe)
    }
}

opcode! {
    /// Send a fixed file descriptor to another io_uring instance.
    #[derive(Debug)]
    pub struct MsgRingSendFd {
        fd: { impl sealed::UseFd },
        src_fd: { impl sealed::UseFixed },
        dest_slot: { types::DestinationSlot },
        user_data: { u64 },
        ;;
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_MSG_RING;

    pub fn build(self) -> Entry {
        let MsgRingSendFd { fd, src_fd, dest_slot, user_data, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = fd;
        sqe.off = user_data;
        sqe.op_flags = flags;
        sqe.splice_fd_in = sys::IORING_MSG_SEND_FD as i32;
        match src_fd {
            sealed::Target::Fd(src) => sqe.addr = src as u64,
            sealed::Target::Fixed(idx) => {
                sqe.addr = idx as u64;
                sqe.flags |= crate::squeue::Flags::FIXED_FILE.bits();
            }
        }
        sqe.addr3 = dest_slot.kernel_index_arg() as u64;
        Entry(sqe)
    }
}

opcode! {
    /// Create a socket, equivalent to `socket(2)`.
    #[derive(Debug)]
    pub struct Socket {
        domain: { i32 },
        socket_type: { i32 },
        protocol: { i32 },
        ;;
        file_index: Option<u32> = None
    }

    pub const CODE = sys::IORING_OP_SOCKET;

    pub fn build(self) -> Entry {
        let Socket { domain, socket_type, protocol, file_index } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = domain;
        sqe.off = socket_type as u64;
        sqe.len = protocol as u32;
        if let Some(idx) = file_index {
            sqe.splice_fd_in = idx as i32 + 1;
        }
        Entry(sqe)
    }
}

opcode! {
    /// Send with zero-copy.
    #[derive(Debug)]
    pub struct SendZc {
        fd: { impl sealed::UseFixed },
        buf: { *const u8 },
        len: { u32 },
        ;;
        flags: u32 = 0,
        zc_flags: u16 = 0,
        buf_index: u16 = 0,
        dest_addr: *const std::ffi::c_void = std::ptr::null(),
        dest_addr_len: u16 = 0
    }

    pub const CODE = sys::IORING_OP_SEND_ZC;

    pub fn build(self) -> Entry {
        let SendZc { fd, buf, len, flags, zc_flags, buf_index, dest_addr, dest_addr_len } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = buf as u64;
        sqe.len = len;
        sqe.op_flags = flags;
        sqe.ioprio = zc_flags;
        sqe.buf_index = buf_index;
        if !dest_addr.is_null() {
            sqe.addr3 = dest_addr as u64;
            sqe.splice_fd_in = dest_addr_len as i32;
        }
        Entry(sqe)
    }
}

opcode! {
    /// Sendmsg with zero-copy.
    #[derive(Debug)]
    pub struct SendMsgZc {
        fd: { impl sealed::UseFixed },
        msg: { *const std::ffi::c_void },
        ;;
        ioprio: u16 = 0,
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_SENDMSG_ZC;

    pub fn build(self) -> Entry {
        let SendMsgZc { fd, msg, ioprio, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = msg as u64;
        sqe.op_flags = flags;
        sqe.len = 1;
        Entry(sqe)
    }
}

opcode! {
    /// Truncate a file, equivalent to `ftruncate(2)`.
    #[derive(Debug)]
    pub struct Ftruncate {
        fd: { impl sealed::UseFixed },
        len: { u64 },
        ;;
    }

    pub const CODE = sys::IORING_OP_FTRUNCATE;

    pub fn build(self) -> Entry {
        let Ftruncate { fd, len } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.off = len;
        Entry(sqe)
    }
}

opcode! {
    /// Bind a socket, equivalent to `bind(2)`.
    #[derive(Debug)]
    pub struct Bind {
        fd: { impl sealed::UseFixed },
        addr: { *const std::ffi::c_void },
        addrlen: { u32 },
        ;;
    }

    pub const CODE = sys::IORING_OP_BIND;

    pub fn build(self) -> Entry {
        let Bind { fd, addr, addrlen } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = addr as u64;
        sqe.off = addrlen as u64;
        Entry(sqe)
    }
}

opcode! {
    /// Listen on a socket, equivalent to `listen(2)`.
    #[derive(Debug)]
    pub struct Listen {
        fd: { impl sealed::UseFixed },
        backlog: { i32 },
        ;;
    }

    pub const CODE = sys::IORING_OP_LISTEN;

    pub fn build(self) -> Entry {
        let Listen { fd, backlog } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.len = backlog as u32;
        Entry(sqe)
    }
}

// Fadvise and Madvise

opcode! {
    /// Provide file access advice, equivalent to `posix_fadvise(2)`.
    #[derive(Debug)]
    pub struct Fadvise {
        fd: { impl sealed::UseFixed },
        len: { u32 },
        advice: { i32 },
        ;;
        offset: u64 = 0
    }

    pub const CODE = sys::IORING_OP_FADVISE;

    pub fn build(self) -> Entry {
        let Fadvise { fd, len, advice, offset } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.len = len;
        sqe.off = offset;
        sqe.op_flags = advice as u32;
        Entry(sqe)
    }
}

opcode! {
    /// Provide memory advice, equivalent to `madvise(2)`.
    #[derive(Debug)]
    pub struct Madvise {
        addr: { *const u8 },
        len: { u32 },
        advice: { i32 },
        ;;
    }

    pub const CODE = sys::IORING_OP_MADVISE;

    pub fn build(self) -> Entry {
        let Madvise { addr, len, advice } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = -1;
        sqe.addr = addr as u64;
        sqe.len = len;
        sqe.op_flags = advice as u32;
        Entry(sqe)
    }
}

// ---------------------------------------------------------------------------
// Multishot / convenience variants
// ---------------------------------------------------------------------------

opcode! {
    /// Multishot accept — repeatedly accept connections with a single SQE.
    ///
    /// Unlike [`Accept`], this does not return a sockaddr. The caller should use
    /// `getpeername` if the remote address is needed.
    #[derive(Debug)]
    pub struct AcceptMulti {
        fd: { impl sealed::UseFixed },
        ;;
        allocate_file_index: bool = false,
        flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_ACCEPT;

    pub fn build(self) -> Entry {
        let AcceptMulti { fd, allocate_file_index, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = sys::IORING_ACCEPT_MULTISHOT as u16;
        sqe.op_flags = flags as u32;
        if allocate_file_index {
            sqe.splice_fd_in = sys::IORING_FILE_INDEX_ALLOC;
        }
        Entry(sqe)
    }
}

opcode! {
    /// Multishot receive — repeatedly receive messages with buffer selection.
    #[derive(Debug)]
    pub struct RecvMulti {
        fd: { impl sealed::UseFixed },
        buf_group: { u16 },
        ;;
        flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_RECV;

    pub fn build(self) -> Entry {
        let RecvMulti { fd, buf_group, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.op_flags = flags as u32;
        sqe.buf_index = buf_group;
        sqe.flags |= crate::squeue::Flags::BUFFER_SELECT.bits();
        sqe.ioprio = sys::IORING_RECV_MULTISHOT as u16;
        Entry(sqe)
    }
}

opcode! {
    /// Multishot recvmsg — repeatedly receive messages with buffer ring selection.
    #[derive(Debug)]
    pub struct RecvMsgMulti {
        fd: { impl sealed::UseFixed },
        msg: { *const std::ffi::c_void },
        buf_group: { u16 },
        ;;
        ioprio: u16 = 0,
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_RECVMSG;

    pub fn build(self) -> Entry {
        let RecvMsgMulti { fd, msg, buf_group, ioprio, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = msg as u64;
        sqe.len = 1;
        sqe.op_flags = flags;
        sqe.buf_index = buf_group;
        sqe.flags |= crate::squeue::Flags::BUFFER_SELECT.bits();
        sqe.ioprio = ioprio | (sys::IORING_RECV_MULTISHOT as u16);
        Entry(sqe)
    }
}

opcode! {
    /// Receive a bundle of buffers from a socket.
    #[derive(Debug)]
    pub struct RecvBundle {
        fd: { impl sealed::UseFixed },
        buf_group: { u16 },
        ;;
        flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_RECV;

    pub fn build(self) -> Entry {
        let RecvBundle { fd, buf_group, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.op_flags = flags as u32;
        sqe.buf_index = buf_group;
        sqe.flags |= crate::squeue::Flags::BUFFER_SELECT.bits();
        sqe.ioprio |= sys::IORING_RECVSEND_BUNDLE as u16;
        Entry(sqe)
    }
}

opcode! {
    /// Multishot receive with bundle — repeatedly receive bundled buffers.
    #[derive(Debug)]
    pub struct RecvMultiBundle {
        fd: { impl sealed::UseFixed },
        buf_group: { u16 },
        ;;
        flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_RECV;

    pub fn build(self) -> Entry {
        let RecvMultiBundle { fd, buf_group, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.op_flags = flags as u32;
        sqe.buf_index = buf_group;
        sqe.flags |= crate::squeue::Flags::BUFFER_SELECT.bits();
        sqe.ioprio = sys::IORING_RECV_MULTISHOT as u16;
        sqe.ioprio |= sys::IORING_RECVSEND_BUNDLE as u16;
        Entry(sqe)
    }
}

opcode! {
    /// Send a bundle of buffers on a socket in a single request.
    #[derive(Debug)]
    pub struct SendBundle {
        fd: { impl sealed::UseFixed },
        buf_group: { u16 },
        ;;
        flags: i32 = 0,
        len: u32 = 0
    }

    pub const CODE = sys::IORING_OP_SEND;

    pub fn build(self) -> Entry {
        let SendBundle { fd, len, flags, buf_group } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.len = len;
        sqe.op_flags = flags as u32;
        sqe.ioprio |= sys::IORING_RECVSEND_BUNDLE as u16;
        sqe.flags |= crate::squeue::Flags::BUFFER_SELECT.bits();
        sqe.buf_index = buf_group;
        Entry(sqe)
    }
}

opcode! {
    /// Multishot read — buffer group selection for repeated reads.
    #[derive(Debug)]
    pub struct ReadMulti {
        fd: { impl sealed::UseFixed },
        len: { u32 },
        buf_group: { u16 },
        ;;
        offset: u64 = 0
    }

    pub const CODE = sys::IORING_OP_READ_MULTISHOT;

    pub fn build(self) -> Entry {
        let ReadMulti { fd, len, buf_group, offset } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.off = offset;
        sqe.len = len;
        sqe.buf_index = buf_group;
        sqe.flags = crate::squeue::Flags::BUFFER_SELECT.bits();
        Entry(sqe)
    }
}

// ---------------------------------------------------------------------------
// Xattr operations
// ---------------------------------------------------------------------------

opcode! {
    /// Get extended attribute, equivalent to `getxattr(2)`.
    #[derive(Debug)]
    pub struct GetXattr {
        name: { *const u8 },
        value: { *mut std::ffi::c_void },
        path: { *const u8 },
        len: { u32 },
        ;;
    }

    pub const CODE = sys::IORING_OP_GETXATTR;

    pub fn build(self) -> Entry {
        let GetXattr { name, value, path, len } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.addr = name as u64;
        sqe.len = len;
        sqe.off = value as u64;
        sqe.addr3 = path as u64;
        sqe.op_flags = 0;
        Entry(sqe)
    }
}

opcode! {
    /// Set extended attribute, equivalent to `setxattr(2)`.
    #[derive(Debug)]
    pub struct SetXattr {
        name: { *const u8 },
        value: { *const std::ffi::c_void },
        path: { *const u8 },
        len: { u32 },
        ;;
        flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_SETXATTR;

    pub fn build(self) -> Entry {
        let SetXattr { name, value, path, flags, len } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.addr = name as u64;
        sqe.len = len;
        sqe.off = value as u64;
        sqe.addr3 = path as u64;
        sqe.op_flags = flags as u32;
        Entry(sqe)
    }
}

opcode! {
    /// Get extended attribute on a file descriptor, equivalent to `fgetxattr(2)`.
    #[derive(Debug)]
    pub struct FGetXattr {
        fd: { impl sealed::UseFixed },
        name: { *const u8 },
        value: { *mut std::ffi::c_void },
        len: { u32 },
        ;;
    }

    pub const CODE = sys::IORING_OP_FGETXATTR;

    pub fn build(self) -> Entry {
        let FGetXattr { fd, name, value, len } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = name as u64;
        sqe.len = len;
        sqe.off = value as u64;
        sqe.op_flags = 0;
        Entry(sqe)
    }
}

opcode! {
    /// Set extended attribute on a file descriptor, equivalent to `fsetxattr(2)`.
    #[derive(Debug)]
    pub struct FSetXattr {
        fd: { impl sealed::UseFixed },
        name: { *const u8 },
        value: { *const std::ffi::c_void },
        len: { u32 },
        ;;
        flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_FSETXATTR;

    pub fn build(self) -> Entry {
        let FSetXattr { fd, name, value, flags, len } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = name as u64;
        sqe.len = len;
        sqe.off = value as u64;
        sqe.op_flags = flags as u32;
        Entry(sqe)
    }
}

// ---------------------------------------------------------------------------
// Cancel with builder
// ---------------------------------------------------------------------------

opcode! {
    /// Cancel an in-flight operation using a [`CancelBuilder`](types::CancelBuilder).
    #[derive(Debug)]
    pub struct AsyncCancel2 {
        builder: { types::CancelBuilder },
        ;;
    }

    pub const CODE = sys::IORING_OP_ASYNC_CANCEL;

    pub fn build(self) -> Entry {
        let AsyncCancel2 { builder } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = builder.to_fd();
        sqe.addr = builder.user_data.unwrap_or(0);
        sqe.op_flags = builder.flags.bits();
        Entry(sqe)
    }
}

// ---------------------------------------------------------------------------
// Timeout update
// ---------------------------------------------------------------------------

opcode! {
    /// Update an existing timeout operation.
    #[derive(Debug)]
    pub struct TimeoutUpdate {
        user_data: { u64 },
        timespec: { *const types::Timespec },
        ;;
        flags: types::TimeoutFlags = types::TimeoutFlags::empty()
    }

    pub const CODE = sys::IORING_OP_TIMEOUT_REMOVE;

    pub fn build(self) -> Entry {
        let TimeoutUpdate { user_data, timespec, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = -1;
        sqe.off = timespec as u64;
        sqe.addr = user_data;
        sqe.op_flags = flags.bits() | sys::IORING_TIMEOUT_UPDATE;
        Entry(sqe)
    }
}

// ---------------------------------------------------------------------------
// Uring commands
// ---------------------------------------------------------------------------

opcode! {
    /// A file/device-specific 16-byte command, akin to `ioctl(2)`.
    #[derive(Debug)]
    pub struct UringCmd16 {
        fd: { impl sealed::UseFixed },
        cmd_op: { u32 },
        ;;
        buf_index: Option<u16> = None,
        cmd: [u8; 16] = [0u8; 16]
    }

    pub const CODE = sys::IORING_OP_URING_CMD;

    pub fn build(self) -> Entry {
        let UringCmd16 { fd, cmd_op, cmd, buf_index } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        // cmd_op goes in the off field (first 4 bytes of the union)
        sqe.off = cmd_op as u64;
        // cmd[0..16] mapped to addr3 + __pad2
        sqe.addr3 = u64::from_le_bytes(cmd[0..8].try_into().unwrap());
        sqe.__pad2[0] = u64::from_le_bytes(cmd[8..16].try_into().unwrap());
        if let Some(buf_index) = buf_index {
            sqe.buf_index = buf_index;
            sqe.op_flags |= sys::IORING_URING_CMD_FIXED;
        }
        Entry(sqe)
    }
}

opcode! {
    /// A file/device-specific 80-byte command (needs [`Entry128`]).
    pub struct UringCmd80 {
        fd: { impl sealed::UseFixed },
        cmd_op: { u32 },
        ;;
        buf_index: Option<u16> = None,
        cmd: [u8; 80] = [0u8; 80]
    }

    pub const CODE = sys::IORING_OP_URING_CMD;

    pub fn build(self) -> Entry128 {
        let UringCmd80 { fd, cmd_op, cmd, buf_index } = self;
        let cmd1: [u8; 16] = cmd[..16].try_into().unwrap();
        let cmd2: [u8; 64] = cmd[16..].try_into().unwrap();

        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.off = cmd_op as u64;
        sqe.addr3 = u64::from_le_bytes(cmd1[0..8].try_into().unwrap());
        sqe.__pad2[0] = u64::from_le_bytes(cmd1[8..16].try_into().unwrap());
        if let Some(buf_index) = buf_index {
            sqe.buf_index = buf_index;
            sqe.op_flags |= sys::IORING_URING_CMD_FIXED;
        }
        Entry128(Entry(sqe), cmd2)
    }
}

opcode! {
    /// Set a socket option via `URING_CMD`.
    #[derive(Debug)]
    pub struct SetSockOpt {
        fd: { impl sealed::UseFixed },
        level: { u32 },
        optname: { u32 },
        optval: { *const std::ffi::c_void },
        optlen: { u32 },
        ;;
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_URING_CMD;

    pub fn build(self) -> Entry {
        let SetSockOpt { fd, level, optname, optval, optlen, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        // cmd_op = SOCKET_URING_OP_SETSOCKOPT
        sqe.off = sys::SOCKET_URING_OP_SETSOCKOPT as u64;
        // level + optname packed into addr (level in low 32, optname via separate field)
        sqe.addr = ((optname as u64) << 32) | (level as u64);
        sqe.op_flags = flags;
        sqe.splice_fd_in = optlen as i32;
        sqe.addr3 = optval as u64;
        Entry(sqe)
    }
}

// ---------------------------------------------------------------------------
// Futex operations
// ---------------------------------------------------------------------------

opcode! {
    /// Wait on a futex, similar to `futex(2)` `FUTEX_WAIT_BITSET`.
    #[derive(Debug)]
    pub struct FutexWait {
        futex: { *const u32 },
        val: { u64 },
        mask: { u64 },
        futex_flags: { u32 },
        ;;
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_FUTEX_WAIT;

    pub fn build(self) -> Entry {
        let FutexWait { futex, val, mask, futex_flags, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = futex_flags as i32;
        sqe.addr = futex as u64;
        sqe.off = val;
        sqe.addr3 = mask;
        sqe.op_flags = flags;
        Entry(sqe)
    }
}

opcode! {
    /// Wake waiters on a futex, similar to `futex(2)` `FUTEX_WAKE_BITSET`.
    #[derive(Debug)]
    pub struct FutexWake {
        futex: { *const u32 },
        val: { u64 },
        mask: { u64 },
        futex_flags: { u32 },
        ;;
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_FUTEX_WAKE;

    pub fn build(self) -> Entry {
        let FutexWake { futex, val, mask, futex_flags, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = futex_flags as i32;
        sqe.addr = futex as u64;
        sqe.off = val;
        sqe.addr3 = mask;
        sqe.op_flags = flags;
        Entry(sqe)
    }
}

opcode! {
    /// Wait on multiple futexes simultaneously.
    #[derive(Debug)]
    pub struct FutexWaitV {
        futexv: { *const types::FutexWaitV },
        nr_futex: { u32 },
        ;;
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_FUTEX_WAITV;

    pub fn build(self) -> Entry {
        let FutexWaitV { futexv, nr_futex, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.addr = futexv as u64;
        sqe.len = nr_futex;
        sqe.op_flags = flags;
        Entry(sqe)
    }
}

// ---------------------------------------------------------------------------
// Misc operations
// ---------------------------------------------------------------------------

opcode! {
    /// Issue the equivalent of a `waitid(2)` system call.
    #[derive(Debug)]
    pub struct WaitId {
        idtype: { u32 },
        id: { u32 },
        options: { i32 },
        ;;
        infop: *const std::ffi::c_void = std::ptr::null(),
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_WAITID;

    pub fn build(self) -> Entry {
        let WaitId { idtype, id, options, infop, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = id as i32;
        sqe.len = idtype;
        sqe.op_flags = flags;
        sqe.splice_fd_in = options;
        sqe.off = infop as u64;
        Entry(sqe)
    }
}

opcode! {
    /// Install a fixed file descriptor as a regular fd.
    #[derive(Debug)]
    pub struct FixedFdInstall {
        fd: { types::Fixed },
        file_flags: { u32 },
        ;;
    }

    pub const CODE = sys::IORING_OP_FIXED_FD_INSTALL;

    pub fn build(self) -> Entry {
        let FixedFdInstall { fd, file_flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = fd.0 as i32;
        sqe.flags = crate::squeue::Flags::FIXED_FILE.bits();
        sqe.op_flags = file_flags;
        Entry(sqe)
    }
}

opcode! {
    /// Zero-copy receive, equivalent to `recv(2)` with zero-copy semantics.
    #[derive(Debug)]
    pub struct RecvZc {
        fd: { impl sealed::UseFixed },
        len: { u32 },
        ;;
        ifq: u32 = 0,
        ioprio: u16 = 0
    }

    pub const CODE = sys::IORING_OP_RECV_ZC;

    pub fn build(self) -> Entry {
        let RecvZc { fd, len, ifq, ioprio } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.len = len;
        sqe.ioprio = ioprio | sys::IORING_RECV_MULTISHOT as u16;
        sqe.splice_fd_in = ifq as i32;
        Entry(sqe)
    }
}

opcode! {
    /// Wait for epoll events, equivalent to `epoll_wait(2)`.
    #[derive(Debug)]
    pub struct EpollWait {
        fd: { impl sealed::UseFixed },
        events: { *mut types::epoll_event },
        max_events: { u32 },
        ;;
        flags: u32 = 0
    }

    pub const CODE = sys::IORING_OP_EPOLL_WAIT;

    pub fn build(self) -> Entry {
        let EpollWait { fd, events, max_events, flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.addr = events as u64;
        sqe.len = max_events;
        sqe.op_flags = flags;
        Entry(sqe)
    }
}

opcode! {
    /// Vectored read into a fixed (pre-registered) buffer.
    #[derive(Debug)]
    pub struct ReadvFixed {
        fd: { impl sealed::UseFixed },
        iovec: { *const libc_iovec },
        len: { u32 },
        buf_index: { u16 },
        ;;
        ioprio: u16 = 0,
        offset: u64 = 0,
        rw_flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_READV_FIXED;

    pub fn build(self) -> Entry {
        let ReadvFixed { fd, iovec, len, buf_index, offset, ioprio, rw_flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.off = offset;
        sqe.addr = iovec as u64;
        sqe.len = len;
        sqe.buf_index = buf_index;
        sqe.ioprio = ioprio;
        sqe.op_flags = rw_flags as u32;
        Entry(sqe)
    }
}

opcode! {
    /// Vectored write from a fixed (pre-registered) buffer.
    #[derive(Debug)]
    pub struct WritevFixed {
        fd: { impl sealed::UseFixed },
        iovec: { *const libc_iovec },
        len: { u32 },
        buf_index: { u16 },
        ;;
        ioprio: u16 = 0,
        offset: u64 = 0,
        rw_flags: i32 = 0
    }

    pub const CODE = sys::IORING_OP_WRITEV_FIXED;

    pub fn build(self) -> Entry {
        let WritevFixed { fd, iovec, len, buf_index, offset, ioprio, rw_flags } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        assign_fd!(sqe.fd = fd);
        sqe.off = offset;
        sqe.addr = iovec as u64;
        sqe.len = len;
        sqe.buf_index = buf_index;
        sqe.ioprio = ioprio;
        sqe.op_flags = rw_flags as u32;
        Entry(sqe)
    }
}

opcode! {
    /// Create a pipe, equivalent to `pipe2(2)`.
    #[derive(Debug)]
    pub struct Pipe {
        fds: { *mut RawFd },
        ;;
        flags: u32 = 0,
        file_index: Option<types::DestinationSlot> = None
    }

    pub const CODE = sys::IORING_OP_PIPE;

    pub fn build(self) -> Entry {
        let Pipe { fds, flags, file_index } = self;
        let mut sqe = sqe_zeroed();
        sqe.opcode = Self::CODE;
        sqe.fd = 0;
        sqe.addr = fds as u64;
        sqe.op_flags = flags;
        if let Some(dest) = file_index {
            sqe.splice_fd_in = dest.kernel_index_arg() as i32;
        }
        Entry(sqe)
    }
}

// ---------------------------------------------------------------------------
// Iovec type for Miri compatibility (avoids libc dependency)
// ---------------------------------------------------------------------------

/// Iovec compatible with the kernel's struct iovec.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct libc_iovec {
    pub iov_base: *mut std::ffi::c_void,
    pub iov_len: usize,
}
