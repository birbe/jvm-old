use walrus::ir::Instr;
use jvm::vm::vm::bytecode::Bytecode;
use std::collections::HashMap;
use std::io::{Cursor, Seek, SeekFrom};

pub type IndexedBytecode = (Bytecode, usize);
type Intermediate1 = (IndexedBytecode, bool);

#[derive(Debug)]
pub enum Intermediate2 {
    Intermediate1(Vec<Intermediate1>),
    Instructions(Vec<IndexedBytecode>),
    Block(Vec<Intermediate2>)
}

pub struct ControlFlow {
    pub source_bytecode_to_byte: HashMap<usize, usize>,
    pub source_byte_to_bytecode: HashMap<usize, usize>,
    pub bytecode: Vec<IndexedBytecode>
}

impl ControlFlow {

    //The first pass flags each location in the bytecode that requires a block
    fn first_pass(&mut self) -> Vec<Intermediate1> {
        let mut pass_1: Vec<Intermediate1> = self.bytecode.iter().map(|b| {
            (b.clone(), false)
        }).collect();

        for index in 0..self.bytecode.len() {
            let instr = self.bytecode.get(index).unwrap();

            let byte_index = *self.source_bytecode_to_byte.get(&index).unwrap();

            match &instr.0 {
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
                    let i = ((byte_index as isize) + (*offset as isize)) as usize;
                    let index = *self.source_byte_to_bytecode.get(&i).unwrap();
                    let entry: &mut Intermediate1 = pass_1.get_mut(index).unwrap();
                    entry.1 = true;
                },

                Bytecode::Jsr(_) => unimplemented!(),
                Bytecode::Jsr_w(_) => unimplemented!(),

                Bytecode::Lookupswitch(_, _) => {}
                Bytecode::Swap => {}
                Bytecode::Tableswitch => {}
                Bytecode::Wide(_) => {}
                _ => {}
            }
        }

        pass_1
    }

    fn second_pass(i2: Intermediate2) -> Intermediate2 {
        match i2 {
            Intermediate2::Intermediate1(i1) => {
                let mut bytecodes = Vec::new();
                //this to Intermediate2::Instructions

                for index in 0..i1.len() {
                    let entry = i1.get(index).unwrap();
                    if entry.1 { //Make a block, starting from here
                        let mut block: Vec<Intermediate1> = Vec::new();
                        block.push(
                            (
                                entry.0.clone(), false
                            )
                        );

                        let following: Vec<Intermediate1> = (index+1..i1.len()).map(|i| {
                            i1.get(i).unwrap().clone()
                        }).collect();

                        block.extend(following);

                        return if bytecodes.len() > 0 {
                            Intermediate2::Block(
                                vec![
                                    Intermediate2::Instructions(bytecodes),
                                    Self::second_pass(Intermediate2::Intermediate1(block))
                                ]
                            )
                        } else {
                            Self::second_pass(Intermediate2::Intermediate1(block))
                        }
                    } else {
                        bytecodes.push(entry.0.clone());
                    }
                }

                Intermediate2::Instructions(bytecodes)
            },
            Intermediate2::Instructions(i) => Intermediate2::Instructions(i), //End case, no more blocks to add
            Intermediate2::Block(mut block) => {
                let mut new = Vec::new();
                for _ in 0..block.len() {
                    new.push(
                        Self::second_pass(block.remove(0))
                    );
                }
                Intermediate2::Block(new)
            }
        }
    }

    pub fn convert(bytecode: &[u8]) -> Option<Intermediate2> {
        let mut flow = Self {
            source_bytecode_to_byte: HashMap::new(),
            source_byte_to_bytecode: HashMap::new(),
            bytecode: Vec::new()
        };

        let mut cursor = Cursor::new(bytecode);

        while (cursor.position() as usize) < bytecode.len() {
            let byte = Bytecode::from_bytes(cursor.position() as usize, &bytecode[cursor.position() as usize..bytecode.len()])?;
            let length = Bytecode::size_of(&byte);

            flow.source_bytecode_to_byte.insert(flow.bytecode.len(), cursor.position() as usize);
            flow.source_byte_to_bytecode.insert(cursor.position() as usize, flow.bytecode.len());

            flow.bytecode.push((byte, cursor.position() as usize));

            cursor.seek(SeekFrom::Current(
                length as i64
            )).ok()?;
        }

        let i1 = flow.first_pass();

        let mut i2 = Intermediate2::Intermediate1(i1);
        Option::Some(Self::second_pass(i2))
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

