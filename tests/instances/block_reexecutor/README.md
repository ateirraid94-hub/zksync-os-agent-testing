# block_reexecutor

Re-executes a single L2 block against RPC state, validates transaction outputs against RPC receipts, and writes call traces.

## What it does

- Loads block data and metadata from RPC (or from disk cache).
- Executes transactions with `RpcValueOracleFactory`.
- Compares execution results against RPC receipts:
  - success/failure status
  - gas used
  - logs count/content
- Runs REVM on the same block context/state.
- Writes two trace files in geth call tracer format (`CallFrame[]`).

## Run

```bash
RUST_LOG=info cargo run -p block_reexecutor -- --endpoint <rpc> --block-hash <block_hash>
```

## Output files

- `tracer_output_<block_number>.json`
- `revm_call_trace_<block_number>.json`

## Cache files

All cache files are stored under:

- `.cache/block_reexecutor/`

If you need a full refetch, delete the cache.
