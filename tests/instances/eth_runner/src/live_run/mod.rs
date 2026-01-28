use anyhow::Result;
mod db;
mod rpc;
mod utils;
mod statistics;
mod prefetch;
mod block_execution;
mod error_handling;
use block_execution::GpuSharedState;
use db::{BlockStatus, BlockTraces, Database};
use rig::log::{debug, info, warn};
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Instant;
use statistics::RunStatistics;

/// Prefetch tuning constants - adjust based on your server characteristics.
///
/// ## Tuning Guide
///
/// ### PREFETCH_QUEUE_CAPACITY
/// - **Purpose**: Number of prefetched batches that can queue before prefetch thread blocks
/// - **When to increase**: If prefetch thread is faster than main thread and you want larger buffer
/// - **When to decrease**: If memory is constrained or you want tighter coordination
/// - **Default**: 16 (allows ~2-3 batches ahead with PREFETCH_SIZE=6)
///
/// ### PREFETCH_WAIT_TOTAL_MS
/// - **Purpose**: Maximum time to wait for prefetch thread to deliver a block before fetching ourselves
/// - **When to increase**: If RPC is slow but prefetch is working (reduces duplicate fetches)
/// - **When to decrease**: If RPC is fast and waiting wastes time
/// - **Default**: 2200ms (~2 seconds)
///
/// ### PREFETCH_WAIT_STEP_MS
/// - **Purpose**: Time between checks while waiting for prefetch (granularity of wait loop)
/// - **When to adjust**: Usually fine at default, but can be reduced for faster response
/// - **Default**: 300ms
///
/// ### INITIAL_PREFETCH_BATCHES
/// - **Purpose**: Number of batches to prefetch synchronously before starting main loop
/// - **When to increase**: If you want a larger initial buffer (reduces early misses)
/// - **When to decrease**: If you want to start processing immediately
/// - **Default**: 1 (prefetches first PREFETCH_SIZE blocks)
///
/// ## Server-Specific Tuning
///
/// ### If block execution is SLOWER than block fetching:
/// - **Increase** `PREFETCH_SIZE` (in prefetch.rs) - fetch more blocks per batch
/// - **Increase** `PREFETCH_QUEUE_CAPACITY` - allow more queued batches
/// - **Increase** `INITIAL_PREFETCH_BATCHES` - start with larger buffer
/// - **Result**: Prefetch stays well ahead, reducing misses
///
/// ### If block execution is FASTER than block fetching:
/// - **Decrease** `PREFETCH_SIZE` - smaller batches reduce latency per batch
/// - **Keep** `PREFETCH_QUEUE_CAPACITY` moderate - don't need huge buffer
/// - **Decrease** `PREFETCH_WAIT_TOTAL_MS` - don't wait long, fetch immediately
/// - **Result**: Main thread doesn't wait unnecessarily
///
/// ### If RPC is very slow (50+ seconds per batch):
/// - **Decrease** `PREFETCH_SIZE` - smaller batches complete faster
/// - **Increase** `PREFETCH_QUEUE_CAPACITY` - queue more batches in parallel
/// - **Increase** `PREFETCH_WAIT_TOTAL_MS` - wait longer before duplicate fetch
/// - **Result**: Better parallelization, fewer duplicate fetches
///
/// ### If RPC is very fast (<1 second per batch):
/// - **Increase** `PREFETCH_SIZE` - take advantage of fast RPC
/// - **Decrease** `PREFETCH_WAIT_TOTAL_MS` - don't wait, prefetch is fast
/// - **Result**: Maximum throughput
const PREFETCH_QUEUE_CAPACITY: usize = 16;
const PREFETCH_WAIT_TOTAL_MS: u64 = 12000;
const PREFETCH_WAIT_STEP_MS: u64 = 200;
const INITIAL_PREFETCH_BATCHES: u64 = 2;


pub fn live_run(
    start_block: u64,
    end_block: u64,
    endpoint: String,
    db_path: String,
    witness_output_dir: Option<String>,
    skip_successful: bool,
    persist_all: bool,
    webhook: Option<String>,
    single_tx: Option<u64>,
    only_forward: bool,
    backup_endpoint: Option<String>,
    call_tracing_enabled: bool,
) -> Result<()> {
    let run_start = Instant::now();
    
    // Install panic hook (with or without webhook)
    utils::install_panic_hook(webhook.clone());
    
    let init_start = Instant::now();
    let db = Database::init(db_path)?;
    assert!(start_block <= end_block);
    utils::fetch_block_hashes(start_block, &db, &endpoint)?;
    let chain_id = rpc::get_chain_id(&endpoint)?;
    let init_time = init_start.elapsed();
    
    info!("=== Live Run Started ===");
    info!("Blocks: {} to {}", start_block, end_block);
    info!("Initialization: {:.2}ms", init_time.as_secs_f64() * 1000.0);

    #[cfg(feature = "gpu")]
    let mut gpu_state = {
        info!("Setting up GPU state...");
        let bin_path = rig::chain::get_zksync_os_img_path(&Some("evm_replay".to_string()))
            .as_path()
            .to_str()
            .unwrap()
            .to_string();
        let binary = rig::cli_lib::prover_utils::load_binary_from_path(&bin_path);
        let s = rig::cli_lib::prover_utils::GpuSharedState::new(
            &binary,
            rig::gpu_prover::circuit_type::MainCircuitType::ReducedRiscVMachine,
        );
        info!("Done setting up GPU state...");
        s
    };
    #[cfg(feature = "gpu")]
    let gpu_state = &mut Some(&mut gpu_state);

    #[cfg(not(feature = "gpu"))]
    let gpu_state: &mut Option<&mut GpuSharedState> = &mut None;

    let mut stats = RunStatistics::new();
    let mut prefetch_cache = std::collections::HashMap::<u64, BlockTraces>::new();
    
    // Initialize prefetch coordination
    // Start prefetch thread from start_block + initial batches to allow initial synchronous prefetch
    let initial_prefetch_start = start_block + prefetch::PREFETCH_SIZE as u64 * INITIAL_PREFETCH_BATCHES;
    let next_block_to_prefetch = Arc::new(AtomicU64::new(initial_prefetch_start));
    let stop_prefetch = Arc::new(AtomicBool::new(false));
    
    // Channel for prefetch thread to send completed batches to main thread
    // Capacity allows multiple batches to queue, keeping prefetch ahead of main thread
    let (prefetch_tx, prefetch_rx) =
        mpsc::sync_channel::<(std::collections::HashMap<u64, BlockTraces>, std::time::Duration)>(PREFETCH_QUEUE_CAPACITY);
    
    // Prefetch initial batch(es) synchronously before starting main loop
    // This reduces early misses and provides initial buffer
    let initial_prefetch_end = (start_block
        + prefetch::PREFETCH_SIZE as u64 * INITIAL_PREFETCH_BATCHES
        - 1)
        .min(end_block);
    let mut initial_blocks = Vec::new();
    for block_num in start_block..=initial_prefetch_end {
        // Skip blocks already in database
        if !db
            .get_block_traces(block_num)
            .map(|opt| opt.is_none())
            .unwrap_or(false)
        {
            continue;
        }
        // Skip blocks already successfully processed (if skip_successful is enabled)
        if skip_successful {
            if let Ok(Some(status)) = db.get_block_status(block_num) {
                if matches!(status, BlockStatus::Success) {
                    continue;
                }
            }
        }
        initial_blocks.push(block_num);
    }
    
        // Fetch initial batch if we have blocks to prefetch
        if !initial_blocks.is_empty() {
            let prefetch_start = Instant::now();
            match prefetch::fetch_block_traces_batch(
                &initial_blocks,
                &db,
                &endpoint,
                call_tracing_enabled,
            ) {
                Ok(batch_results) => {
                    let prefetch_time = prefetch_start.elapsed();
                    let block_count = batch_results.len();
                    stats.total_prefetch_time += prefetch_time;
                    stats.total_blocks_prefetched += block_count as u64;
                    prefetch_cache.extend(batch_results);
                    info!("Initial prefetch: {} blocks in {:.2}ms", 
                        block_count,
                        prefetch_time.as_secs_f64() * 1000.0
                    );
                }
                Err(e) => warn!("Failed to prefetch initial batch: {}, will continue without it", e),
            }
        }

    let prefetch_db = db.clone();
    let prefetch_endpoint = endpoint.clone();
    let prefetch_next = Arc::clone(&next_block_to_prefetch);
    let prefetch_stop = Arc::clone(&stop_prefetch);
    let prefetch_skip_successful = skip_successful;
    let prefetch_call_tracing_enabled = call_tracing_enabled;
    // Spawn prefetch thread to continuously fetch blocks ahead of main thread
    let prefetch_handle = std::thread::spawn(move || {
        // Prefetch thread continuously fetches blocks ahead of main thread
        // It maintains a buffer by fetching batches and sending them via channel
        while !prefetch_stop.load(Ordering::Relaxed) {
            let next = prefetch_next.load(Ordering::Relaxed);
            if next > end_block {
                break;
            }
            
            // Determine range for this batch
            let prefetch_range_end =
                (next + prefetch::PREFETCH_SIZE as u64 - 1).min(end_block);
            let mut prefetch_blocks = Vec::new();
            
            // Collect blocks to prefetch, filtering out blocks already in DB or already succeeded
            for block_num in next..=prefetch_range_end {
                if prefetch_stop.load(Ordering::Relaxed) {
                    return;
                }
                
                // Skip blocks already in database
                if !prefetch_db
                    .get_block_traces(block_num)
                    .map(|opt| opt.is_none())
                    .unwrap_or(false)
                {
                    continue;
                }
                
                // Skip blocks already successfully processed (if skip_successful is enabled)
                if prefetch_skip_successful {
                    if let Ok(Some(status)) = prefetch_db.get_block_status(block_num) {
                        if matches!(status, BlockStatus::Success) {
                            continue;
                        }
                    }
                }
                
                prefetch_blocks.push(block_num);
            }
            
            // If no blocks to prefetch in this range, advance and continue
            if prefetch_blocks.is_empty() {
                prefetch_next.store(prefetch_range_end + 1, Ordering::Relaxed);
                continue;
            }
            
            // Update next_block_to_prefetch BEFORE fetching to signal we're working on this range
            // This prevents main thread from starting to fetch the same blocks
            prefetch_next.store(prefetch_range_end + 1, Ordering::Relaxed);
            
            // Fetch the batch
            let batch_start = Instant::now();
            match prefetch::fetch_block_traces_batch(
                &prefetch_blocks,
                &prefetch_db,
                &prefetch_endpoint,
                prefetch_call_tracing_enabled,
            ) {
                Ok(batch_results) => {
                    let batch_time = batch_start.elapsed();
                    // Send batch to main thread via channel
                    // If channel is full or closed, break (main thread may have finished)
                    if prefetch_tx.send((batch_results, batch_time)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    warn!("Failed to prefetch batch starting at block {}: {}, will retry next iteration", 
                        prefetch_blocks.first().copied().unwrap_or(next), e);
                    // On error, roll back next_block_to_prefetch to allow retry of failed blocks
                    // This ensures we don't skip blocks that failed to fetch
                    let failed_start = prefetch_blocks.first().copied().unwrap_or(next);
                    prefetch_next.store(failed_start, Ordering::Relaxed);
                }
            }
        }
    });
    let mut stopped_early = false;
    
    for n in start_block..=end_block {
        // Update current block number for panic handler
        utils::CURRENT_BLOCK_NUMBER.store(n, Ordering::Relaxed);

        // Drain prefetch queue - process all available batches immediately
        while let Ok((batch_results, batch_time)) = prefetch_rx.try_recv() {
            stats.total_prefetch_time += batch_time;
            stats.total_blocks_prefetched += batch_results.len() as u64;
            prefetch_cache.extend(batch_results);
        }
        
        // Check if we should skip this block
        // Remove from prefetch cache first to prevent cache from stalling
        if let std::result::Result::Ok(Some(status)) = db.get_block_status(n) {
            if skip_successful && matches!(status, BlockStatus::Success) {
                // Remove from cache before skipping to prevent prefetch stall
                prefetch_cache.remove(&n);
                debug!("Skipping block {n}, already succeeded");
                stats.blocks_skipped_already_succeeded += 1;
                continue;
            }
        }
        
        // Get traces from prefetch cache or fetch if not available
        let block_traces = if let Some(traces) = prefetch_cache.remove(&n) {
            stats.prefetch_hits += 1;
            traces
        } else {
            stats.prefetch_misses += 1;
            let next = next_block_to_prefetch.load(Ordering::Relaxed);
            let prefetch_range_end = (next + prefetch::PREFETCH_SIZE as u64 - 1).min(end_block);
            // Check if prefetch thread is currently working on this block or will soon
            // Only wait if prefetch has reached this block (n >= next)
            let is_prefetching = n >= next && n <= prefetch_range_end;

            let mut waited_trace = None;
            // If prefetch thread is working on this block, wait briefly for it to arrive
            // This reduces duplicate fetches when prefetch is just slightly behind
            if is_prefetching {
                let wait_deadline =
                    Instant::now() + std::time::Duration::from_millis(PREFETCH_WAIT_TOTAL_MS);
                let wait_step = std::time::Duration::from_millis(PREFETCH_WAIT_STEP_MS);
                loop {
                    let now = Instant::now();
                    if now >= wait_deadline {
                        break;
                    }
                    
                    // First try non-blocking receive to process any immediately available batches
                    while let Ok((batch_results, batch_time)) = prefetch_rx.try_recv() {
                        stats.total_prefetch_time += batch_time;
                        stats.total_blocks_prefetched += batch_results.len() as u64;
                        prefetch_cache.extend(batch_results);
                        if let Some(traces) = prefetch_cache.remove(&n) {
                            stats.prefetch_hits += 1;
                            stats.prefetch_misses -= 1;
                            waited_trace = Some(traces);
                            break;
                        }
                    }
                    
                    if waited_trace.is_some() {
                        break;
                    }
                    
                    // Then wait with timeout for next batch
                    let remaining = wait_deadline - now;
                    let step = if remaining > wait_step { wait_step } else { remaining };
                    match prefetch_rx.recv_timeout(step) {
                        Ok((batch_results, batch_time)) => {
                            stats.total_prefetch_time += batch_time;
                            stats.total_blocks_prefetched += batch_results.len() as u64;
                            prefetch_cache.extend(batch_results);
                            if let Some(traces) = prefetch_cache.remove(&n) {
                                stats.prefetch_hits += 1;
                                stats.prefetch_misses -= 1;
                                waited_trace = Some(traces);
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // Timeout - continue waiting if we have time left
                            continue;
                        }
                        Err(_) => {
                            // Channel closed or error - stop waiting
                            break;
                        }
                    }
                }
            }
            
            if let Some(traces) = waited_trace {
                traces
            } else {
                // Still not in cache, fetch it ourselves
                // Update next_block_to_prefetch to current block so prefetch can catch up
                // Only update if we're ahead of prefetch (n < next) to avoid interfering
                if n < next {
                    next_block_to_prefetch.store(n, Ordering::Relaxed);
                }
                let fetch_start = Instant::now();
                let fetched = match error_handling::fetch_block_traces_with_backup(
                    n,
                    &db,
                    &endpoint,
                    backup_endpoint.as_ref(),
                    chain_id,
                    webhook.as_ref(),
                    &mut stats,
                    call_tracing_enabled,
                )? {
                    Some(traces) => traces,
                    None => continue, // Block skipped due to trace fetch failure
                };
                stats.total_prefetch_wait_time += fetch_start.elapsed();
                fetched
            }
        };
        
        // Process block sequentially
        let block_start = Instant::now();
        let primary_result = block_execution::run_block(
            n,
            &db,
            &endpoint,
            witness_output_dir.clone(),
            persist_all,
            chain_id,
            single_tx,
            gpu_state,
            only_forward,
            block_traces,
            call_tracing_enabled,
        );
        let block_time = block_start.elapsed();
        stats.total_block_time += block_time;
        
        let result = match primary_result {
            Ok(BlockStatus::Success) => Ok(BlockStatus::Success),
            failed_result => {
                if !call_tracing_enabled {
                    match rpc::get_all_block_traces(&endpoint, n, true) {
                        Ok((block, prestate, diff, receipts, call)) => {
                            let retry_traces = BlockTraces {
                                block,
                                prestate,
                                diff,
                                receipts,
                                call,
                            };
                            let retry_start = Instant::now();
                            let retry_result = block_execution::run_block(
                                n,
                                &db,
                                &endpoint,
                                witness_output_dir.clone(),
                                persist_all,
                                chain_id,
                                single_tx,
                                gpu_state,
                                only_forward,
                                retry_traces,
                                true,
                            );
                            stats.total_block_time += retry_start.elapsed();
                            if matches!(retry_result, Ok(BlockStatus::Success)) {
                                retry_result
                            } else if let Some(backup) = backup_endpoint.as_ref() {
                                error_handling::retry_block_with_backup_endpoint(
                                    n,
                                    backup,
                                    &db,
                                    witness_output_dir.clone(),
                                    persist_all,
                                    chain_id,
                                    single_tx,
                                    gpu_state,
                                    only_forward,
                                    &mut stats.total_block_time,
                                    true,
                                )
                            } else {
                                retry_result
                            }
                        }
                        Err(err) => {
                            warn!("Failed to refetch call trace for block {n}: {err}");
                            if let Some(backup) = backup_endpoint.as_ref() {
                                error_handling::retry_block_with_backup_endpoint(
                                    n,
                                    backup,
                                    &db,
                                    witness_output_dir.clone(),
                                    persist_all,
                                    chain_id,
                                    single_tx,
                                    gpu_state,
                                    only_forward,
                                    &mut stats.total_block_time,
                                    true,
                                )
                            } else {
                                failed_result
                            }
                        }
                    }
                } else if let Some(backup) = backup_endpoint.as_ref() {
                    error_handling::retry_block_with_backup_endpoint(
                        n,
                        backup,
                        &db,
                        witness_output_dir.clone(),
                        persist_all,
                        chain_id,
                        single_tx,
                        gpu_state,
                        only_forward,
                        &mut stats.total_block_time,
                        true,
                    )
                } else {
                    failed_result
                }
            }
        };
        
        // Handle result (update stats, send webhooks, check failures)
        // If max failures reached, break out of loop gracefully
        if let Err(e) = error_handling::handle_block_result(result, n, chain_id, webhook.as_ref(), &mut stats) {
            warn!("Stopping execution: {e}");
            stopped_early = true;
            break;
        }
    }

    stop_prefetch.store(true, Ordering::Relaxed);
    drop(prefetch_rx);
    let _ = prefetch_handle.join();
    
    let total_time = run_start.elapsed();
    statistics::log_run_statistics(start_block, end_block, chain_id, init_time, total_time, &stats);
    
    if let Some(webhook) = webhook.as_ref() {
        let machine_info = utils::get_machine_info();
        let (emoji, status_msg) = if stopped_early {
            (":rotating_light:", "stopped early due to max failures")
        } else {
            (":white_check_mark:", "successfully!")
        };
        let msg = format!(
            "{emoji} eth_runner: finished running from block {start_block} to {end_block} on chain with id {chain_id} {status_msg}\n\
            \n\
            *Block Range:* {start_block} to {end_block}\n\
            *Chain ID:* {chain_id}\n\
            *Blocks Processed:* {}\n\
            *Blocks Skipped (already succeeded):* {}\n\
            *Blocks Skipped (trace fetch failed):* {}\n\
            *Failures:* {} ({} critical)\n\
            \n\
            *Machine Info:*\n\
            {machine_info}",
            stats.blocks_actually_processed,
            stats.blocks_skipped_already_succeeded,
            stats.blocks_skipped_trace_fetch,
            stats.failures,
            stats.critical_failures
        );
        utils::send_slack(webhook, &msg)?
    }
    Ok(())
}

///
/// Export native/effective cycles ratios to csv file.
///
pub fn export_block_ratios(db: String, path: Option<String>) -> Result<()> {
    let db = Database::init(db)?;
    let path = path.unwrap_or("ratios.csv".to_string());
    db.export_block_ratios_to_csv(&path)?;
    db.export_block_resource_info_to_csv("resource_info.csv")?;
    Ok(())
}

///
/// Show failed blocks, if any.
///
pub fn show_status(db: String) -> Result<()> {
    let db = Database::init(db)?;
    let failures = db.iter_failed_block_statuses()?;
    if failures.is_empty() {
        println!("✅ All blocks succeeded.");
        Ok(())
    } else {
        println!("❌ Failed blocks:");
        for (block_number, status) in failures {
            println!("Block {block_number:<8} => {status:?}");
        }
        Ok(())
    }
}
