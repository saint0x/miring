//! Probe support for querying supported operations.

use crate::sys;

/// Probe structure for checking which opcodes are supported.
pub struct Probe {
    supported: [bool; Self::COUNT],
}

impl Probe {
    pub(crate) const COUNT: usize = 64;

    /// Create a new empty probe. Call [`Submitter::register_probe`](crate::Submitter::register_probe)
    /// to populate it.
    pub fn new() -> Self {
        Probe {
            supported: [false; Self::COUNT],
        }
    }

    /// Check if the given opcode is supported.
    pub fn is_supported(&self, opcode: u8) -> bool {
        self.supported
            .get(opcode as usize)
            .copied()
            .unwrap_or(false)
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut u8 {
        self.supported.as_mut_ptr() as *mut u8
    }

    /// Mark all opcodes up to `last` as supported.
    pub(crate) fn set_supported_range(&mut self, last: u8) {
        for i in 0..=last.min((Self::COUNT - 1) as u8) {
            self.supported[i as usize] = true;
        }
    }

    /// Mark a specific opcode as supported.
    pub(crate) fn set_supported(&mut self, opcode: u8) {
        if (opcode as usize) < Self::COUNT {
            self.supported[opcode as usize] = true;
        }
    }
}

impl Default for Probe {
    fn default() -> Self {
        Self::new()
    }
}

/// Restriction entry (stub for API compatibility).
#[repr(transparent)]
pub struct Restriction(sys::io_uring_restriction);

impl Restriction {
    /// Restrict to allow only the given register opcode.
    pub fn register_op(op: u8) -> Self {
        Restriction(sys::io_uring_restriction {
            opcode: 0, // IORING_RESTRICTION_REGISTER_OP
            register_op: op,
            sqe_op: 0,
            sqe_flags: 0,
            resv: 0,
            resv2: [0; 3],
        })
    }

    /// Restrict to allow only the given SQE opcode.
    pub fn sqe_op(op: u8) -> Self {
        Restriction(sys::io_uring_restriction {
            opcode: 1, // IORING_RESTRICTION_SQE_OP
            register_op: 0,
            sqe_op: op,
            sqe_flags: 0,
            resv: 0,
            resv2: [0; 3],
        })
    }

    /// Restrict to allow only the given SQE flags.
    pub fn sqe_flags_allowed(flags: u8) -> Self {
        Restriction(sys::io_uring_restriction {
            opcode: 2, // IORING_RESTRICTION_SQE_FLAGS_ALLOWED
            register_op: 0,
            sqe_op: 0,
            sqe_flags: flags,
            resv: 0,
            resv2: [0; 3],
        })
    }

    /// Restrict to require the given SQE flags.
    pub fn sqe_flags_required(flags: u8) -> Self {
        Restriction(sys::io_uring_restriction {
            opcode: 3, // IORING_RESTRICTION_SQE_FLAGS_REQUIRED
            register_op: 0,
            sqe_op: 0,
            sqe_flags: flags,
            resv: 0,
            resv2: [0; 3],
        })
    }
}
