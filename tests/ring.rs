//! Integration tests for the miring io_uring emulator.

use miring::cqueue;
use miring::opcode;
use miring::squeue;
use miring::types::Fd;
use miring::IoUring;

// ---------------------------------------------------------------------------
// Basic ring lifecycle
// ---------------------------------------------------------------------------

#[test]
fn create_and_drop() {
    let _ring = IoUring::new(8).unwrap();
}

#[test]
fn builder_default() {
    let ring: IoUring = IoUring::builder().build(16).unwrap();
    let p = ring.params();
    assert_eq!(p.sq_entries(), 16);
    assert_eq!(p.cq_entries(), 32); // default: 2 * sq
    assert!(p.is_feature_single_mmap());
    assert!(p.is_feature_nodrop());
    assert!(p.is_feature_skip_cqe_on_success());
}

#[test]
fn builder_custom_cqsize() {
    let ring: IoUring = IoUring::builder()
        .setup_cqsize(64)
        .build(8)
        .unwrap();
    let p = ring.params();
    assert_eq!(p.sq_entries(), 8);
    assert_eq!(p.cq_entries(), 64);
}

#[test]
fn entries_rounded_to_power_of_two() {
    let ring = IoUring::new(5).unwrap();
    assert_eq!(ring.params().sq_entries(), 8);
    assert_eq!(ring.params().cq_entries(), 16);
}

#[test]
fn zero_entries_is_error() {
    assert!(IoUring::new(0).is_err());
}

// ---------------------------------------------------------------------------
// Submission queue basics
// ---------------------------------------------------------------------------

#[test]
fn sq_push_and_len() {
    let mut ring = IoUring::new(4).unwrap();
    {
        let mut sq = ring.submission();
        assert!(sq.is_empty());
        assert_eq!(sq.capacity(), 4);

        let nop = opcode::Nop::new().build().user_data(42);
        unsafe { sq.push(&nop).unwrap() };
        assert_eq!(sq.len(), 1);
    }
}

#[test]
fn sq_full_returns_push_error() {
    let mut ring = IoUring::new(2).unwrap();
    let mut sq = ring.submission();
    let nop = opcode::Nop::new().build();

    unsafe {
        sq.push(&nop).unwrap();
        sq.push(&nop).unwrap();
        assert!(sq.push(&nop).is_err());
    }
}

// ---------------------------------------------------------------------------
// Submit + complete: single NOP
// ---------------------------------------------------------------------------

#[test]
fn submit_single_nop() {
    let mut ring = IoUring::new(8).unwrap();

    // Push a NOP
    {
        let nop = opcode::Nop::new().build().user_data(0xBEEF);
        let mut sq = ring.submission();
        unsafe { sq.push(&nop).unwrap() };
    }

    // Submit
    let submitted = ring.submit().unwrap();
    assert_eq!(submitted, 1);

    // Read completion
    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 0xBEEF);
    assert_eq!(cqe.result(), 0);
    assert!(cq.next().is_none());
}

// ---------------------------------------------------------------------------
// Submit + complete: batch of NOPs
// ---------------------------------------------------------------------------

#[test]
fn submit_batch_nops() {
    let mut ring = IoUring::new(8).unwrap();

    // Push 5 NOPs with different user_data
    {
        let mut sq = ring.submission();
        for i in 0..5u64 {
            let nop = opcode::Nop::new().build().user_data(i);
            unsafe { sq.push(&nop).unwrap() };
        }
        assert_eq!(sq.len(), 5);
    }

    let submitted = ring.submit().unwrap();
    assert_eq!(submitted, 5);

    // Check all completions
    let mut cq = ring.completion();
    for i in 0..5u64 {
        let cqe = cq.next().unwrap();
        assert_eq!(cqe.user_data(), i);
        assert_eq!(cqe.result(), 0);
    }
    assert!(cq.next().is_none());
}

// ---------------------------------------------------------------------------
// NOP with IORING_NOP_INJECT_RESULT
// ---------------------------------------------------------------------------

#[test]
fn nop_inject_result() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let nop = opcode::Nop::new()
            .inject_result(42)
            .build()
            .user_data(1);
        let mut sq = ring.submission();
        unsafe { sq.push(&nop).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 1);
    assert_eq!(cqe.result(), 42);
}

#[test]
fn nop_inject_negative_result() {
    let mut ring = IoUring::new(8).unwrap();

    {
        // Cast -22 (EINVAL) to u32 for len field, dispatch reads it back as i32
        let nop = opcode::Nop::new()
            .inject_result(-22i32 as u32)
            .build()
            .user_data(2);
        let mut sq = ring.submission();
        unsafe { sq.push(&nop).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.result(), -22);
}

// ---------------------------------------------------------------------------
// Read op touches the buffer (Miri validates writability)
// ---------------------------------------------------------------------------

#[test]
fn read_op_fills_buffer() {
    let mut ring = IoUring::new(8).unwrap();
    let mut buf = [0xFFu8; 64];

    {
        let entry = opcode::Read::new(miring::types::Fd(0), buf.as_mut_ptr(), buf.len() as u32)
            .build()
            .user_data(99);
        let mut sq = ring.submission();
        unsafe { sq.push(&entry).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 99);
    assert_eq!(cqe.result(), 64); // bytes "read"
    assert!(buf.iter().all(|&b| b == 0)); // emulator fills with zeros
}

// ---------------------------------------------------------------------------
// Write op reads the buffer (Miri validates initialization)
// ---------------------------------------------------------------------------

#[test]
fn write_op_reads_buffer() {
    let mut ring = IoUring::new(8).unwrap();
    let buf = [42u8; 32];

    {
        let entry =
            opcode::Write::new(miring::types::Fd(1), buf.as_ptr(), buf.len() as u32)
                .build()
                .user_data(100);
        let mut sq = ring.submission();
        unsafe { sq.push(&entry).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 100);
    assert_eq!(cqe.result(), 32);
}

// ---------------------------------------------------------------------------
// Readv touches scatter buffers
// ---------------------------------------------------------------------------

#[test]
fn readv_fills_scatter_buffers() {
    let mut ring = IoUring::new(8).unwrap();
    let mut buf1 = [0xFFu8; 16];
    let mut buf2 = [0xFFu8; 32];

    let iovecs = [
        std::io::IoSliceMut::new(&mut buf1),
        std::io::IoSliceMut::new(&mut buf2),
    ];

    {
        let entry = opcode::Readv::new(
            miring::types::Fd(0),
            iovecs.as_ptr() as *const _,
            iovecs.len() as u32,
        )
        .build()
        .user_data(200);
        let mut sq = ring.submission();
        unsafe { sq.push(&entry).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 200);
    assert_eq!(cqe.result(), 48); // 16 + 32
    assert!(buf1.iter().all(|&b| b == 0));
    assert!(buf2.iter().all(|&b| b == 0));
}

// ---------------------------------------------------------------------------
// Writev reads gather buffers
// ---------------------------------------------------------------------------

#[test]
fn writev_reads_gather_buffers() {
    let mut ring = IoUring::new(8).unwrap();
    let buf1 = [1u8; 16];
    let buf2 = [2u8; 32];

    let iovecs = [
        std::io::IoSlice::new(&buf1),
        std::io::IoSlice::new(&buf2),
    ];

    {
        let entry = opcode::Writev::new(
            miring::types::Fd(1),
            iovecs.as_ptr() as *const _,
            iovecs.len() as u32,
        )
        .build()
        .user_data(201);
        let mut sq = ring.submission();
        unsafe { sq.push(&entry).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 201);
    assert_eq!(cqe.result(), 48);
}

// ---------------------------------------------------------------------------
// Recv/Send touch buffers like Read/Write
// ---------------------------------------------------------------------------

#[test]
fn recv_fills_buffer() {
    let mut ring = IoUring::new(8).unwrap();
    let mut buf = [0xFFu8; 128];

    {
        let entry = opcode::Recv::new(miring::types::Fd(3), buf.as_mut_ptr(), buf.len() as u32)
            .build()
            .user_data(300);
        let mut sq = ring.submission();
        unsafe { sq.push(&entry).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.result(), 128);
    assert!(buf.iter().all(|&b| b == 0));
}

#[test]
fn send_reads_buffer() {
    let mut ring = IoUring::new(8).unwrap();
    let buf = [77u8; 64];

    {
        let entry = opcode::Send::new(miring::types::Fd(3), buf.as_ptr(), buf.len() as u32)
            .build()
            .user_data(301);
        let mut sq = ring.submission();
        unsafe { sq.push(&entry).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.result(), 64);
}

// ---------------------------------------------------------------------------
// Timeout reads __kernel_timespec
// ---------------------------------------------------------------------------

#[test]
fn timeout_reads_timespec() {
    let mut ring = IoUring::new(8).unwrap();
    let ts = miring::types::Timespec::new().sec(1).nsec(500_000_000);

    {
        let entry = opcode::Timeout::new(&ts as *const _ as *const _)
            .build()
            .user_data(400);
        let mut sq = ring.submission();
        unsafe { sq.push(&entry).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 400);
    assert_eq!(cqe.result(), -62); // -ETIME
}

// ---------------------------------------------------------------------------
// Close succeeds (no-op)
// ---------------------------------------------------------------------------

#[test]
fn close_succeeds() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let entry = opcode::Close::new(miring::types::Fd(5))
            .build()
            .user_data(500);
        let mut sq = ring.submission();
        unsafe { sq.push(&entry).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 500);
    assert_eq!(cqe.result(), 0);
}

// ---------------------------------------------------------------------------
// SKIP_SUCCESS flag
// ---------------------------------------------------------------------------

#[test]
fn skip_success_suppresses_cqe() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let mut sq = ring.submission();
        // NOP with SKIP_SUCCESS — should NOT produce a CQE (result is 0 = success)
        let nop = opcode::Nop::new()
            .build()
            .user_data(1)
            .flags(squeue::Flags::SKIP_SUCCESS);
        unsafe { sq.push(&nop).unwrap() };

        // Normal NOP — should produce a CQE
        let nop2 = opcode::Nop::new().build().user_data(2);
        unsafe { sq.push(&nop2).unwrap() };
    }

    let submitted = ring.submit().unwrap();
    assert_eq!(submitted, 2);

    let mut cq = ring.completion();
    // Only the second NOP should appear
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 2);
    assert!(cq.next().is_none());
}

#[test]
fn skip_success_still_posts_on_error() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let mut sq = ring.submission();
        // NOP with inject_result(-1) and SKIP_SUCCESS — result < 0 so CQE IS posted
        let nop = opcode::Nop::new()
            .inject_result(-1i32 as u32)
            .build()
            .user_data(1)
            .flags(squeue::Flags::SKIP_SUCCESS);
        unsafe { sq.push(&nop).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 1);
    assert_eq!(cqe.result(), -1);
}

// ---------------------------------------------------------------------------
// split() API
// ---------------------------------------------------------------------------

#[test]
fn split_api() {
    let mut ring = IoUring::new(8).unwrap();
    let (submitter, mut sq, _cq) = ring.split();

    let nop = opcode::Nop::new().build().user_data(0xCAFE);
    unsafe { sq.push(&nop).unwrap() };
    sq.sync();

    let submitted = submitter.submit().unwrap();
    assert_eq!(submitted, 1);
}

// ---------------------------------------------------------------------------
// Ring wrapping
// ---------------------------------------------------------------------------

#[test]
fn ring_wraps_around() {
    let mut ring = IoUring::new(4).unwrap(); // 4-entry SQ

    // Submit + drain multiple rounds to exercise wrapping
    for round in 0..10u64 {
        {
            let mut sq = ring.submission();
            for i in 0..4u64 {
                let nop = opcode::Nop::new().build().user_data(round * 4 + i);
                unsafe { sq.push(&nop).unwrap() };
            }
        }

        let submitted = ring.submit().unwrap();
        assert_eq!(submitted, 4);

        let mut cq = ring.completion();
        let mut count = 0;
        for cqe in &mut cq {
            assert_eq!(cqe.user_data(), round * 4 + count);
            count += 1;
        }
        assert_eq!(count, 4);
    }
}

// ---------------------------------------------------------------------------
// CQ overflow
// ---------------------------------------------------------------------------

#[test]
fn cq_overflow_counter() {
    let mut ring: IoUring = IoUring::builder().setup_cqsize(4).build(4).unwrap();

    // Push 4 NOPs (CQ size = 4)
    {
        let mut sq = ring.submission();
        for i in 0..4u64 {
            let nop = opcode::Nop::new().build().user_data(i);
            unsafe { sq.push(&nop).unwrap() };
        }
    }
    ring.submit().unwrap();

    // CQ is now full (4 entries). Don't drain it.
    // Push and submit one more — this should overflow.
    {
        let mut sq = ring.submission();
        let nop = opcode::Nop::new().build().user_data(99);
        unsafe { sq.push(&nop).unwrap() };
    }
    ring.submit().unwrap();

    // Check overflow counter
    let cq = ring.completion();
    assert_eq!(cq.overflow(), 1);
}

// ---------------------------------------------------------------------------
// submit_and_wait
// ---------------------------------------------------------------------------

#[test]
fn submit_and_wait_basic() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let mut sq = ring.submission();
        let nop = opcode::Nop::new().build().user_data(7);
        unsafe { sq.push(&nop).unwrap() };
    }

    let submitted = ring.submit_and_wait(1).unwrap();
    assert_eq!(submitted, 1);

    let mut cq = ring.completion();
    assert_eq!(cq.next().unwrap().user_data(), 7);
}

// ---------------------------------------------------------------------------
// Multiple submit cycles without draining CQ
// ---------------------------------------------------------------------------

#[test]
fn multiple_submits_accumulate_cqes() {
    let mut ring = IoUring::new(8).unwrap();

    for i in 0..3u64 {
        {
            let mut sq = ring.submission();
            let nop = opcode::Nop::new().build().user_data(i);
            unsafe { sq.push(&nop).unwrap() };
        }
        ring.submit().unwrap();
    }

    // Drain all 3 at once
    let mut cq = ring.completion();
    for i in 0..3u64 {
        let cqe = cq.next().unwrap();
        assert_eq!(cqe.user_data(), i);
    }
    assert!(cq.next().is_none());
}

// ---------------------------------------------------------------------------
// Entry builder helpers
// ---------------------------------------------------------------------------

#[test]
fn entry_user_data_roundtrip() {
    let entry = opcode::Nop::new().build().user_data(u64::MAX);
    assert_eq!(entry.get_user_data(), u64::MAX);
}

#[test]
fn entry_opcode_is_nop() {
    let entry = opcode::Nop::new().build();
    assert_eq!(entry.get_opcode(), 0); // IORING_OP_NOP
}

// ---------------------------------------------------------------------------
// Probe
// ---------------------------------------------------------------------------

#[test]
fn probe_reports_nop_supported() {
    let ring = IoUring::new(8).unwrap();
    let mut probe = miring::Probe::new();
    ring.submitter().register_probe(&mut probe).unwrap();
    assert!(probe.is_supported(0)); // NOP
}

// ---------------------------------------------------------------------------
// Parameters feature flags
// ---------------------------------------------------------------------------

#[test]
fn params_features() {
    let ring = IoUring::new(8).unwrap();
    let p = ring.params();

    assert!(p.is_feature_single_mmap());
    assert!(p.is_feature_nodrop());
    assert!(p.is_feature_submit_stable());
    assert!(p.is_feature_rw_cur_pos());
    assert!(p.is_feature_cur_personality());
    assert!(p.is_feature_fast_poll());
    assert!(p.is_feature_poll_32bits());
    assert!(p.is_feature_ext_arg());
    assert!(p.is_feature_native_workers());
    assert!(p.is_feature_resource_tagging());
    assert!(p.is_feature_skip_cqe_on_success());
    assert!(p.is_feature_linked_file());
}

// ---------------------------------------------------------------------------
// CQ helper functions
// ---------------------------------------------------------------------------

#[test]
fn cqueue_buffer_select() {
    assert!(cqueue::buffer_select(0).is_none());
    // IORING_CQE_F_BUFFER (1) with buffer ID in upper bits
    let flags = 1u32 | (42u32 << 16);
    assert_eq!(cqueue::buffer_select(flags), Some(42));
}

#[test]
fn cqueue_more_flag() {
    assert!(!cqueue::more(0));
    assert!(cqueue::more(2)); // IORING_CQE_F_MORE = 2
}

#[test]
fn cqueue_sock_nonempty_flag() {
    assert!(!cqueue::sock_nonempty(0));
    assert!(cqueue::sock_nonempty(4)); // IORING_CQE_F_SOCK_NONEMPTY = 4
}

// ---------------------------------------------------------------------------
// 32-byte CQE (Entry32) support
// ---------------------------------------------------------------------------

#[test]
fn entry32_ring() {
    let mut ring: IoUring<squeue::Entry, cqueue::Entry32> =
        IoUring::builder().build(8).unwrap();

    {
        let mut sq = ring.submission();
        let nop = opcode::Nop::new().build().user_data(0xDEAD);
        unsafe { sq.push(&nop).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 0xDEAD);
    assert_eq!(cqe.result(), 0);
    assert_eq!(*cqe.big_cqe(), [0u64; 2]);
}

// ---------------------------------------------------------------------------
// 128-byte SQE (Entry128) support
// ---------------------------------------------------------------------------

#[test]
fn entry128_ring() {
    let mut ring: IoUring<squeue::Entry128, cqueue::Entry> =
        IoUring::builder().build(4).unwrap();

    {
        let mut sq = ring.submission();
        let nop = opcode::Nop::new().build().user_data(0xFACE);
        let entry128: squeue::Entry128 = nop.into();
        unsafe { sq.push(&entry128).unwrap() };
    }

    ring.submit().unwrap();

    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    assert_eq!(cqe.user_data(), 0xFACE);
    assert_eq!(cqe.result(), 0);
}

// ---------------------------------------------------------------------------
// Personality (no-op but API compatible)
// ---------------------------------------------------------------------------

#[test]
fn register_personality() {
    let ring = IoUring::new(8).unwrap();
    let id = ring.submitter().register_personality().unwrap();
    assert_eq!(id, 0);
    ring.submitter().unregister_personality(id).unwrap();
}

// ---------------------------------------------------------------------------
// Debug formatting
// ---------------------------------------------------------------------------

#[test]
fn debug_format_params() {
    let ring = IoUring::new(8).unwrap();
    let debug = format!("{:?}", ring.params());
    assert!(debug.contains("Parameters"));
    assert!(debug.contains("sq_entries"));
}

#[test]
fn debug_format_cqe() {
    let mut ring = IoUring::new(8).unwrap();
    {
        let mut sq = ring.submission();
        let nop = opcode::Nop::new().build().user_data(123);
        unsafe { sq.push(&nop).unwrap() };
    }
    ring.submit().unwrap();
    let mut cq = ring.completion();
    let cqe = cq.next().unwrap();
    let debug = format!("{:?}", cqe);
    assert!(debug.contains("123"));
}

// ---------------------------------------------------------------------------
// squeue_wait (always returns Ok(0) in emulator)
// ---------------------------------------------------------------------------

#[test]
fn squeue_wait() {
    let ring = IoUring::new(8).unwrap();
    assert_eq!(ring.submitter().squeue_wait().unwrap(), 0);
}

// ---------------------------------------------------------------------------
// Deferred completion: buffer touching happens on a background thread,
// NOT during submit(). This enables Miri to detect use-after-free and
// data races on buffers freed/modified between submit and completion.
// ---------------------------------------------------------------------------

#[test]
fn submit_defers_buffer_touch_to_completion() {
    use miring::types::Fd;

    let mut ring = IoUring::new(8).unwrap();

    // Fill buffer with 0xFF — our Read dispatch fills with 0x00,
    // so we can tell whether the dispatch has run.
    let mut buf = vec![0xFFu8; 64];
    let entry = opcode::Read::new(Fd(0), buf.as_mut_ptr(), 64)
        .build()
        .user_data(1);
    unsafe { ring.submission().push(&entry).unwrap() };

    // Submit — spawns background thread but doesn't block for it.
    ring.submit().unwrap();

    // Give the background thread a chance to run (it yields once per op).
    // Even if it does run, Miri won't flag the read below as a data race
    // because the thread is done before we check.
    // The key point: on the default schedule (preemption_rate=0), the
    // background thread hasn't run yet, so buffer is UNTOUCHED.
    // (With preemption>0, the thread might have already run — either way
    // is correct; we just need to verify completions eventually appear.)

    // Reap completions — THIS joins the background thread and posts CQEs.
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.user_data(), 1);
    assert_eq!(cqe.result(), 64);

    // After completion, buffer should be zeroed by the Read dispatch.
    assert!(
        buf.iter().all(|&b| b == 0),
        "buffer should be zeroed after Read completion"
    );
}

#[test]
fn multiple_submits_before_completion() {
    use miring::types::Fd;

    let mut ring = IoUring::new(8).unwrap();

    // Submit two batches without reading completions between them.
    let mut buf1 = vec![0xFFu8; 32];
    let entry1 = opcode::Read::new(Fd(0), buf1.as_mut_ptr(), 32)
        .build()
        .user_data(1);
    unsafe { ring.submission().push(&entry1).unwrap() };
    ring.submit().unwrap();

    let mut buf2 = vec![0xFFu8; 16];
    let entry2 = opcode::Read::new(Fd(0), buf2.as_mut_ptr(), 16)
        .build()
        .user_data(2);
    unsafe { ring.submission().push(&entry2).unwrap() };
    ring.submit().unwrap();

    // Reap all completions — both batches should be flushed.
    let mut cq = ring.completion();
    let cqe1 = cq.next().unwrap();
    let cqe2 = cq.next().unwrap();
    assert_eq!(cqe1.user_data(), 1);
    assert_eq!(cqe1.result(), 32);
    assert_eq!(cqe2.user_data(), 2);
    assert_eq!(cqe2.result(), 16);
    drop(cq);

    assert!(buf1.iter().all(|&b| b == 0));
    assert!(buf2.iter().all(|&b| b == 0));
}

#[test]
fn submit_and_wait_flushes_pending() {
    let mut ring = IoUring::new(8).unwrap();

    let mut buf = vec![0xFFu8; 64];
    let entry = opcode::Read::new(Fd(0), buf.as_mut_ptr(), 64)
        .build()
        .user_data(1);
    unsafe { ring.submission().push(&entry).unwrap() };

    // submit_and_wait(1) should both submit AND flush pending (join thread).
    ring.submit_and_wait(1).unwrap();

    // Buffer should be zeroed now — the dispatch ran during submit_and_wait.
    assert!(buf.iter().all(|&b| b == 0));

    // CQE should be ready.
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.user_data(), 1);
    assert_eq!(cqe.result(), 64);
}

// ===========================================================================
// Pathname-touching ops
// ===========================================================================

#[test]
fn renameat_reads_two_pathnames() {
    let mut ring = IoUring::new(8).unwrap();
    let old = b"/tmp/old\0";
    let new = b"/tmp/new\0";

    {
        let entry = opcode::RenameAt::new(Fd(-1), old.as_ptr(), -1, new.as_ptr())
            .build()
            .user_data(1000);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.user_data(), 1000);
    assert_eq!(cqe.result(), 0);
}

#[test]
fn unlinkat_reads_pathname() {
    let mut ring = IoUring::new(8).unwrap();
    let path = b"/tmp/gone\0";

    {
        let entry = opcode::UnlinkAt::new(Fd(-1), path.as_ptr())
            .build()
            .user_data(1001);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn mkdirat_reads_pathname() {
    let mut ring = IoUring::new(8).unwrap();
    let path = b"/tmp/dir\0";

    {
        let entry = opcode::MkDirAt::new(Fd(-1), path.as_ptr())
            .build()
            .user_data(1002);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn symlinkat_reads_two_pathnames() {
    let mut ring = IoUring::new(8).unwrap();
    let target = b"/tmp/target\0";
    let link = b"/tmp/link\0";

    {
        let entry = opcode::SymlinkAt::new(Fd(-1), target.as_ptr(), link.as_ptr())
            .build()
            .user_data(1003);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn linkat_reads_two_pathnames() {
    let mut ring = IoUring::new(8).unwrap();
    let old = b"/tmp/existing\0";
    let new = b"/tmp/hardlink\0";

    {
        let entry = opcode::LinkAt::new(Fd(-1), old.as_ptr(), -1, new.as_ptr())
            .build()
            .user_data(1004);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

// ===========================================================================
// Network ops
// ===========================================================================

#[test]
fn connect_reads_sockaddr() {
    let mut ring = IoUring::new(8).unwrap();
    // Fake 16-byte sockaddr
    let sockaddr = [0u8; 16];

    {
        let entry = opcode::Connect::new(
            Fd(3),
            sockaddr.as_ptr() as *const _,
            sockaddr.len() as u32,
        )
        .build()
        .user_data(1100);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn accept_writes_sockaddr() {
    let mut ring = IoUring::new(8).unwrap();
    let mut sockaddr = [0xFFu8; 28];
    let mut addrlen: u32 = 28;

    {
        let entry = opcode::Accept::new(Fd(3))
            .addr(sockaddr.as_mut_ptr() as *mut _)
            .addrlen(&mut addrlen as *mut _)
            .build()
            .user_data(1101);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 42); // fake fd
    assert!(sockaddr.iter().all(|&b| b == 0)); // zeroed by emulator
}

#[test]
fn bind_reads_sockaddr() {
    let mut ring = IoUring::new(8).unwrap();
    let sockaddr = [2u8; 16]; // AF_INET-ish

    {
        let entry = opcode::Bind::new(
            Fd(3),
            sockaddr.as_ptr() as *const _,
            sockaddr.len() as u32,
        )
        .build()
        .user_data(1102);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn socket_returns_fd() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let entry = opcode::Socket::new(2, 1, 0) // AF_INET, SOCK_STREAM
            .build()
            .user_data(1103);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 42); // fake fd
}

#[test]
fn send_zc_reads_buffer() {
    let mut ring = IoUring::new(8).unwrap();
    let buf = [99u8; 48];

    {
        let entry = opcode::SendZc::new(Fd(3), buf.as_ptr(), buf.len() as u32)
            .build()
            .user_data(1104);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 48);
}

#[test]
fn sendmsg_reads_msghdr() {
    let mut ring = IoUring::new(8).unwrap();

    // Build a minimal msghdr with one iovec
    let buf = [42u8; 32];
    let iov = opcode::libc_iovec {
        iov_base: buf.as_ptr() as *mut _,
        iov_len: buf.len(),
    };

    // Use repr(C) layout matching libc::msghdr
    #[repr(C)]
    struct MsgHdr {
        msg_name: *mut u8,
        msg_namelen: u32,
        _pad0: u32,
        msg_iov: *const opcode::libc_iovec,
        msg_iovlen: usize,
        msg_control: *mut u8,
        msg_controllen: usize,
        msg_flags: i32,
        _pad1: u32,
    }

    let hdr = MsgHdr {
        msg_name: std::ptr::null_mut(),
        msg_namelen: 0,
        _pad0: 0,
        msg_iov: &iov as *const _ as *const _,
        msg_iovlen: 1,
        msg_control: std::ptr::null_mut(),
        msg_controllen: 0,
        msg_flags: 0,
        _pad1: 0,
    };

    {
        let entry = opcode::SendMsg::new(Fd(3), &hdr as *const _ as *const _)
            .build()
            .user_data(1105);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 32);
}

#[test]
fn recvmsg_writes_msghdr() {
    let mut ring = IoUring::new(8).unwrap();

    let mut buf = [0xFFu8; 64];
    let iov = opcode::libc_iovec {
        iov_base: buf.as_mut_ptr() as *mut _,
        iov_len: buf.len(),
    };

    #[repr(C)]
    struct MsgHdr {
        msg_name: *mut u8,
        msg_namelen: u32,
        _pad0: u32,
        msg_iov: *const opcode::libc_iovec,
        msg_iovlen: usize,
        msg_control: *mut u8,
        msg_controllen: usize,
        msg_flags: i32,
        _pad1: u32,
    }

    let hdr = MsgHdr {
        msg_name: std::ptr::null_mut(),
        msg_namelen: 0,
        _pad0: 0,
        msg_iov: &iov as *const _ as *const _,
        msg_iovlen: 1,
        msg_control: std::ptr::null_mut(),
        msg_controllen: 0,
        msg_flags: 0,
        _pad1: 0,
    };

    {
        let entry = opcode::RecvMsg::new(Fd(3), &hdr as *const _ as *mut _)
            .build()
            .user_data(1106);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 64);
    assert!(buf.iter().all(|&b| b == 0));
}

// ===========================================================================
// Xattr ops
// ===========================================================================

#[test]
fn fsetxattr_reads_name_and_value() {
    let mut ring = IoUring::new(8).unwrap();
    let name = b"user.test\0";
    let value = b"hello";

    {
        let entry = opcode::FSetXattr::new(
            Fd(3),
            name.as_ptr(),
            value.as_ptr() as *const _,
            value.len() as u32,
        )
        .build()
        .user_data(1200);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn fgetxattr_reads_name_writes_value() {
    let mut ring = IoUring::new(8).unwrap();
    let name = b"user.test\0";
    let mut value = [0xFFu8; 32];

    {
        let entry = opcode::FGetXattr::new(
            Fd(3),
            name.as_ptr(),
            value.as_mut_ptr() as *mut _,
            value.len() as u32,
        )
        .build()
        .user_data(1201);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 32); // returns len
    assert!(value.iter().all(|&b| b == 0));
}

#[test]
fn setxattr_reads_name_value_path() {
    let mut ring = IoUring::new(8).unwrap();
    let name = b"user.test\0";
    let value = b"data";
    let path = b"/tmp/file\0";

    {
        let entry = opcode::SetXattr::new(
            name.as_ptr(),
            value.as_ptr() as *const _,
            path.as_ptr(),
            value.len() as u32,
        )
        .build()
        .user_data(1202);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn getxattr_reads_name_path_writes_value() {
    let mut ring = IoUring::new(8).unwrap();
    let name = b"user.test\0";
    let mut value = [0xFFu8; 16];
    let path = b"/tmp/file\0";

    {
        let entry = opcode::GetXattr::new(
            name.as_ptr(),
            value.as_mut_ptr() as *mut _,
            path.as_ptr(),
            value.len() as u32,
        )
        .build()
        .user_data(1203);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 16);
    assert!(value.iter().all(|&b| b == 0));
}

// ===========================================================================
// Futex ops
// ===========================================================================

#[test]
fn futex_wait_reads_word() {
    let mut ring = IoUring::new(8).unwrap();
    let futex_word: u32 = 42;

    {
        let entry = opcode::FutexWait::new(&futex_word as *const u32, 0, 0xFFFFFFFF, 0)
            .build()
            .user_data(1300);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn futex_wake_reads_word() {
    let mut ring = IoUring::new(8).unwrap();
    let futex_word: u32 = 0;

    {
        let entry = opcode::FutexWake::new(&futex_word as *const u32, 1, 0xFFFFFFFF, 0)
            .build()
            .user_data(1301);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn futex_waitv_reads_array() {
    let mut ring = IoUring::new(8).unwrap();
    let entries = [
        miring::types::FutexWaitV::new().val(1).uaddr(0x1000).flags(0),
        miring::types::FutexWaitV::new().val(2).uaddr(0x2000).flags(0),
    ];

    {
        let entry = opcode::FutexWaitV::new(entries.as_ptr(), entries.len() as u32)
            .build()
            .user_data(1302);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

// ===========================================================================
// Misc ops
// ===========================================================================

#[test]
fn epoll_ctl_reads_event() {
    let mut ring = IoUring::new(8).unwrap();
    // 12-byte epoll_event: 4 bytes events + 8 bytes data
    let ev = [0u8; 12];

    {
        let entry = opcode::EpollCtl::new(
            Fd(5),
            3, // target fd
            1, // EPOLL_CTL_ADD
            ev.as_ptr() as *const _,
        )
        .build()
        .user_data(1400);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn epoll_wait_writes_events() {
    let mut ring = IoUring::new(8).unwrap();
    let mut events = [0xFFu8; 48]; // 4 * 12 bytes

    {
        let entry = opcode::EpollWait::new(
            Fd(5),
            events.as_mut_ptr() as *mut _,
            4, // max_events
        )
        .build()
        .user_data(1401);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0); // 0 events returned
    assert!(events.iter().all(|&b| b == 0));
}

#[test]
fn pipe_writes_two_fds() {
    let mut ring = IoUring::new(8).unwrap();
    let mut fds = [-1i32; 2];

    {
        let entry = opcode::Pipe::new(fds.as_mut_ptr())
            .build()
            .user_data(1402);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
    assert_eq!(fds[0], 42);
    assert_eq!(fds[1], 43);
}

#[test]
fn waitid_writes_siginfo() {
    let mut ring = IoUring::new(8).unwrap();
    let mut siginfo = [0xFFu8; 128];

    {
        let entry = opcode::WaitId::new(1, 0, 0) // P_PID=1, id=0
            .infop(siginfo.as_mut_ptr() as *const _)
            .build()
            .user_data(1403);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
    assert!(siginfo.iter().all(|&b| b == 0));
}

#[test]
fn fixed_fd_install_returns_fd() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let entry = opcode::FixedFdInstall::new(miring::types::Fixed(0), 0)
            .build()
            .user_data(1404);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 42); // fake fd
}

#[test]
fn provide_buffers_validates_range() {
    let mut ring = IoUring::new(8).unwrap();
    let mut bufs = vec![0u8; 256]; // 4 buffers of 64 bytes

    {
        let entry = opcode::ProvideBuffers::new(bufs.as_mut_ptr(), 64, 4, 1, 0)
            .build()
            .user_data(1405);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
}

#[test]
fn files_update_reads_fd_array() {
    let mut ring = IoUring::new(8).unwrap();
    let fds = [3i32, 4, 5, -1];

    {
        let entry = opcode::FilesUpdate::new(fds.as_ptr(), fds.len() as u32)
            .build()
            .user_data(1406);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 4); // returns count
}

// ===========================================================================
// Fixed-buffer vectored ops
// ===========================================================================

#[test]
fn readv_fixed_fills_scatter_buffers() {
    let mut ring = IoUring::new(8).unwrap();
    let mut buf1 = [0xFFu8; 32];
    let mut buf2 = [0xFFu8; 16];

    let iovecs = [
        opcode::libc_iovec {
            iov_base: buf1.as_mut_ptr() as *mut _,
            iov_len: buf1.len(),
        },
        opcode::libc_iovec {
            iov_base: buf2.as_mut_ptr() as *mut _,
            iov_len: buf2.len(),
        },
    ];

    {
        let entry = opcode::ReadvFixed::new(Fd(0), iovecs.as_ptr(), 2, 0)
            .build()
            .user_data(1500);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 48);
    assert!(buf1.iter().all(|&b| b == 0));
    assert!(buf2.iter().all(|&b| b == 0));
}

#[test]
fn writev_fixed_reads_gather_buffers() {
    let mut ring = IoUring::new(8).unwrap();
    let buf1 = [1u8; 20];
    let buf2 = [2u8; 10];

    let iovecs = [
        opcode::libc_iovec {
            iov_base: buf1.as_ptr() as *mut _,
            iov_len: buf1.len(),
        },
        opcode::libc_iovec {
            iov_base: buf2.as_ptr() as *mut _,
            iov_len: buf2.len(),
        },
    ];

    {
        let entry = opcode::WritevFixed::new(Fd(1), iovecs.as_ptr(), 2, 0)
            .build()
            .user_data(1501);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 30);
}

// ===========================================================================
// Multishot convenience builders — verify correct SQE construction
// ===========================================================================

#[test]
fn recv_multi_builds_correct_opcode() {
    let entry = opcode::RecvMulti::new(Fd(3), 7).build();
    assert_eq!(entry.get_opcode(), opcode::Recv::CODE as u32);
}

#[test]
fn accept_multi_builds_correct_opcode() {
    let entry = opcode::AcceptMulti::new(Fd(3)).build();
    assert_eq!(entry.get_opcode(), opcode::Accept::CODE as u32);
}

#[test]
fn recv_msg_multi_builds_correct_opcode() {
    let hdr = std::ptr::null();
    let entry = opcode::RecvMsgMulti::new(Fd(3), hdr, 5).build();
    assert_eq!(entry.get_opcode(), opcode::RecvMsg::CODE as u32);
}

#[test]
fn read_multi_builds_correct_opcode() {
    let entry = opcode::ReadMulti::new(Fd(0), 4096, 3).build();
    assert_eq!(entry.get_opcode(), opcode::ReadMulti::CODE as u32);
}

#[test]
fn recv_bundle_builds_correct_opcode() {
    let entry = opcode::RecvBundle::new(Fd(3), 2).build();
    assert_eq!(entry.get_opcode(), opcode::Recv::CODE as u32);
}

#[test]
fn send_bundle_builds_correct_opcode() {
    let entry = opcode::SendBundle::new(Fd(3), 2).build();
    assert_eq!(entry.get_opcode(), opcode::Send::CODE as u32);
}

#[test]
fn recv_multi_bundle_builds_correct_opcode() {
    let entry = opcode::RecvMultiBundle::new(Fd(3), 4).build();
    assert_eq!(entry.get_opcode(), opcode::Recv::CODE as u32);
}

// ===========================================================================
// Linked operations
// ===========================================================================

#[test]
fn linked_ops_cancel_on_failure() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let mut sq = ring.submission();
        // First NOP: inject -1 (failure), linked to next
        let nop1 = opcode::Nop::new()
            .inject_result(-1i32 as u32)
            .build()
            .user_data(1)
            .flags(squeue::Flags::IO_LINK);
        unsafe { sq.push(&nop1).unwrap() };

        // Second NOP: should get -ECANCELED (-125)
        let nop2 = opcode::Nop::new().build().user_data(2);
        unsafe { sq.push(&nop2).unwrap() };
    }

    ring.submit().unwrap();
    let mut cq = ring.completion();
    let cqe1 = cq.next().unwrap();
    assert_eq!(cqe1.user_data(), 1);
    assert_eq!(cqe1.result(), -1);

    let cqe2 = cq.next().unwrap();
    assert_eq!(cqe2.user_data(), 2);
    assert_eq!(cqe2.result(), -125); // -ECANCELED
}

#[test]
fn hardlinked_ops_continue_on_failure() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let mut sq = ring.submission();
        // First NOP: inject -1 (failure), hardlinked to next
        let nop1 = opcode::Nop::new()
            .inject_result(-1i32 as u32)
            .build()
            .user_data(1)
            .flags(squeue::Flags::IO_HARDLINK);
        unsafe { sq.push(&nop1).unwrap() };

        // Second NOP: should still execute (hardlink ignores predecessor failure)
        let nop2 = opcode::Nop::new().build().user_data(2);
        unsafe { sq.push(&nop2).unwrap() };
    }

    ring.submit().unwrap();
    let mut cq = ring.completion();
    let cqe1 = cq.next().unwrap();
    assert_eq!(cqe1.user_data(), 1);
    assert_eq!(cqe1.result(), -1);

    let cqe2 = cq.next().unwrap();
    assert_eq!(cqe2.user_data(), 2);
    assert_eq!(cqe2.result(), 0); // NOT cancelled
}

#[test]
fn link_chain_propagates_cancel() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let mut sq = ring.submission();
        // A (fail, linked) -> B (linked) -> C
        let a = opcode::Nop::new()
            .inject_result(-1i32 as u32)
            .build()
            .user_data(1)
            .flags(squeue::Flags::IO_LINK);
        let b = opcode::Nop::new()
            .build()
            .user_data(2)
            .flags(squeue::Flags::IO_LINK);
        let c = opcode::Nop::new().build().user_data(3);

        unsafe {
            sq.push(&a).unwrap();
            sq.push(&b).unwrap();
            sq.push(&c).unwrap();
        };
    }

    ring.submit().unwrap();
    let mut cq = ring.completion();
    let cqe_a = cq.next().unwrap();
    assert_eq!(cqe_a.user_data(), 1);
    assert_eq!(cqe_a.result(), -1);

    let cqe_b = cq.next().unwrap();
    assert_eq!(cqe_b.user_data(), 2);
    assert_eq!(cqe_b.result(), -125); // -ECANCELED

    let cqe_c = cq.next().unwrap();
    assert_eq!(cqe_c.user_data(), 3);
    assert_eq!(cqe_c.result(), -125); // -ECANCELED
}

#[test]
fn linked_ops_succeed_when_no_failure() {
    let mut ring = IoUring::new(8).unwrap();

    {
        let mut sq = ring.submission();
        // Two linked NOPs that succeed
        let nop1 = opcode::Nop::new()
            .build()
            .user_data(1)
            .flags(squeue::Flags::IO_LINK);
        let nop2 = opcode::Nop::new().build().user_data(2);
        unsafe {
            sq.push(&nop1).unwrap();
            sq.push(&nop2).unwrap();
        };
    }

    ring.submit().unwrap();
    let mut cq = ring.completion();
    assert_eq!(cq.next().unwrap().result(), 0);
    assert_eq!(cq.next().unwrap().result(), 0);
}

// ===========================================================================
// Probe accuracy
// ===========================================================================

#[test]
fn probe_reports_handled_ops_only() {
    let ring = IoUring::new(8).unwrap();
    let mut probe = miring::Probe::new();
    ring.submitter().register_probe(&mut probe).unwrap();

    // These should be supported
    assert!(probe.is_supported(0)); // NOP
    assert!(probe.is_supported(22)); // READ
    assert!(probe.is_supported(23)); // WRITE
    assert!(probe.is_supported(27)); // RECV
    assert!(probe.is_supported(26)); // SEND
    assert!(probe.is_supported(11)); // TIMEOUT
    assert!(probe.is_supported(13)); // ACCEPT
    assert!(probe.is_supported(18)); // OPENAT
    assert!(probe.is_supported(19)); // CLOSE
    assert!(probe.is_supported(45)); // SOCKET
    assert!(probe.is_supported(62)); // PIPE

    // IORING_OP_LAST (63) and beyond should NOT be supported
    assert!(!probe.is_supported(63));
}

// ===========================================================================
// OpenAt / OpenAt2 / Statx
// ===========================================================================

#[test]
fn openat_reads_pathname_returns_fd() {
    let mut ring = IoUring::new(8).unwrap();
    let path = b"/tmp/test\0";

    {
        let entry = opcode::OpenAt::new(Fd(-1), path.as_ptr())
            .build()
            .user_data(1600);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 42); // fake fd
}

#[test]
fn openat2_reads_pathname_and_how() {
    let mut ring = IoUring::new(8).unwrap();
    let path = b"/tmp/test2\0";
    let how = miring::types::OpenHow::new().flags(0).mode(0o644);

    {
        let entry = opcode::OpenAt2::new(Fd(-1), path.as_ptr(), &how as *const _ as *const _)
            .build()
            .user_data(1601);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 42);
}

#[test]
fn statx_reads_pathname_writes_buffer() {
    let mut ring = IoUring::new(8).unwrap();
    let path = b"/tmp/stat\0";
    let mut statxbuf = [0xFFu8; 256];

    {
        let entry = opcode::Statx::new(
            Fd(-1),
            path.as_ptr(),
            statxbuf.as_mut_ptr() as *mut _,
        )
        .build()
        .user_data(1602);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.result(), 0);
    assert!(statxbuf.iter().all(|&b| b == 0));
}

// ===========================================================================
// Timeout variants
// ===========================================================================

#[test]
fn timeout_update_builds_correct_opcode() {
    let ts = miring::types::Timespec::new().sec(5);
    let entry = opcode::TimeoutUpdate::new(0xABCD, &ts as *const _ as *const _).build();
    // TimeoutUpdate uses TIMEOUT_REMOVE opcode with IORING_TIMEOUT_UPDATE flag
    assert_eq!(entry.get_opcode(), opcode::TimeoutRemove::CODE as u32);
}

// ===========================================================================
// UringCmd
// ===========================================================================

#[test]
fn uring_cmd16_builds_correct_opcode() {
    let cmd = [0u8; 16];
    let entry = opcode::UringCmd16::new(Fd(3), 42).cmd(cmd).build();
    assert_eq!(entry.get_opcode(), opcode::UringCmd16::CODE as u32);
}

#[test]
fn uring_cmd80_builds_entry128() {
    let cmd = [0u8; 80];
    let entry = opcode::UringCmd80::new(Fd(3), 99).cmd(cmd).build();
    assert_eq!(entry.get_opcode(), opcode::UringCmd80::CODE as u32);
}

// ===========================================================================
// UringCmd dispatches as no-op success
// ===========================================================================

#[test]
fn uring_cmd16_dispatches_success() {
    let mut ring = IoUring::new(8).unwrap();
    let cmd = [0u8; 16];

    {
        let entry = opcode::UringCmd16::new(Fd(3), 0).cmd(cmd)
            .build()
            .user_data(1800);
        unsafe { ring.submission().push(&entry).unwrap() };
    }
    ring.submit().unwrap();
    let cqe = ring.completion().next().unwrap();
    assert_eq!(cqe.user_data(), 1800);
    assert_eq!(cqe.result(), 0);
}

// ===========================================================================
// AsyncCancel2 with CancelBuilder
// ===========================================================================

#[test]
fn async_cancel2_builds_correct_opcode() {
    let builder = miring::types::CancelBuilder::user_data(0x1234);
    let entry = opcode::AsyncCancel2::new(builder).build();
    assert_eq!(entry.get_opcode(), opcode::AsyncCancel::CODE as u32);
}

#[test]
fn async_cancel2_any_builds_correct_opcode() {
    let builder = miring::types::CancelBuilder::any();
    let entry = opcode::AsyncCancel2::new(builder).build();
    assert_eq!(entry.get_opcode(), opcode::AsyncCancel::CODE as u32);
}

// ---------------------------------------------------------------------------
// RecvMsgOut::parse
// ---------------------------------------------------------------------------

#[test]
fn recvmsg_out_parse_basic() {
    use miring::types::RecvMsgOut;

    // Build a synthetic buffer matching the kernel's RecvMsgMulti output:
    //   [io_uring_recvmsg_out (16B)] [name (28B)] [payload]
    //
    // io_uring_recvmsg_out fields (all u32, little-endian):
    //   namelen, controllen, payloadlen, flags
    let name_bytes: &[u8] = &[0x02, 0x00, 0x1F, 0x90, 127, 0, 0, 1, // AF_INET, port 8080, 127.0.0.1
                               0, 0, 0, 0, 0, 0, 0, 0,               // padding to 16B for sockaddr_in
                               0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];  // padding to 28B (msghdr_name_len)
    let payload: &[u8] = b"QUIC datagram payload";

    let mut buf = Vec::new();
    // Header: namelen=16, controllen=0, payloadlen=21, flags=0
    buf.extend_from_slice(&16u32.to_ne_bytes()); // namelen (actual name data = 16B sockaddr_in)
    buf.extend_from_slice(&0u32.to_ne_bytes());  // controllen
    buf.extend_from_slice(&(payload.len() as u32).to_ne_bytes()); // payloadlen
    buf.extend_from_slice(&0u32.to_ne_bytes());  // flags
    buf.extend_from_slice(name_bytes);           // 28B of name field
    buf.extend_from_slice(payload);

    let msg = RecvMsgOut::parse(&buf, 28, 0).expect("parse failed");
    assert_eq!(msg.incoming_name_len(), 16);
    assert_eq!(msg.name_data().len(), 16); // min(namelen=16, msghdr_name_len=28)
    assert!(!msg.is_name_data_truncated());
    assert_eq!(msg.control_data().len(), 0);
    assert_eq!(msg.payload_data(), payload);
    assert_eq!(msg.incoming_payload_len(), payload.len() as u32);
    assert_eq!(msg.flags(), 0);
}

#[test]
fn recvmsg_out_parse_truncated_name() {
    use miring::types::RecvMsgOut;

    // Simulate name truncation: kernel says namelen=100 but we only have 8B field
    let mut buf = Vec::new();
    buf.extend_from_slice(&100u32.to_ne_bytes()); // namelen (incoming was 100B)
    buf.extend_from_slice(&0u32.to_ne_bytes());
    buf.extend_from_slice(&5u32.to_ne_bytes());   // payloadlen
    buf.extend_from_slice(&0u32.to_ne_bytes());
    buf.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]); // 8B name field
    buf.extend_from_slice(b"hello");

    let msg = RecvMsgOut::parse(&buf, 8, 0).expect("parse failed");
    assert!(msg.is_name_data_truncated());
    assert_eq!(msg.name_data().len(), 8); // min(100, 8) = 8
    assert_eq!(msg.payload_data(), b"hello");
}

#[test]
fn recvmsg_out_parse_too_short_buffer() {
    use miring::types::RecvMsgOut;

    // Buffer too short to contain the header + name + control
    let buf = [0u8; 10]; // less than 16B header
    assert!(RecvMsgOut::parse(&buf, 28, 0).is_err());
}
