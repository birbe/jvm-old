use walrus::ir::Instr;
use jvm::vm::vm::bytecode::Bytecode;
use std::collections::HashMap;
use std::io::{Cursor, Seek, SeekFrom};
use std::cmp::{min, max};
use crate::control_graph::{NodeGraph, Node};
use std::ops::Range;

pub type LengthOfBlock = usize;

#[derive(Clone, Debug)]
pub enum BlockType {
    None,
    Loop,
    Block
}

pub type IndexedBytecode = (Bytecode, usize);

type BytecodeWithBlockFlag = (IndexedBytecode, Vec<BlockType>);

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

pub type ScopedNode = (Node<IndexedBytecode>, usize, Vec<BlockType>);

impl ControlFlow {

    fn make_control_flow_graph(&mut self) -> NodeGraph<IndexedBytecode> {

        println!("Entered");

        let mut id = 0;

        let mut nodes: Vec<Node<IndexedBytecode>> = self.bytecode.iter().map(|bytecode| {
            let node = Node {
                id,
                front_edges: vec![],
                bytecode: bytecode.clone()
            };

            id += 1;

            node
        }).collect();

        for index in 0..self.bytecode.len() {
            let instr = self.bytecode.get(index).unwrap();

            let byte_index = *self.source_bytecode_to_byte.get(&index).unwrap();

            match &instr.0 {
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

                    if index < self.bytecode.len() - 1 { //Conditional, so also add the next instruction
                        nodes.get_mut(index).unwrap().front_edges.push(index + 1);
                    }

                    nodes.get_mut(index).unwrap().front_edges.push(target_bytecode_index);
                },
                Bytecode::Goto(offset) => {
                    let target_bytecode_index = *self.source_byte_to_bytecode.get(
                        &(( (byte_index as isize) + (*offset as isize) ) as usize)
                    ).unwrap();

                    nodes.get_mut(index).unwrap().front_edges.push(target_bytecode_index);
                }

                Bytecode::Jsr(_) => unimplemented!(),
                Bytecode::Jsr_w(_) => unimplemented!(),

                Bytecode::Lookupswitch(_, _) => {}
                Bytecode::Swap => {}
                Bytecode::Tableswitch => {}
                Bytecode::Wide(_) => {}

                _ => if index < self.bytecode.len() - 1 { //Non control-flow related instruction
                    nodes.get_mut(index).unwrap().front_edges.push(index + 1);
                }
            }
        }

        println!("Exited");

        NodeGraph {
            nodes
        }

    }

    //The first pass flags each location in the bytecode that requires a block
    fn flag_bytecode(&mut self, graph: NodeGraph<IndexedBytecode>) -> Vec<BytecodeWithBlockFlag> {

        let mut scoped: Vec<ScopedNode> = graph.nodes.into_iter().map(|node| {
            (node, 0, Vec::new())
        }).collect();

        let scoped_view = scoped.clone();

        let scopes: Vec<(Range<usize>, BlockType)> = Vec::new();

        //Define the scopes that each instruction is located in

        for index in 0..scoped_view.len() {
            let (node, _, _) = scoped_view.get(index).unwrap();

            node.front_edges.iter().for_each(|front_id| {
                if *front_id < index { //Loop, create a scope ending at this instruction and beginning at the target
                    (*front_id..index).for_each(|front_index| {
                        scoped.get_mut(front_index).unwrap().1 += 1;
                    });
                    scoped.get_mut(*front_id).unwrap().2.push(BlockType::Loop);
                } else if (*front_id - index) > 1 { //Jump forwards
                    let depth = scoped.get(index).unwrap().1;

                    let begin = 0;

                    // for node_index in index..=0 {
                    //     if scoped.get(node_index).unwrap().1 < depth {
                    //         begin = Option::Some(node_index + 1);
                    //         println!("Found a scope");
                    //         break;
                    //     } else if node_index == 0 {
                    //         begin = Option::Some(0);
                    //         println!("Defaulting to 0");
                    //     }
                    // }
                    //
                    // let begin = begin.expect(&format!("{} {:?}", index, scoped));

                    (begin..*front_id).for_each(|front_index| {
                        scoped.get_mut(front_index).unwrap().1 += 1;
                    });

                    scoped.get_mut(begin).unwrap().2.push(BlockType::Block);
                }
            });
        }

        scoped.into_iter().map(|scoped_node: ScopedNode| {
            (scoped_node.0.bytecode, scoped_node.2)
        }).collect()

    }

    fn fold_bytecode(flagged: &[BytecodeWithBlockFlag], in_block: bool) -> Vec<FoldedBytecode> {
        let mut folded: Vec<FoldedBytecode> = Vec::new();

        let mut bytecodes: Vec<IndexedBytecode> = Vec::new();

        let mut index = 0;

        while index < flagged.len() {
            let (indexed_bytecode, blocktypes) = flagged.get(index).unwrap();

            if index == 0 && in_block {
                bytecodes.push(indexed_bytecode.clone());
                index += 1;
                continue;
            }



            blocktypes.iter().for_each(|block| {

            });

            // match blocktype {
            //     BlockType::None => {
            //         bytecodes.push(indexed_bytecode.clone());
            //         index += 1;
            //     }, //Push the instruction
            //     BlockType::Loop(length) => {
            //         //Beginning of a loop, assemble the previous bytecodes we've compiled in the bytecodes vec
            //
            //         if bytecodes.len() > 0 {
            //             let mut swap_vec: Vec<IndexedBytecode> = Vec::new();
            //             std::mem::swap(&mut bytecodes, &mut swap_vec);
            //             folded.push(FoldedBytecode::Instructions(swap_vec));
            //         }
            //
            //         folded.push(
            //             FoldedBytecode::LoopBlock(Self::fold_bytecode(&flagged[index..], true))
            //         );
            //
            //         index += length + 1;
            //     }
            //     BlockType::Block(length) => {
            //         //Beginning of a loop, assemble the previous bytecodes we've compiled in the bytecodes vec
            //
            //         bytecodes.push(
            //             indexed_bytecode.clone()
            //         );
            //
            //         let mut swap_vec: Vec<IndexedBytecode> = Vec::new();
            //         std::mem::swap(&mut bytecodes, &mut swap_vec);
            //         folded.push(FoldedBytecode::Instructions(swap_vec));
            //
            //         folded.push(
            //             FoldedBytecode::IfBlock(Self::fold_bytecode(&flagged[index+1..], true))
            //         );
            //
            //         index += length + 1;
            //     }
            // }
        }

        unimplemented!();

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

        let graph = control_flow.make_control_flow_graph();
        let flagged_bytecode: Vec<BytecodeWithBlockFlag> = control_flow.flag_bytecode(graph);
        dbg!(&flagged_bytecode);

        // let folded_bytecode = ControlFlow::fold_bytecode(&flagged_bytecode[..], false);

        Option::Some(Vec::new())
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

