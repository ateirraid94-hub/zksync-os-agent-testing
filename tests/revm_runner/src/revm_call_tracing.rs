use alloy::hex;
use reth_revm::inspector::Inspector;
use reth_revm::interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RevmCallTrace {
    pub trace_type: String,
    pub from: String,
    pub to: Option<String>,
    pub created_contract: Option<String>,
    pub value: String,
    pub input: String,
    pub output: Option<String>,
    pub gas_limit: u64,
    pub gas_used: Option<u64>,
    pub success: Option<bool>,
    pub status: Option<String>,
    pub children: Vec<RevmCallTrace>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RevmTxCallTrace {
    pub tx_index: usize,
    pub calls: Vec<RevmCallTrace>,
}

#[derive(Debug, Clone)]
struct TraceNode {
    trace: RevmCallTrace,
    children: Vec<usize>,
}

#[derive(Debug, Default, Clone)]
pub struct RevmCallInspector {
    current_tx: usize,
    active_stack: Vec<usize>,
    nodes: Vec<TraceNode>,
    tx_roots: Vec<Vec<usize>>,
}

impl RevmCallInspector {
    pub fn begin_transaction(&mut self, tx_index: usize) {
        self.current_tx = tx_index;
        self.active_stack.clear();
        while self.tx_roots.len() <= tx_index {
            self.tx_roots.push(Vec::new());
        }
    }

    fn push_node(&mut self, trace: RevmCallTrace) {
        let parent = self.active_stack.last().copied();
        let idx = self.nodes.len();
        self.nodes.push(TraceNode {
            trace,
            children: Vec::new(),
        });

        if let Some(parent_idx) = parent {
            self.nodes[parent_idx].children.push(idx);
        } else {
            self.tx_roots[self.current_tx].push(idx);
        }

        self.active_stack.push(idx);
    }

    fn finalize_active_call(
        &mut self,
        output_hex: String,
        gas_used: u64,
        success: bool,
        status: String,
        created_contract: Option<String>,
    ) {
        if let Some(idx) = self.active_stack.pop() {
            self.nodes[idx].trace.output = Some(output_hex);
            self.nodes[idx].trace.gas_used = Some(gas_used);
            self.nodes[idx].trace.success = Some(success);
            self.nodes[idx].trace.status = Some(status);
            self.nodes[idx].trace.created_contract = created_contract;
        }
    }

    pub fn export(&self) -> Vec<RevmTxCallTrace> {
        self.tx_roots
            .iter()
            .enumerate()
            .filter(|(_, roots)| !roots.is_empty())
            .map(|(tx_index, roots)| RevmTxCallTrace {
                tx_index,
                calls: roots.iter().map(|idx| self.export_node(*idx)).collect(),
            })
            .collect()
    }

    fn export_node(&self, idx: usize) -> RevmCallTrace {
        let node = &self.nodes[idx];
        let mut trace = node.trace.clone();
        trace.children = node
            .children
            .iter()
            .map(|child_idx| self.export_node(*child_idx))
            .collect();
        trace
    }
}

impl<CTX, INTR> Inspector<CTX, INTR> for RevmCallInspector
where
    CTX: reth_revm::context_interface::ContextTr,
    INTR: reth_revm::interpreter::InterpreterTypes,
{
    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        let (from, to) = match inputs.scheme {
            reth_revm::interpreter::CallScheme::Call => (inputs.caller, inputs.target_address),
            reth_revm::interpreter::CallScheme::CallCode => {
                (inputs.target_address, inputs.bytecode_address)
            }
            reth_revm::interpreter::CallScheme::DelegateCall => {
                (inputs.target_address, inputs.bytecode_address)
            }
            reth_revm::interpreter::CallScheme::StaticCall => {
                (inputs.caller, inputs.target_address)
            }
        };

        let trace = RevmCallTrace {
            trace_type: format!("{:?}", inputs.scheme),
            from: format!("{:#x}", from),
            to: Some(format!("{:#x}", to)),
            created_contract: None,
            value: inputs.call_value().to_string(),
            input: hex::encode_prefixed(inputs.input.bytes(context)),
            output: None,
            gas_limit: inputs.gas_limit,
            gas_used: None,
            success: None,
            status: None,
            children: Vec::new(),
        };
        self.push_node(trace);
        None
    }

    fn call_end(&mut self, _context: &mut CTX, _inputs: &CallInputs, outcome: &mut CallOutcome) {
        self.finalize_active_call(
            hex::encode_prefixed(outcome.output()),
            outcome.gas().spent(),
            outcome.instruction_result().is_ok(),
            format!("{:?}", outcome.instruction_result()),
            None,
        );
    }

    fn create(&mut self, _context: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        let trace = RevmCallTrace {
            trace_type: format!("{:?}", inputs.scheme),
            from: format!("{:#x}", inputs.caller),
            to: None,
            created_contract: None,
            value: inputs.value.to_string(),
            input: hex::encode_prefixed(&inputs.init_code),
            output: None,
            gas_limit: inputs.gas_limit,
            gas_used: None,
            success: None,
            status: None,
            children: Vec::new(),
        };
        self.push_node(trace);
        None
    }

    fn create_end(
        &mut self,
        _context: &mut CTX,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        self.finalize_active_call(
            hex::encode_prefixed(outcome.output()),
            outcome.gas().spent(),
            outcome.instruction_result().is_ok(),
            format!("{:?}", outcome.instruction_result()),
            outcome.address.map(|address| format!("{:#x}", address)),
        );
    }
}
