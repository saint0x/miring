# miring

Pure-Rust io\_uring emulator. API-compatible with [`io-uring`](https://crates.io/crates/io-uring) 0.7, runs entirely in userspace, and works under [Miri](https://github.com/rust-lang/miri) for undefined-behavior detection.

## Why

Code that targets `io_uring` on Linux is notoriously difficult to test:

- The real kernel interface is only available on Linux 5.1+
- Miri cannot execute syscalls, so any `io_uring` code is invisible to it
- Pointer-heavy SQE/CQE buffers are a common source of UB that goes undetected

miring replaces the kernel with a cooperative userspace emulator. The SQ/CQ ring protocol and completion semantics are faithfully reproduced using heap allocations and atomics — giving Miri full visibility into every pointer dereference your `io_uring` code makes.

## Quick start

```toml
[target.'cfg(not(miri))'.dependencies]
io-uring = "0.7"

[target.'cfg(miri)'.dependencies]
miring = "0.1"
```

```rust
#[cfg(not(miri))]
use io_uring;
#[cfg(miri)]
use miring as io_uring;
```

Your code uses `io_uring::opcode`, `io_uring::types`, etc. as normal. Under Miri, it transparently switches to the emulator.

## What's emulated

| Feature | Status |
|---------|--------|
| SQ/CQ ring protocol (push, submit, complete, overflow) | Emulated |
| Opcode builders (full io-uring 0.7.11 parity) | Emulated |
| Dispatch handlers that touch user memory | Emulated |
| Linked operations (`IO_LINK`, `IO_HARDLINK`) | Emulated |
| `SKIP_SUCCESS` (CQE suppression) | Emulated |
| Buffer selection (`IOSQE_BUFFER_SELECT` + CQE flags) | Emulated |
| `RecvMsgOut::parse` (multishot recvmsg buffer parsing) | Emulated |
| `Probe` (reports actually-handled ops) | Emulated |
| `Entry128` / 32-byte CQEs | Emulated |
| Builder setup flags (`coop_taskrun`, `single_issuer`, ...) | Accepted (no-op) |
| Actual I/O (disk, network, etc.) | Not emulated |

The emulator does not perform real I/O. Instead, each dispatch handler walks the same user-memory pointers the kernel would — reads from write buffers, writes sentinel bytes into read buffers — so that Miri can detect any UB in your buffer management, lifetime handling, or pointer arithmetic.

## Opcode coverage

All builder structs from `io-uring` 0.7.11 are implemented:

`Nop`, `Read`, `Write`, `Readv`, `Writev`, `ReadFixed`, `WriteFixed`, `Fsync`, `PollAdd`, `PollRemove`, `SyncFileRange`, `SendMsg`, `RecvMsg`, `Timeout`, `TimeoutRemove`, `TimeoutUpdate`, `Accept`, `AcceptMulti`, `AsyncCancel`, `AsyncCancel2`, `LinkTimeout`, `Connect`, `Fallocate`, `OpenAt`, `OpenAt2`, `Close`, `Statx`, `FilesUpdate`, `ProvideBuffers`, `RemoveBuffers`, `Send`, `Recv`, `RecvMulti`, `RecvBundle`, `RecvMultiBundle`, `RecvMsgMulti`, `SendZc`, `SendMsgZc`, `SendBundle`, `RecvZc`, `Splice`, `Tee`, `Shutdown`, `RenameAt`, `UnlinkAt`, `MkDirAt`, `SymlinkAt`, `LinkAt`, `MsgRing`, `MsgRingSendFd`, `Socket`, `Bind`, `Listen`, `Ftruncate`, `Fadvise`, `Madvise`, `EpollCtl`, `EpollWait`, `ReadMulti`, `GetXattr`, `SetXattr`, `FGetXattr`, `FSetXattr`, `UringCmd16`, `UringCmd80`, `SetSockOpt`, `FutexWait`, `FutexWake`, `FutexWaitV`, `WaitId`, `FixedFdInstall`, `ReadvFixed`, `WritevFixed`, `Pipe`

## Loom support

miring's atomic operations go through a shim that swaps in [Loom](https://crates.io/crates/loom) primitives under `cfg(loom)`, enabling model-checked verification of the ring protocol's concurrency:

```sh
RUSTFLAGS='--cfg loom' cargo test --release --lib loom_
```

## Testing

```sh
cargo test
cargo +nightly miri test
```

## Limitations

- **No real I/O.** Dispatch handlers validate buffer pointers and simulate results (e.g. `Read` fills the buffer with `0`, `OpenAt` returns fd 42). They do not interact with the filesystem, network, or any kernel subsystem.
- **Deferred dispatch.** Completions are produced by background dispatch threads that join on completion reaping — this is sufficient for Miri and Loom but does not model kernel-level concurrency.
- **No `io_uring_register` file/buffer tables.** `register_buffers`, `register_files`, etc. are accepted but don't change dispatch behavior.
- **Setup flags are no-ops.** `setup_sqpoll`, `setup_iopoll`, `setup_coop_taskrun`, etc. are accepted by the builder for API compatibility but have no effect on emulation.

## License

MIT OR Apache-2.0
