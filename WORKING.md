# WORKING

Short doc to keep track of what saint0x is working on.

Note: This doc can be removed if it becomes noisy or unnecessary.

1. ✅ Doc alignment
- What/why: Align README claims with current implementation so contributors and users have an accurate baseline.
- Architecture note: Documentation updates only; no behavioral changes to the emulator design.

2. Set up Fozzy
- What/why: Add deterministic Fozzy scenarios and trace validation to improve reproducible system/regression testing early.
- Architecture note: Testing harness work around the existing ring/dispatch model without changing core APIs.

3. SetSockOpt implementation
- What/why: Implement dispatch-side memory touching for SetSockOpt so Miri can validate pointer/lifetime correctness on this path.
- Architecture note: Follow existing opcode-builder + dispatch handler structure and preserve current emulation boundaries.
