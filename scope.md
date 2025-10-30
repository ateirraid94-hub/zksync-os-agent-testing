# Scope for bug bounty

This document describes the scope for the bug bounty.

## Impact/severity

## Coarse-grained scope

The crates part of the scope are:

- [basic_bootloader](basic_bootloader/Cargo.toml)
- [basic_system](basic_system/Cargo.toml)
- [callable_oracles](callable_oracles/Cargo.toml)
- [crypto](crypto/Cargo.toml)
- [evm_interpreter](evm_interpreter/Cargo.toml)
- [oracle_provider](oracle_provider/Cargo.toml)
- [proof_running_system](proof_running_system/Cargo.toml)
- [storage_models](storage_models/Cargo.toml)
- [supporting_crates/delegated_u255](supporting_crates/delegated_u256/Cargo.toml)
- [supporting_crates/modexp](supporting_crates/modexp/Cargo.toml)
- [supporting_crates/u256](supporting_crates/u256/Cargo.toml)
- [system_hooks](system_hooks/Cargo.toml)
- [zk_ee](zk_ee/Cargo.toml)
- [zksync_os](zksync_os/Cargo.toml)

## Fine-grained scope

This section limits the scope of some of the crates mentioned in the previous section. If a crate from the previous section is not present in this one, all the modules from that crate are part of the scope.

### basic_system

We exclude from the scope the [ethereum_storage_model](basic_system/src/system_implementation/ethereum_storage_model/mod.rs) module, as for production we only use the flat storage model for now.

### callable_oracles

We exclude from the scope the [hash_to_prime](callable_oracles/src/hash_to_prime/mod.rs) modules, as it's unused.

## Enabled features

The featureset that should be considered for the scope is the one that corresponds to the `production` and `multiblock-batch` features defined in the [proof_running_system](proof_running_system/Cargo.toml). Any other code under disabled features is considered out of scope.
