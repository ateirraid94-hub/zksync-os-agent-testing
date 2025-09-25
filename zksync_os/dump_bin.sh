#!/bin/sh
set -e

USAGE="Usage: $0 --type {server|server-logging-enabled|debug-in-simulator|evm-replay|evm-replay-benchmarking|pectra|multiblock-batch|multiblock-batch-logging-enabled|evm-tester|for-tests}"
TYPE=""

# Parse --type argument
while [ "$#" -gt 0 ]; do
  case "$1" in
    --type)
      [ "$#" -ge 2 ] || { echo "Missing value for --type"; echo "$USAGE"; exit 2; }
      TYPE="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1"
      echo "$USAGE"
      exit 2
      ;;
  esac
done

# Base features and output names
FEATURES="proving"

# Adjust for server modes
case "$TYPE" in
  server)
    FEATURES="$FEATURES"
    BIN_NAME="server_app.bin"
    ELF_NAME="server_app.elf"
    TEXT_NAME="server_app.text"
    ;;
  server-logging-enabled)
    FEATURES="$FEATURES,print_debug_info"
    BIN_NAME="server_app_logging_enabled.bin"
    ELF_NAME="server_app_logging_enabled.elf"
    TEXT_NAME="server_app_logging_enabled.text"
    ;;
  debug-in-simulator)
    FEATURES="$FEATURES,print_debug_info,proof_running_system/cycle_marker,proof_running_system/p256_precompile,proof_running_system/state-diffs-pi"
    BIN_NAME="app_debug.bin"
    ELF_NAME="app_debug.elf"
    TEXT_NAME="app_debug.text"
    ;;
  evm-replay)
    FEATURES="$FEATURES,proof_running_system/disable_system_contracts,proof_running_system/prevrandao,proof_running_system/evm_refunds,proof_running_system/state-diffs-pi"
    BIN_NAME="evm_replay.bin"
    ELF_NAME="evm_replay.elf"
    TEXT_NAME="evm_replay.text"
    ;;
  evm-replay-benchmarking)
    FEATURES="$FEATURES,proof_running_system/unlimited_native,proof_running_system/disable_system_contracts,proof_running_system/cycle_marker,proof_running_system/prevrandao,proof_running_system/evm_refunds,proof_running_system/state-diffs-pi"
    BIN_NAME="evm_replay.bin"
    ELF_NAME="evm_replay.elf"
    TEXT_NAME="evm_replay.text"
    ;;
  pectra)
    FEATURES="$FEATURES,pectra,state-diffs-pi"
    BIN_NAME="pectra.bin"
    ELF_NAME="pectra.elf"
    TEXT_NAME="pectra.text"
    ;;
  multiblock-batch)
    FEATURES="$FEATURES,proof_running_system/multiblock-batch"
    BIN_NAME="multiblock_batch.bin"
    ELF_NAME="multiblock_batch.elf"
    TEXT_NAME="multiblock_batch.text"
    ;;
  multiblock-batch-logging-enabled)
    FEATURES="$FEATURES,proof_running_system/multiblock-batch,print_debug_info"
    BIN_NAME="multiblock_batch_logging_enabled.bin"
    ELF_NAME="multiblock_batch_logging_enabled.elf"
    TEXT_NAME="multiblock_batch_logging_enabled.text"
    ;;
  evm-tester)
    FEATURES="$FEATURES,proof_running_system/state-diffs-pi,proof_running_system/resources_for_tester,proof_running_system/prevrandao,proof_running_system/pectra,proof_running_system/p256_precompile",
    BIN_NAME="evm_tester.bin"
    ELF_NAME="evm_tester.elf"
    TEXT_NAME="evm_tester.text"
    ;;
  for-tests)
    FEATURES="$FEATURES,proof_running_system/state-diffs-pi,proof_running_system/p256_precompile,proof_running_system/cycle_marker",proof_running_system/point_eval_precompile,
    BIN_NAME="for_tests.bin"
    ELF_NAME="for_tests.elf"
    TEXT_NAME="for_tests.text"
    ;;
  *)
    echo "Invalid --type: $TYPE"
    echo "$USAGE"
    exit 1
    ;;
esac

# Clean up only the artifacts for this mode
rm -f "$BIN_NAME" "$ELF_NAME" "$TEXT_NAME"

# Build
cargo build --features "$FEATURES" --release

# Produce and rename outputs
cargo objcopy --features "$FEATURES" --release -- -O binary "$BIN_NAME"
cargo objcopy --features "$FEATURES" --release -- -R .text "$ELF_NAME"
cargo objcopy --features "$FEATURES" --release -- -O binary --only-section=.text "$TEXT_NAME"

# Summary
echo "Built [$TYPE] with features: $FEATURES"
echo "→ $BIN_NAME"
echo "→ $ELF_NAME"
echo "→ $TEXT_NAME"
