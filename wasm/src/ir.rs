use walrus::ir::Instr;

type FlowFlags = u16;
type JavaInstruction = (Vec<u8>, FlowFlags);

pub struct ControlFlow {
    pub instructions: Vec<JavaInstruction>
}

impl From<Vec<u8>> for ControlFlow {
    fn from(bytes: Vec<u8>) -> Self {
        unimplemented!()
    }
}

pub enum WasmIR {
    Instruction(JavaInstruction),
    Block(Vec<WasmIR>)
}

