# WORKING

Short doc to keep track of what saint0x is working on.

Note: This doc can be removed if it becomes noisy or unnecessary.

- [x] ✅ Doc alignment
- [ ] Set up Fozzy
- [ ] SetSockOpt implementation
  - What/why: Implement dispatch-side memory touching for SetSockOpt so Miri can validate pointer/lifetime correctness on this path.
  - Architecture note: Follow existing opcode-builder + dispatch handler structure and preserve current emulation boundaries.
