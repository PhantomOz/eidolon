use revm::{
    Database, EvmContext, Inspector,
    interpreter::{Interpreter, OpCode},
    primitives::Log,
};
use serde::Serialize;
use tracing::debug;

/// A single step in the execution trace
#[derive(Clone, Debug, Serialize)]
pub struct TraceStep {
    pub pc: u64,            // Program Counter (Line number)
    pub op: String,         // Opcode Name (e.g., "PUSH1")
    pub gas: u64,           // Gas remaining
    pub stack: Vec<String>, // Current Stack
    pub depth: u64,
}

/// The Inspector that records execution
#[derive(Default, Debug, Clone, Serialize)]
pub struct EidolonTracer {
    pub steps: Vec<TraceStep>,
}

impl<DB: Database> Inspector<DB> for EidolonTracer {
    // Called before every OpCode is executed
    fn step(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        let pc = interp.program_counter() as u64;
        let op_u8 = interp.current_opcode();
        let gas = interp.gas.remaining();

        let stack: Vec<String> = interp
            .stack()
            .data()
            .iter()
            .map(|x| x.to_string())
            .collect();

        let op_name = OpCode::new(op_u8)
            .map(|o| o.as_str().to_string())
            .unwrap_or_else(|| format!("0x{:02x}", op_u8));

        self.steps.push(TraceStep {
            pc,
            op: op_name,
            gas,
            stack,
            depth: 0,
        });
    }

    // You can also hook into log, call, create, selfdestruct...
    fn log(&mut self, _interp: &mut Interpreter, _context: &mut EvmContext<DB>, log: &Log) {
        debug!("LOG EMITTED: {:?}", log.topics());
    }
}
