# zkip

This template contains:

- `.gitignore`: ignores local build artifacts
- `guest/`: Airbender guest program
- `host/`: host-side runner/prover application
- `guest/.cargo/config.toml`: guest target and build flags for local tooling

## Quick Start

Build guest artifacts:

```sh
cd guest
cargo airbender build
```

Run host execution:

```sh
cd ../host
cargo run
```

Run host execution + proof:

```sh
cargo run -- --prove
```

## Prover Backend

Default prover backend: `dev`.

`dev` mode does not run cryptographic proving; it emits a mock proof envelope and is ideal for development.

