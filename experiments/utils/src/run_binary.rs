use risc_v_simulator::sim::BinarySource;
use risc_v_simulator::{
    abstractions::{memory::VectorMemoryImpl, non_determinism::NonDeterminismCSRSource},
    cycle::IMStandardIsaConfig,
    sim::{DiagnosticsConfig, ProfilerConfig, SimulatorConfig},
};
use std::{io::Read, path::PathBuf};
use std::str::FromStr;

const MAX_CYCLES: usize = 1 << 25;

pub fn run_binary(
    img_path: &str,
    non_determinism_source: impl NonDeterminismCSRSource<VectorMemoryImpl>,
) -> [u32; 8] {
    let img_path = PathBuf::from_str(img_path).unwrap();
    println!("ZK RISC-V simulator is starting");

    // Check that the bin file is present and readable.
    let mut file = std::fs::File::open(img_path.clone())
        .unwrap_or_else(|_| panic!("RISC-V bin file missing: {img_path:?}"));
    let mut buffer = vec![];
    file.read_to_end(&mut buffer).expect("must read the file");

    let config = SimulatorConfig {
        bin: BinarySource::Path(img_path),
        cycles: MAX_CYCLES,
        entry_point: 0,
        diagnostics: None,
    };

    let run_result =
        risc_v_simulator::runner::run_simple_with_entry_point_and_non_determimism_source(
            config,
            non_determinism_source,
        );

    // our convention is to return 32 bytes placed into registers x10-x17

    #[allow(deprecated)]
    run_result.state.registers[10..18].try_into().unwrap()
}