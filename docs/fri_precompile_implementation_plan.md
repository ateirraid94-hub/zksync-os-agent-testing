# FRI Precompile (`0x7c` / `0x12`) — Full Implementation Plan

## Architecture Overview

```
User/L1 → zksync-os-server (RPC + mempool)
              ↓  encodes 0x7c tx into block
         zksync-os (bootloader / EVM kernel, RISC-V)
              ↓  calls precompile 0x12 → CSRRW 0x7CC
         zksync-airbender (delegation circuit proves CSRRW)
              ↓  consistency check
         zksync-os-revm (re-executes 0x7c for determinism check)
              ↓  final proof artifact
         (L1 contracts / protocol — out of scope for initial impl)
```

## Repository Roles

| Repository | Role | Critical Path |
|---|---|---|
| `zksync-os` | Bootloader, EVM kernel, block processing | Yes |
| `zksync-os-server` | Sequencer — RPC, mempool, block building | Yes |
| `zksync-os-revm` | REVM consistency checker for replay | Yes |
| `zksync-airbender` | RISC-V STARK/FRI proving backend | Yes (for provability) |
| `zksync-protocol` | Protocol version definition | No (follow-on) |
| `era-contracts` | L1 contract tx type constants | No (follow-on) |
| `zkevm_circuits` / `zk_evm` | Legacy path — not applicable | No |

## Change Summary

| Repository | Files Changed | Effort |
|---|---|---|
| `zksync-os` | 20+ | Large |
| `zksync-os-server` | ~9 | Medium |
| `zksync-os-revm` | ~5 | Small |
| `zksync-airbender` | ~8 areas + circuit generation | Very Large |
