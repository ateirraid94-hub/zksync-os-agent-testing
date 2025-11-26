use crate::system::system::*;
use basic_bootloader::bootloader::config::BasicBootloaderExecutionConfig;
use basic_bootloader::bootloader::errors::BootloaderSubsystemError;
use basic_bootloader::bootloader::result_keeper::ResultKeeperExt;
use basic_system::system_implementation::system::BatchOutput;
use oracle_provider::DummyMemorySource;
use oracle_provider::ReadWitnessSource;
use oracle_provider::ZkEENonDeterminismSource;
use zk_ee::system::tracer::Tracer;

///
/// Run bootloader with forward system with a given `oracle`.
/// Returns execution results(tx results, state changes, events, etc) via `results_keeper`.
///
pub fn run_forward<Config: BasicBootloaderExecutionConfig>(
    oracle: ZkEENonDeterminismSource<DummyMemorySource>,
    result_keeper: &mut impl ResultKeeperExt,
    tracer: &mut impl Tracer<ForwardRunningSystem>,
) {
    if let Err(err) = ForwardBootloader::run_prepared::<Config>(oracle, result_keeper, tracer) {
        panic!("Forward run failed with: {err}")
    };
}

pub fn run_forward_no_panic<Config: BasicBootloaderExecutionConfig>(
    oracle: ZkEENonDeterminismSource<DummyMemorySource>,
    result_keeper: &mut impl ResultKeeperExt,
    tracer: &mut impl Tracer<ForwardRunningSystem>,
) -> Result<(), BootloaderSubsystemError> {
    ForwardBootloader::run_prepared::<Config>(oracle, result_keeper, tracer).map(|_| ())
}

pub fn run_prover_input_no_panic<Config: BasicBootloaderExecutionConfig>(
    oracle: ReadWitnessSource<DummyMemorySource>,
    result_keeper: &mut impl ResultKeeperExt,
    tracer: &mut impl Tracer<ProverInputSystem>,
) -> Result<(Vec<u32>, BatchOutput), BootloaderSubsystemError> {
    ProverInputBootloader::run_prepared::<Config>(oracle, result_keeper, tracer)
        .map(|o| (o.0.get_read_items().borrow().clone(), o.2))
}
