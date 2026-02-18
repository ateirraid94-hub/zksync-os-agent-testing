use alloy::{hex, rpc::types::trace::geth::CallFrame};
use zksync_os_revm_runner::revm_runner;

pub fn convert_call_frames_to_revm_trace(
    call_frames: Vec<CallFrame>,
) -> Vec<revm_runner::RevmTxCallTrace> {
    call_frames
        .into_iter()
        .enumerate()
        .map(|(tx_index, call_frame)| revm_runner::RevmTxCallTrace {
            tx_index,
            calls: vec![convert_call_frame(call_frame)],
        })
        .collect()
}

fn convert_call_frame(call_frame: CallFrame) -> revm_runner::RevmCallTrace {
    let success = call_frame.error.is_none() && call_frame.revert_reason.is_none();
    let status = if let Some(error) = call_frame.error {
        Some(error)
    } else {
        call_frame
            .revert_reason
            .map(|reason| format!("reverted: {reason}"))
            .or_else(|| Some("ok".to_owned()))
    };

    revm_runner::RevmCallTrace {
        trace_type: call_frame.typ,
        from: format!("{:#x}", call_frame.from),
        to: call_frame.to.map(|address| format!("{:#x}", address)),
        created_contract: None,
        value: call_frame.value.unwrap_or_default().to_string(),
        input: hex::encode_prefixed(call_frame.input),
        output: call_frame.output.map(hex::encode_prefixed),
        gas_limit: call_frame.gas.saturating_to(),
        gas_used: Some(call_frame.gas_used.saturating_to()),
        success: Some(success),
        status,
        children: call_frame
            .calls
            .into_iter()
            .map(convert_call_frame)
            .collect(),
    }
}
