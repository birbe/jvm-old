use walrus::ir::Instr;
use jvm::vm::vm::bytecode::Bytecode;
use std::collections::HashMap;

type Intermediate1 = (Bytecode, bool);

pub enum Intermediate2 {
    Intermediate1(Vec<Intermediate1>),
    Instructions(Vec<Bytecode>),
    Block(Vec<Intermediate2>)
}

pub struct ControlFlow {
    pub sourcemap: HashMap<usize, usize>,
    pub bytecode: Vec<Bytecode>,

    pub pass_1: Vec<Intermediate1>,
    pub pass_2: Vec<Intermediate2>
}

impl ControlFlow {

    //The first pass flags each location in the bytecode that requires a block
    fn first_pass(&mut self) {
        self.pass_1 = self.bytecode.iter().map(|b| {
            (b.clone(), false)
        }).collect();

        for index in 0..self.bytecode.len() {
            let instr = self.bytecode.get(index).unwrap();

            let byte_index = self.sourcemap.get(&index).unwrap();

            match instr {
                Bytecode::Goto(offset) |
                Bytecode::If_acmpeq(offset) |
                Bytecode::If_acmpne(offset) |
                Bytecode::If_icmpeq(offset) |
                Bytecode::If_icmpne(offset) |
                Bytecode::If_icmplt(offset) |
                Bytecode::If_icmpge(offset) |
                Bytecode::If_icmpgt(offset) |
                Bytecode::If_icmple(offset) |
                Bytecode::Ifeq(offset) |
                Bytecode::Ifne(offset) |
                Bytecode::Iflt(offset) |
                Bytecode::Ifge(offset) |
                Bytecode::Ifgt(offset) |
                Bytecode::Ifle(offset) |
                Bytecode::Ifnonnull(offset) |
                Bytecode::Ifnull(offset) => {
                    // let entry: &mut Intermediate1 = self.pass_1.get_mut((byte_index as isize) + (*offset as i16)).unwrap();
                    // entry.1 = true;
                },

                Bytecode::Jsr(_) => unimplemented!(),
                Bytecode::Jsr_w(_) => unimplemented!(),

                Bytecode::Lookupswitch(_, _) => {}
                Bytecode::Swap => {}
                Bytecode::Tableswitch => {}
                Bytecode::Wide => {}
                _ => {}
            }
        }
    }

    fn second_pass(i2: &mut Intermediate2) -> Intermediate2 {
        match i2 {
            Intermediate2::Intermediate1(i1) => {
                let mut bytecodes = Vec::new();
                //this to Intermediate2::Instructions

                for index in 0..i1.len() {
                    let entry = i1.get(index).unwrap();
                    if entry.1 { //Make a block, starting from here
                        let block: Vec<Intermediate1> = (index..i1.len()).map(|i| {
                            i1.get(i).unwrap().clone()
                        }).collect();

                        return if bytecodes.len() > 0 {
                            Intermediate2::Block(
                                vec![
                                    Intermediate2::Instructions(bytecodes),
                                    Intermediate2::Intermediate1(block)
                                ]
                            )
                        } else {
                            Intermediate2::Intermediate1(block)
                        }
                    } else {
                        bytecodes.push(entry.0.clone());
                    }
                }
            },
            Intermediate2::Instructions(_) => {}, //End case, no more blocks to add
            Intermediate2::Block(block) => {
                block.iter_mut().for_each(|e| {
                    *e = Self::second_pass(e);
                });
            }
        }

        Intermediate2::Intermediate1(Vec::new())
    }

}

impl From<Vec<u8>> for ControlFlow {
    fn from(bytes: Vec<u8>) -> Self {
        unimplemented!()
    }
}

pub enum WasmIR {
    Instruction(Intermediate1),
    Block(Vec<WasmIR>)
}

