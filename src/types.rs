//! Common Linux types not provided by libc.

pub(crate) mod sealed {
    use super::{Fd, Fixed};

    #[derive(Debug)]
    pub enum Target {
        Fd(i32),
        Fixed(u32),
    }

    pub trait UseFd: Sized {
        fn into(self) -> i32;
    }

    pub trait UseFixed: Sized {
        fn into(self) -> Target;
    }

    impl UseFd for Fd {
        #[inline]
        fn into(self) -> i32 {
            self.0
        }
    }

    impl UseFixed for Fd {
        #[inline]
        fn into(self) -> Target {
            Target::Fd(self.0)
        }
    }

    impl UseFixed for Fixed {
        #[inline]
        fn into(self) -> Target {
            Target::Fixed(self.0)
        }
    }
}

use crate::sys;
use crate::util::{unwrap_nonzero, unwrap_u32};
use bitflags::bitflags;
use std::marker::PhantomData;
use std::num::NonZeroU32;

/// A file descriptor that has not been registered with io_uring.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Fd(pub i32);

/// A file descriptor that has been registered with io_uring using
/// [`Submitter::register_files`](crate::Submitter::register_files) or
/// [`Submitter::register_files_sparse`](crate::Submitter::register_files_sparse).
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Fixed(pub u32);

bitflags! {
    /// Options for [`Timeout`](super::opcode::Timeout).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct TimeoutFlags: u32 {
        const ABS = sys::IORING_TIMEOUT_ABS;
        const BOOTTIME = sys::IORING_TIMEOUT_BOOTTIME;
        const REALTIME = sys::IORING_TIMEOUT_REALTIME;
        const LINK_TIMEOUT_UPDATE = sys::IORING_LINK_TIMEOUT_UPDATE;
        const ETIME_SUCCESS = sys::IORING_TIMEOUT_ETIME_SUCCESS;
        const MULTISHOT = sys::IORING_TIMEOUT_MULTISHOT;
    }
}

bitflags! {
    /// Options for [`Fsync`](super::opcode::Fsync).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct FsyncFlags: u32 {
        const DATASYNC = sys::IORING_FSYNC_DATASYNC;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub(crate) struct AsyncCancelFlags: u32 {
        const ALL = sys::IORING_ASYNC_CANCEL_ALL;
        const FD = sys::IORING_ASYNC_CANCEL_FD;
        const ANY = sys::IORING_ASYNC_CANCEL_ANY;
        const FD_FIXED = sys::IORING_ASYNC_CANCEL_FD_FIXED;
    }
}

/// Wrapper around `open_how` as used in openat2(2).
#[derive(Default, Debug, Clone, Copy)]
#[repr(transparent)]
pub struct OpenHow(pub(crate) sys::open_how);

impl OpenHow {
    pub const fn new() -> Self {
        OpenHow(sys::open_how {
            flags: 0,
            mode: 0,
            resolve: 0,
        })
    }

    pub const fn flags(mut self, flags: u64) -> Self {
        self.0.flags = flags;
        self
    }

    pub const fn mode(mut self, mode: u64) -> Self {
        self.0.mode = mode;
        self
    }

    pub const fn resolve(mut self, resolve: u64) -> Self {
        self.0.resolve = resolve;
        self
    }
}

#[derive(Default, Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Timespec(pub(crate) sys::__kernel_timespec);

impl Timespec {
    #[inline]
    pub const fn new() -> Self {
        Timespec(sys::__kernel_timespec {
            tv_sec: 0,
            tv_nsec: 0,
        })
    }

    #[inline]
    pub const fn sec(mut self, sec: u64) -> Self {
        self.0.tv_sec = sec as _;
        self
    }

    #[inline]
    pub const fn nsec(mut self, nsec: u32) -> Self {
        self.0.tv_nsec = nsec as _;
        self
    }
}

impl From<std::time::Duration> for Timespec {
    fn from(value: std::time::Duration) -> Self {
        Timespec::new()
            .sec(value.as_secs())
            .nsec(value.subsec_nanos())
    }
}

/// Submit arguments.
#[repr(transparent)]
#[derive(Default, Debug, Clone, Copy)]
pub struct SubmitArgs<'prev: 'now, 'now> {
    pub(crate) args: sys::io_uring_getevents_arg,
    prev: PhantomData<&'prev ()>,
    now: PhantomData<&'now ()>,
}

impl<'prev, 'now> SubmitArgs<'prev, 'now> {
    #[inline]
    pub const fn new() -> SubmitArgs<'static, 'static> {
        let args = sys::io_uring_getevents_arg {
            sigmask: 0,
            sigmask_sz: 0,
            min_wait_usec: 0,
            ts: 0,
        };
        SubmitArgs {
            args,
            prev: PhantomData,
            now: PhantomData,
        }
    }

    /// Sets a timeout in microseconds to start waiting for a minimum of a single completion.
    #[inline]
    pub fn min_wait_usec(mut self, min_wait_usec: u32) -> Self {
        self.args.min_wait_usec = min_wait_usec;
        self
    }

    #[inline]
    pub fn timespec<'new>(mut self, timespec: &'new Timespec) -> SubmitArgs<'now, 'new> {
        self.args.ts = timespec as *const Timespec as u64;
        SubmitArgs {
            args: self.args,
            prev: self.now,
            now: PhantomData,
        }
    }
}

#[repr(transparent)]
pub struct BufRingEntry(sys::io_uring_buf);

#[allow(clippy::len_without_is_empty)]
impl BufRingEntry {
    pub fn set_addr(&mut self, addr: u64) {
        self.0.addr = addr;
    }

    pub fn addr(&self) -> u64 {
        self.0.addr
    }

    pub fn set_len(&mut self, len: u32) {
        self.0.len = len;
    }

    pub fn len(&self) -> u32 {
        self.0.len
    }

    pub fn set_bid(&mut self, bid: u16) {
        self.0.bid = bid;
    }

    pub fn bid(&self) -> u16 {
        self.0.bid
    }

    pub unsafe fn tail(ring_base: *const BufRingEntry) -> *const u16 {
        std::ptr::addr_of!((*ring_base).0.resv)
    }
}

/// A destination slot for sending fixed resources.
#[derive(Debug, Clone, Copy)]
pub struct DestinationSlot {
    dest: NonZeroU32,
}

impl DestinationSlot {
    const AUTO_ALLOC: NonZeroU32 =
        unwrap_nonzero(NonZeroU32::new(sys::IORING_FILE_INDEX_ALLOC as u32));

    pub const fn auto_target() -> Self {
        Self {
            dest: DestinationSlot::AUTO_ALLOC,
        }
    }

    pub fn try_from_slot_target(target: u32) -> Result<Self, u32> {
        const MAX_INDEX: u32 = unwrap_u32(DestinationSlot::AUTO_ALLOC.get().checked_sub(2));

        if target > MAX_INDEX {
            return Err(target);
        }

        let kernel_index = target.saturating_add(1);
        debug_assert!(0 < kernel_index && kernel_index < DestinationSlot::AUTO_ALLOC.get());
        let dest = NonZeroU32::new(kernel_index).unwrap();

        Ok(Self { dest })
    }

    pub(crate) fn kernel_index_arg(&self) -> u32 {
        self.dest.get()
    }
}

/// [CancelBuilder] constructs match criteria for request cancellation.
#[derive(Debug)]
pub struct CancelBuilder {
    pub(crate) flags: AsyncCancelFlags,
    pub(crate) user_data: Option<u64>,
    pub(crate) fd: Option<sealed::Target>,
}

impl CancelBuilder {
    pub const fn any() -> Self {
        Self {
            flags: AsyncCancelFlags::ANY,
            user_data: None,
            fd: None,
        }
    }

    pub const fn user_data(user_data: u64) -> Self {
        Self {
            flags: AsyncCancelFlags::empty(),
            user_data: Some(user_data),
            fd: None,
        }
    }

    pub fn fd(fd: impl sealed::UseFixed) -> Self {
        let mut flags = AsyncCancelFlags::FD;
        let target = fd.into();
        if matches!(target, sealed::Target::Fixed(_)) {
            flags.insert(AsyncCancelFlags::FD_FIXED);
        }
        Self {
            flags,
            user_data: None,
            fd: Some(target),
        }
    }

    pub fn all(mut self) -> Self {
        self.flags.insert(AsyncCancelFlags::ALL);
        self
    }

    pub(crate) fn to_fd(&self) -> i32 {
        self.fd
            .as_ref()
            .map(|target| match *target {
                sealed::Target::Fd(fd) => fd,
                sealed::Target::Fixed(idx) => idx as i32,
            })
            .unwrap_or(-1)
    }
}

/// Wrapper around `futex_waitv`.
#[derive(Default, Debug, Clone, Copy)]
#[repr(transparent)]
pub struct FutexWaitV(pub(crate) sys::futex_waitv);

impl FutexWaitV {
    pub const fn new() -> Self {
        Self(sys::futex_waitv {
            val: 0,
            uaddr: 0,
            flags: 0,
            __reserved: 0,
        })
    }

    pub const fn val(mut self, val: u64) -> Self {
        self.0.val = val;
        self
    }

    pub const fn uaddr(mut self, uaddr: u64) -> Self {
        self.0.uaddr = uaddr;
        self
    }

    pub const fn flags(mut self, flags: u32) -> Self {
        self.0.flags = flags;
        self
    }
}

/// Opaque types — use the libc equivalents directly.
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct statx {
    _priv: (),
}

#[repr(C)]
#[allow(non_camel_case_types)]
pub struct epoll_event {
    _priv: (),
}

/// Helper structure for parsing the result of a multishot RecvMsg.
#[derive(Debug)]
pub struct RecvMsgOut<'buf> {
    header: sys::io_uring_recvmsg_out,
    msghdr_name_len: usize,
    name_data: &'buf [u8],
    control_data: &'buf [u8],
    payload_data: &'buf [u8],
}

impl<'buf> RecvMsgOut<'buf> {
    const DATA_START: usize = std::mem::size_of::<sys::io_uring_recvmsg_out>();

    /// Parse the data buffered upon completion of a multishot `RecvMsg` operation.
    ///
    /// `buffer` is the whole buffer previously provided to the ring.
    /// `msghdr_name_len` and `msghdr_control_len` correspond to the `msg_namelen`
    /// and `msg_controllen` fields of the msghdr passed to the SQE.
    #[allow(clippy::result_unit_err)]
    pub fn parse(
        buffer: &'buf [u8],
        msghdr_name_len: u32,
        msghdr_control_len: usize,
    ) -> Result<Self, ()> {
        let name_len = msghdr_name_len as usize;
        let control_len = msghdr_control_len;

        if Self::DATA_START
            .checked_add(name_len)
            .and_then(|acc| acc.checked_add(control_len))
            .map(|header_len| buffer.len() < header_len)
            .unwrap_or(true)
        {
            return Err(());
        }

        // Read the io_uring_recvmsg_out header (may be unaligned).
        let header = unsafe {
            buffer
                .as_ptr()
                .cast::<sys::io_uring_recvmsg_out>()
                .read_unaligned()
        };

        // Name data: may be truncated if incoming name > msghdr_name_len.
        let (name_data, control_start) = {
            let name_start = Self::DATA_START;
            let name_data_end =
                name_start + usize::min(header.namelen as usize, name_len);
            let name_field_end = name_start + name_len;
            (&buffer[name_start..name_data_end], name_field_end)
        };

        // Control data: similarly may be truncated.
        let (control_data, payload_start) = {
            let control_data_end = control_start
                + usize::min(header.controllen as usize, control_len);
            let control_field_end = control_start + control_len;
            (&buffer[control_start..control_data_end], control_field_end)
        };

        // Payload: the remaining data.
        let payload_data = {
            let payload_data_end = payload_start
                + usize::min(
                    header.payloadlen as usize,
                    buffer.len() - payload_start,
                );
            &buffer[payload_start..payload_data_end]
        };

        Ok(Self {
            header,
            msghdr_name_len: name_len,
            name_data,
            control_data,
            payload_data,
        })
    }

    /// Parse using a raw pointer to a C `msghdr` struct.
    ///
    /// This is API-compatible with `libc::msghdr`. The pointer must point to a
    /// valid `msghdr` struct (only `msg_namelen` at offset 4 and `msg_controllen`
    /// are read).
    ///
    /// # Safety
    ///
    /// `msghdr` must point to a valid, initialized C `msghdr` struct.
    #[allow(clippy::result_unit_err)]
    pub unsafe fn parse_msghdr(
        buffer: &'buf [u8],
        msghdr: *const std::ffi::c_void,
    ) -> Result<Self, ()> {
        // msghdr layout (LP64):
        //   void       *msg_name;       // offset 0, size 8
        //   socklen_t   msg_namelen;     // offset 8, size 4
        //   ...padding...               // offset 12, size 4
        //   struct iovec *msg_iov;       // offset 16, size 8
        //   size_t       msg_iovlen;     // offset 24, size 8
        //   void        *msg_control;    // offset 32, size 8
        //   size_t       msg_controllen; // offset 40, size 8
        let ptr = msghdr as *const u8;
        let msg_namelen = (ptr.add(8) as *const u32).read_unaligned();
        let msg_controllen = (ptr.add(40) as *const usize).read_unaligned();
        Self::parse(buffer, msg_namelen, msg_controllen)
    }

    pub fn incoming_name_len(&self) -> u32 {
        self.header.namelen
    }

    pub fn is_name_data_truncated(&self) -> bool {
        self.header.namelen as usize > self.msghdr_name_len
    }

    pub fn name_data(&self) -> &[u8] {
        self.name_data
    }

    pub fn incoming_control_len(&self) -> u32 {
        self.header.controllen
    }

    pub fn control_data(&self) -> &[u8] {
        self.control_data
    }

    pub fn payload_data(&self) -> &[u8] {
        self.payload_data
    }

    pub fn incoming_payload_len(&self) -> u32 {
        self.header.payloadlen
    }

    pub fn flags(&self) -> u32 {
        self.header.flags
    }
}
