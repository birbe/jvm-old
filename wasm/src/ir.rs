use walrus::ir::Instr;
use jvm::vm::vm::bytecode::Bytecode;
use std::collections::HashMap;
use std::io::{Cursor, Seek, SeekFrom};
use std::cmp::{min, max};

pub type LengthOfBlock = usize;

#[derive(Debug)]
pub enum BlockType {
    None,
    Loop(LengthOfBlock),
    If(LengthOfBlock)
}

pub type IndexedBytecode = (Bytecode, usize);

type BytecodeWithBlockFlag = (IndexedBytecode, BlockType);

#[derive(Debug)]
pub enum FoldedBytecode {
    Instructions(Vec<IndexedBytecode>),
    LoopBlock(Vec<FoldedBytecode>),
    IfBlock(Vec<FoldedBytecode>),
    Block(Vec<FoldedBytecode>)
}

pub struct ControlFlow {
    pub source_bytecode_to_byte: HashMap<usize, usize>,
    pub source_byte_to_bytecode: HashMap<usize, usize>,
    pub bytecode: Vec<IndexedBytecode>
}

impl ControlFlow {

    //The first pass flags each location in the bytecode that requires a block
    fn flag_bytecode(&mut self) -> Vec<BytecodeWithBlockFlag> {
        let mut pass_1: Vec<BytecodeWithBlockFlag> = self.bytecode.iter().map(|b| {
            (b.clone(), BlockType::None)
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
                    let target_bytecode_index = *self.source_byte_to_bytecode.get(
                        &(( (byte_index as isize) + (*offset as isize) ) as usize)
                    ).unwrap();
                    // let entry: &mut BytecodeWithBlockFlag = pass_1.get_mut(index).unwrap();

                    let block_min_index = min(target_bytecode_index, index);
                    let block_max_index = max(target_bytecode_index, index);

                    let block_size = block_max_index - block_min_index;

                    let block_begin: &mut BytecodeWithBlockFlag = pass_1.get_mut(block_min_index).unwrap();

                    block_begin.1 = if *offset <= 0 { //if the offset is less than or equal to 0, it's a loop
                        BlockType::Loop(block_size)
                    } else {
                        BlockType::If(block_size)
                    };
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

    fn fold_bytecode(flagged: &[BytecodeWithBlockFlag], in_block: bool) -> Vec<FoldedBytecode> {
        let mut folded: Vec<FoldedBytecode> = Vec::new();

        let mut bytecodes: Vec<IndexedBytecode> = Vec::new();

        let mut index = 0;

        while index < flagged.len() {
            let (indexed_bytecode, blocktype) = flagged.get(index).unwrap();

            if index == 0 && in_block {
                bytecodes.push(indexed_bytecode.clone());
                index += 1;
                continue;
            }

            match blocktype {
                BlockType::None => {
                    bytecodes.push(indexed_bytecode.clone());
                    index += 1;
                }, //Push the instruction
                BlockType::Loop(length) => {
                    //Beginning of a loop, assemble the previous bytecodes we've compiled in the bytecodes vec

                    if bytecodes.len() > 0 {
                        let mut swap_vec: Vec<IndexedBytecode> = Vec::new();
                        std::mem::swap(&mut bytecodes, &mut swap_vec);
                        folded.push(FoldedBytecode::Instructions(swap_vec));
                    }

                    folded.push(
                        FoldedBytecode::LoopBlock(Self::fold_bytecode(&flagged[index..], true))
                    );

                    index += length + 1;
                }
                BlockType::If(length) => {
                    //Beginning of a loop, assemble the previous bytecodes we've compiled in the bytecodes vec

                    bytecodes.push(
                        indexed_bytecode.clone()
                    );

                    dbg!(&bytecodes);

                    let mut swap_vec: Vec<IndexedBytecode> = Vec::new();
                    std::mem::swap(&mut bytecodes, &mut swap_vec);
                    folded.push(FoldedBytecode::Instructions(swap_vec));

                    folded.push(
                        FoldedBytecode::IfBlock(Self::fold_bytecode(&flagged[index+1..], true))
                    );

                    index += length + 1;
                }
            }
        }

        if bytecodes.len() > 0 {
            folded.push(FoldedBytecode::Instructions(bytecodes));
        }

        folded
    }

    pub fn convert(bytecode: &[u8]) -> Option<Vec<FoldedBytecode>> {
        let mut control_flow = Self {
            source_bytecode_to_byte: HashMap::new(),
            source_byte_to_bytecode: HashMap::new(),
            bytecode: Vec::new()
        };

        let bytecodes = Bytecode::from_bytes_with_indices(0, &bytecode).ok()?;

        bytecodes.into_iter().for_each(|(bytecode, index)| {
            let length = Bytecode::size_of(&bytecode);

            control_flow.source_bytecode_to_byte.insert(control_flow.bytecode.len(), index as usize);
            control_flow.source_byte_to_bytecode.insert(index as usize, control_flow.bytecode.len());

            control_flow.bytecode.push((bytecode, index as usize));
        });

        let flagged_bytecode: Vec<BytecodeWithBlockFlag> = control_flow.flag_bytecode();

        let folded_bytecode = ControlFlow::fold_bytecode(&flagged_bytecode[..], false);

        Option::Some(folded_bytecode)
    }

}

impl From<Vec<u8>> for ControlFlow {
    fn from(bytes: Vec<u8>) -> Self {
        unimplemented!()
    }
}

pub enum WasmIR {
    Instruction(BytecodeWithBlockFlag),
    Block(Vec<WasmIR>)
}

