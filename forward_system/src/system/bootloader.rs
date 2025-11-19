use crate::system::system::*;
use basic_bootloader::bootloader;
use basic_bootloader::bootloader::config::BasicBootloaderExecutionConfig;
use basic_bootloader::bootloader::result_keeper::ResultKeeperExt;
use oracle_provider::ZkEENonDeterminismSource;
use zk_ee::system::tracer::Tracer;
use zk_ee::types_config::EthereumIOTypesConfig;

///
/// Run bootloader with forward system with a given `oracle`.
/// Returns execution results(tx results, state changes, events, etc) via `results_keeper`.
///
pub fn run_forward<Config: BasicBootloaderExecutionConfig>(
    oracle: ZkEENonDeterminismSource,
    result_keeper: &mut impl ResultKeeperExt<
        EthereumIOTypesConfig,
        BlockHeader = bootloader::block_header::BlockHeader,
    >,
    tracer: &mut impl Tracer<ForwardRunningSystem>,
) {
    if let Err(err) = ForwardBootloader::run::<Config>(oracle, result_keeper, tracer) {
        panic!("Forward run failed with: {err}")
    };
}
