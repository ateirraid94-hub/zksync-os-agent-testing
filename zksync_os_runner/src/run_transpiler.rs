use crate::DiagnosticsConfig;
use riscv_transpiler::vm::NonDeterminismCSRSource;
use riscv_transpiler::vm::RamWithRomRegion;
use std::{path::PathBuf, str::FromStr};

/// Runs the zksync_os binary on a simulator with a given non_determinism source for that many cycles.
/// If you enable diagnostics, it will print the flamegraph - but the run will be a lot slower.
pub fn run_default<const ROM_BOUND_SECOND_WORD_BITS: usize>(
    cycles: usize,
    non_determinism_source: impl NonDeterminismCSRSource<RamWithRomRegion<ROM_BOUND_SECOND_WORD_BITS>>,
    enable_diagnostics: bool,
) -> [u32; 8] {
    run_default_with_flamegraph_path(
        cycles,
        non_determinism_source,
        if enable_diagnostics {
            Some(std::env::current_dir().unwrap().join("flamegraph.svg"))
        } else {
            None
        },
    )
}

pub fn run_default_with_flamegraph_path<const ROM_BOUND_SECOND_WORD_BITS: usize>(
    cycles: usize,
    non_determinism_source: impl NonDeterminismCSRSource<RamWithRomRegion<ROM_BOUND_SECOND_WORD_BITS>>,
    _diagnostics_path: Option<PathBuf>,
) -> [u32; 8] {
    let zksync_os_path =
        std::env::var("ZKSYNC_OS_DIR").unwrap_or_else(|_| String::from("../zksync_os"));
    // let diag_config = diagnostics_path.map(|path| {
    //     let sym_path = PathBuf::from_str(&zksync_os_path).unwrap().join("app.elf");

    //     let mut d = DiagnosticsConfig::new(sym_path);

    //     d.profiler_config = {
    //         let mut p = ProfilerConfig::new(path);

    //         p.frequency_recip = 1;
    //         p.reverse_graph = false;

    //         Some(p)
    //     };

    //     d
    // });
    let diag_config = None;
    run(
        PathBuf::from_str(&zksync_os_path).unwrap().join("app.bin"),
        PathBuf::from_str(&zksync_os_path).unwrap().join("app.text"),
        diag_config,
        cycles,
        non_determinism_source,
    )
}

///
/// Runs zkOS on RISC-V (proof running) with given params:
/// `img_path` - path to ZKsync OS binary file (for now always in "zksync_os/app.bin")
/// `diagnostics` - optional diagnostics config, can be used to enable profiler.
/// `cycles` - limit for number of cycles.
/// `non_determinism_source` - non-determinism source used to read values from outside
///  (inside risc-v can be accessed via special system register read). In practice used to get all the block data - txs, metadata, storage values, etc.
///
/// Returns 256 bit program output. In real env this output will be exposed as proof public input.
///
pub fn run<const ROM_BOUND_SECOND_WORD_BITS: usize>(
    img_path: PathBuf,
    text_section_path: PathBuf,
    diagnostics: Option<DiagnosticsConfig>,
    cycles: usize,
    non_determinism_source: impl NonDeterminismCSRSource<RamWithRomRegion<ROM_BOUND_SECOND_WORD_BITS>>,
) -> [u32; 8] {
    run_and_get_effective_cycles(
        img_path,
        text_section_path,
        diagnostics,
        cycles,
        non_determinism_source,
    )
    .0
}

pub(crate) fn read_binary(path: &std::path::Path) -> (Vec<u8>, Vec<u32>) {
    use std::io::Read;
    let mut file = std::fs::File::open(path).expect("must open provided file");
    let mut buffer = vec![];
    file.read_to_end(&mut buffer).expect("must read the file");
    assert_eq!(buffer.len() % core::mem::size_of::<u32>(), 0);
    let mut binary = Vec::with_capacity(buffer.len() / core::mem::size_of::<u32>());
    for el in buffer.as_chunks::<4>().0 {
        binary.push(u32::from_le_bytes(*el));
    }

    (buffer, binary)
}

pub fn run_and_get_effective_cycles<const ROM_BOUND_SECOND_WORD_BITS: usize>(
    img_path: PathBuf,
    text_section_path: PathBuf,
    _diagnostics: Option<DiagnosticsConfig>,
    cycles: usize,
    mut non_determinism_source: impl NonDeterminismCSRSource<
        RamWithRomRegion<ROM_BOUND_SECOND_WORD_BITS>,
    >,
) -> ([u32; 8], Option<u64>) {
    use riscv_transpiler::ir::*;
    use riscv_transpiler::vm::*;

    type CountersT = DelegationsAndFamiliesCounters;
    println!("ZK RISC-V simulator is starting");

    let (_, binary) = read_binary(&img_path);
    let (_, text) = read_binary(&text_section_path);

    let instructions: Vec<Instruction> = text
        .into_iter()
        .map(|el| decode::<FullUnsignedMachineDecoderConfig>(el))
        .collect();

    // let instructions: Vec<Instruction> = text
    //     .into_iter()
    //     .map(|el| decode::<ReducedMachineDecoderConfig>(el))
    //     .collect();

    // let instructions: Vec<Instruction> = text
    //     .into_iter()
    //     .map(|el| decode::<DebugReducedMachineDecoderConfig>(el))
    //     .collect();

    let tape = SimpleTape::new(&instructions);
    let mut ram =
        RamWithRomRegion::<ROM_BOUND_SECOND_WORD_BITS>::from_rom_content(&binary, 1 << 30);
    let period = 1 << 20;
    let num_snapshots = cycles.div_ceil(period);
    let cycles_bound = period * num_snapshots;

    let mut state = State::initial_with_counters(CountersT::default());

    let mut snapshotter: SimpleSnapshotter<CountersT, ROM_BOUND_SECOND_WORD_BITS> =
        SimpleSnapshotter::new_with_cycle_limit(cycles_bound, period, state);

    let now = std::time::Instant::now();
    VM::<CountersT>::run_basic_unrolled::<
        SimpleSnapshotter<CountersT, ROM_BOUND_SECOND_WORD_BITS>,
        RamWithRomRegion<ROM_BOUND_SECOND_WORD_BITS>,
        _,
    >(
        &mut state,
        num_snapshots,
        &mut ram,
        &mut snapshotter,
        &tape,
        period,
        &mut non_determinism_source,
    );
    let elapsed = now.elapsed();

    let exact_cycles_passed = (state.timestamp - 4) / 4;

    println!(
        "Performance is {} MHz",
        (exact_cycles_passed as f64) / (elapsed.as_micros() as f64),
    );

    println!("Passed exactly {} cycles", exact_cycles_passed);

    // our convention is to return 32 bytes placed into registers x10-x17

    // TODO: move to new simulator
    #[allow(deprecated)]
    (
        state.registers[10..18]
            .iter()
            .map(|el| el.value)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        None,
    )
}

#[cfg(feature = "prover")]
pub fn simulate_witness_tracing<const ROM_BOUND_SECOND_WORD_BITS: usize>(
    img_path: PathBuf,
    non_determinism_source: impl NonDeterminismCSRSource<VectorMemoryImplWithRom>,
) {
    println!("ZK RISC-V simulator is starting");
    use risc_v_simulator::cycle::IMStandardIsaConfig;
    use std::alloc::Global;

    // Check that the bin file is present and readable.
    let mut file = std::fs::File::open(img_path.clone())
        .unwrap_or_else(|_| panic!("ZKsync OS bin file missing: {img_path:?}"));
    let mut buffer = vec![];
    file.read_to_end(&mut buffer).expect("must read the file");

    let num_instances_upper_bound = 1 << 14;
    let binary = execution_utils::get_padded_binary(&buffer);
    let worker = trace_and_split::setups::prover::worker::Worker::new();

    let now = std::time::Instant::now();
    let (all_witness_instances, _, _, _) =
        prover_examples::trace_execution_for_gpu::<_, IMStandardIsaConfig, Global>(
            num_instances_upper_bound,
            &binary,
            non_determinism_source,
            &worker,
        );
    let elapsed = now.elapsed();
    let cycles_upper_bound =
        all_witness_instances.len() * all_witness_instances[0].num_cycles_chunk_size;
    let speed = (cycles_upper_bound as f64) / elapsed.as_secs_f64() / 1_000_000f64;
    println!(
        "Simulator witness gen speed is roughly {speed} MHz: ran {cycles_upper_bound} cycles over {elapsed:?}"
    );
}
