use jvm::vm::vm::bytecode::Bytecode;
use std::collections::HashMap;

use crate::control_graph::{Node, NodeGraph};
use std::ops::Range;

pub type LengthOfBlock = usize;

#[derive(Clone)]
pub enum ScopeType {
    If(Range<usize>),
    Block(Range<usize>),
    Loop(Range<usize>),
}

#[derive(Clone, Debug)]
pub enum BlockType {
    Loop(usize),
    Block(usize),
}

pub type IndexedBytecode = (Bytecode, usize);

#[derive(Clone, Debug)]
pub enum BytecodeOrBlock {
    Block(BlockType),
    Bytecode(IndexedBytecode),
}

#[derive(Debug)]
pub enum Relooped {
    Instructions(Range<usize>),
    LoopBlock(Vec<Relooped>),
    IfBlock(Vec<Relooped>),
    Block(Vec<Relooped>),
    Dispatcher(Vec<Relooped>),
}

pub struct ControlFlow {
    pub source_bytecode_to_byte: HashMap<usize, usize>,
    pub source_byte_to_bytecode: HashMap<usize, usize>,
    pub bytecode: Vec<IndexedBytecode>,
}

pub type ScopedNode = (Node<IndexedBytecode>, usize, Vec<ScopeType>);

enum StackMetricsError {
    DynamicStackSemantics, //When the operands of an instruction are only known at compile time
    Other,                 //I'm lazy
}

// fn create_stack_metrics(bytecode: Vec<Bytecode>) -> Result<Vec<(Bytecode, usize)>, StackMetricsError> {
//
//     let mut stack_size = 0isize;
//
//     Result::Ok(bytecode.into_iter().map(|b| {
//
//         let change: i8 = match &b {
//             Bytecode::Aaload => -1,
//             Bytecode::Aastore => -3,
//             Bytecode::Aconst_null => 1,
//             Bytecode::Aload(_) => 1,
//             Bytecode::Aload_n(_) => 1,
//             Bytecode::Anewarray(_) => 0,
//             Bytecode::Areturn => -1,
//             Bytecode::Arraylength => 0,
//             Bytecode::Astore(_) => -1,
//             Bytecode::Astore_n(_) => -1,
//             Bytecode::Athrow => 0,
//             Bytecode::Baload => -1,
//             Bytecode::Bastore => -3,
//             Bytecode::Bipush(_) => 1,
//             Bytecode::Caload => -1,
//             Bytecode::Castore => -3,
//             Bytecode::Checkcast(_) => 0,
//             Bytecode::D2f => 0,
//             Bytecode::D2i => 0,
//             Bytecode::D2l => 0,
//             Bytecode::Dadd => -1,
//             Bytecode::Daload => -1,
//             Bytecode::Dastore => -3,
//             Bytecode::Dcmpg => -1,
//             Bytecode::Dcmpl => -1,
//             Bytecode::Dconst_n(_) => 1,
//             Bytecode::Ddiv => -1,
//             Bytecode::Dload(_) => 1,
//             Bytecode::Dload_n(_) => 1,
//             Bytecode::Dmul => -1,
//             Bytecode::Dneg => -1,
//             Bytecode::Drem => -1,
//             Bytecode::Dreturn => -1,
//             Bytecode::Dstore(_) => -1,
//             Bytecode::Dstore_n(_) => -1,
//             Bytecode::Dsub => -1,
//             Bytecode::Dup => 1,
//             Bytecode::Dup_x2 => 1,
//             Bytecode::Dup2 => return Result::Err(StackMetricsError::DynamicStackSemantics),
//             Bytecode::Dup2_x1 => return Result::Err(StackMetricsError::DynamicStackSemantics),
//             Bytecode::Dup2_x2 => return Result::Err(StackMetricsError::DynamicStackSemantics),
//             Bytecode::F2d => 0,
//             Bytecode::F2i => 0,
//             Bytecode::F2l => 0,
//             Bytecode::Fadd => -1,
//             Bytecode::Faload => -1,
//             Bytecode::Fastore => -3,
//             Bytecode::Fcmpg => -1,
//             Bytecode::Fcmpl => -1,
//             Bytecode::Fconst_n(_) => 1,
//             Bytecode::Fdiv => -1,
//             Bytecode::Fload(_) => 1,
//             Bytecode::Fload_n(_) => 1,
//             Bytecode::Fmul => -1,
//             Bytecode::Fneg => 0,
//             Bytecode::Frem => -1,
//             Bytecode::Freturn => -1,
//             Bytecode::Fstore(_) => -1,
//             Bytecode::Fstore_n(_) => -1,
//             Bytecode::Fsub => -1,
//             Bytecode::Getfield(_) => 0,
//             Bytecode::Getstatic(_) => 1,
//             Bytecode::Goto(_) => 0,
//             Bytecode::Goto_w(_) => 0,
//             Bytecode::I2b => 0,
//             Bytecode::I2c => 0,
//             Bytecode::I2d => 0,
//             Bytecode::I2f => 0,
//             Bytecode::I2l => 0,
//             Bytecode::I2s => 0,
//             Bytecode::Iadd => -1,
//             Bytecode::Iaload => -1,
//             Bytecode::Iand => -1,
//             Bytecode::Iastore => -3,
//             Bytecode::Iconst_n_m1(_) => 1,
//             Bytecode::Idiv => -1,
//             Bytecode::If_acmpeq(_) => -2,
//             Bytecode::If_acmpne(_) => -2,
//             Bytecode::If_icmpeq(_) => -2,
//             Bytecode::If_icmpne(_) => -2,
//             Bytecode::If_icmplt(_) => -2,
//             Bytecode::If_icmpge(_) => -2,
//             Bytecode::If_icmpgt(_) => -2,
//             Bytecode::If_icmple(_) => -2,
//             Bytecode::Ifeq(_) => -1,
//             Bytecode::Ifne(_) => -1,
//             Bytecode::Iflt(_) => -1,
//             Bytecode::Ifge(_) => -1,
//             Bytecode::Ifgt(_) => -1,
//             Bytecode::Ifle(_) => -1,
//             Bytecode::Ifnonnull(_) => -1,
//             Bytecode::Ifnull(_) => -1,
//             Bytecode::Iinc(_, _) => 0,
//             Bytecode::Iload(_) => 1,
//             Bytecode::Iload_n(_) => 1,
//             Bytecode::Imul => -1,
//             Bytecode::Ineg => 0,
//             Bytecode::Instanceof(_) => 0,
//             Bytecode::Invokedynamic(_) => return Result::Err(StackMetricsError::Other),
//             Bytecode::Invokeinterface(_, _) => return Result::Err(StackMetricsError::Other),
//             Bytecode::Invokespecial(_) => return Result::Err(StackMetricsError::Other),
//             Bytecode::Invokestatic(_) => return Result::Err(StackMetricsError::Other),
//             Bytecode::Invokevirtual(_) => return Result::Err(StackMetricsError::Other),
//             Bytecode::Ior => -1,
//             Bytecode::Irem => -1,
//             Bytecode::Ireturn => -1,
//             Bytecode::Ishr => -1,
//             Bytecode::Ishl => -1,
//             Bytecode::Istore(_) => -1,
//             Bytecode::Istore_n(_) => -1,
//             Bytecode::Isub => -1,
//             Bytecode::Iushr => -1,
//             Bytecode::Ixor => -1,
//             Bytecode::Jsr(_) => 1,
//             Bytecode::Jsr_w(_) => 1,
//             Bytecode::L2d => 0,
//             Bytecode::L2f => 0,
//             Bytecode::L2i => 0,
//             Bytecode::Ladd => -1,
//             Bytecode::Laload => -1,
//             Bytecode::Land => -1,
//             Bytecode::Lastore => -3,
//             Bytecode::Lcmp => -1,
//             Bytecode::Lconst_n(_) => 1,
//             Bytecode::Ldc(_) => 1,
//             Bytecode::Ldc_w(_) => 1,
//             Bytecode::Ldc2_w(_) => 1,
//             Bytecode::Ldiv => -1,
//             Bytecode::Lload(_) => 1,
//             Bytecode::Lload_n(_) => 1,
//             Bytecode::Lmul => -1,
//             Bytecode::Lneg => 0,
//             Bytecode::Lookupswitch(_, _) => -1,
//             Bytecode::Lor => -1,
//             Bytecode::Lrem => -1,
//             Bytecode::Lreturn => -1,
//             Bytecode::Lshl => -1,
//             Bytecode::Lshr => -1,
//             Bytecode::Lstore => -1,
//             Bytecode::Lstore_n(_) => -1,
//             Bytecode::Lsub => -1,
//             Bytecode::Lushr => -1,
//             Bytecode::Lxor => -1,
//             Bytecode::Monitorenter => -1,
//             Bytecode::Monitorexit => -1,
//             Bytecode::Multianewarray(_, dimensions) => 1-*dimensions,
//             Bytecode::New(_) => 1,
//             Bytecode::Newarray(_) => 0,
//             Bytecode::Nop => 0,
//             Bytecode::Pop => -1,
//             Bytecode::Pop2 => Result::Err(StackMetricsError::DynamicStackSemantics),
//             Bytecode::Putfield(_) => -2,
//             Bytecode::Putstatic => -1,
//             Bytecode::Ret => 0,
//             Bytecode::Return => 0,
//             Bytecode::Saload => -1,
//             Bytecode::Sastore => -3,
//             Bytecode::Sipush(_) => 1,
//             Bytecode::Swap => 0,
//             Bytecode::Tableswitch => -1,
//             Bytecode::Wide(_) => Result::Err(StackMetricsError::Other) //TODO: can actually be implemented, I'm just lazy
//         };
//
//         Result::Ok((b, stack_size + change))
//     }).collect())
//
// }

impl ControlFlow {
    fn make_control_flow_graph(&mut self) -> NodeGraph<IndexedBytecode> {
        let mut id = 0;

        let mut nodes: Vec<Node<IndexedBytecode>> = self
            .bytecode
            .iter()
            .map(|bytecode| {
                let node = Node {
                    id,
                    front_edges: vec![],
                    bytecode: bytecode.clone(),
                };

                id += 1;

                node
            })
            .collect();

        for index in 0..self.bytecode.len() {
            let instr = self.bytecode.get(index).unwrap();

            let byte_index = *self.source_bytecode_to_byte.get(&index).unwrap();

            match &instr.0 {
                Bytecode::If_acmpeq(offset)
                | Bytecode::If_acmpne(offset)
                | Bytecode::If_icmpeq(offset)
                | Bytecode::If_icmpne(offset)
                | Bytecode::If_icmplt(offset)
                | Bytecode::If_icmpge(offset)
                | Bytecode::If_icmpgt(offset)
                | Bytecode::If_icmple(offset)
                | Bytecode::Ifeq(offset)
                | Bytecode::Ifne(offset)
                | Bytecode::Iflt(offset)
                | Bytecode::Ifge(offset)
                | Bytecode::Ifgt(offset)
                | Bytecode::Ifle(offset)
                | Bytecode::Ifnonnull(offset)
                | Bytecode::Ifnull(offset) => {
                    let target_bytecode_index = *self
                        .source_byte_to_bytecode
                        .get(&(((byte_index as isize) + (*offset as isize)) as usize))
                        .unwrap();

                    if index < self.bytecode.len() - 1 {
                        //Conditional, so also add the next instruction
                        nodes.get_mut(index).unwrap().front_edges.push(index + 1);
                    }

                    nodes
                        .get_mut(index)
                        .unwrap()
                        .front_edges
                        .push(target_bytecode_index);
                }
                Bytecode::Goto(offset) => {
                    let target_bytecode_index = *self
                        .source_byte_to_bytecode
                        .get(&(((byte_index as isize) + (*offset as isize)) as usize))
                        .unwrap();

                    nodes
                        .get_mut(index)
                        .unwrap()
                        .front_edges
                        .push(target_bytecode_index);
                }

                Bytecode::Jsr(_) => unimplemented!(),
                Bytecode::Jsr_w(_) => unimplemented!(),

                Bytecode::Lookupswitch(_, _) => {}
                Bytecode::Swap => {}
                Bytecode::Tableswitch => {}
                Bytecode::Wide(_) => {}

                _ => {
                    if index < self.bytecode.len() - 1 {
                        //Non control-flow related instruction
                        nodes.get_mut(index).unwrap().front_edges.push(index + 1);
                    }
                }
            }
        }

        NodeGraph { nodes }
    }

    //The first pass flags each location in the bytecode that requires a block
    fn mark_scopes(&mut self, graph: NodeGraph<IndexedBytecode>) {
        let mut scoped: Vec<ScopedNode> = graph
            .nodes
            .into_iter()
            .map(|node| (node, 0, Vec::new()))
            .collect();

        let scoped_view = scoped.clone();

        let _scopes: Vec<(Range<usize>, BlockType)> = Vec::new();

        //Define the scopes that each instruction is located in

        for index in 0..scoped_view.len() {
            let (node, _, _) = scoped_view.get(index).unwrap();

            node.front_edges.iter().for_each(|front_id| {
                if *front_id < index {
                    //Loop, create a scope ending at this instruction and beginning at the target
                    (*front_id..index).for_each(|node_index| {
                        let mut node = scoped.get_mut(node_index).unwrap();
                        node.1 += 1;
                        node.2.push(ScopeType::Loop(*front_id..index));
                    });
                } else if (*front_id - index) > 1 {
                    //Jump forwards, disregarding the directly following node
                    let depth = scoped.get(index).unwrap().1;

                    let mut begin = 0;

                    for node_index in index..=0 {
                        if scoped.get(node_index).unwrap().1 == depth {
                            begin = node_index;
                            break;
                        }
                    }

                    (begin..*front_id).for_each(|node_index| {
                        let node = scoped.get_mut(node_index).unwrap();
                        node.1 += 1;
                        node.2.push(ScopeType::Block(begin..*front_id));
                    });
                }
            });
        }
    }

    // fn fold_bytecode(flagged: &[BytecodeOrBlock], in_block: bool, depth: usize) -> Vec<Relooped> {
    //     let mut folded: Vec<Relooped> = Vec::new();
    //
    //     let mut bytecodes: Vec<IndexedBytecode> = Vec::new();
    //
    //     let mut index = 0;
    //
    //     if depth > 5 {
    //         panic!();
    //     }
    //
    //     while index < flagged.len() {
    //         let bytecode_or_block = flagged.get(index).unwrap();
    //
    //         match bytecode_or_block {
    //             BytecodeOrBlock::Block(block) => {
    //                 match block {
    //                     BlockType::Loop(length) | BlockType::Block(length) => {
    //                         if bytecodes.len() > 0 {
    //                             let mut swap_vec: Vec<IndexedBytecode> = Vec::new();
    //                             std::mem::swap(&mut bytecodes, &mut swap_vec);
    //                             folded.push(Relooped::Instructions(swap_vec));
    //                         }
    //
    //
    //                         let mut in_block: Vec<BytecodeOrBlock> = Vec::new();
    //                         {
    //                             let mut count = 0;
    //                             let mut look = 1;
    //
    //                             while count < *length {
    //                                 let bytecode_or_block = flagged.get(look + index).unwrap();
    //                                 in_block.push(bytecode_or_block.clone());
    //                                 match bytecode_or_block {
    //                                     BytecodeOrBlock::Block(_) => {}
    //                                     BytecodeOrBlock::Bytecode(_) => {
    //                                         count += 1;
    //                                     }
    //                                 }
    //                                 look += 1;
    //                                 println!("{}", look);
    //                             }
    //                         }
    //
    //                         folded.push(
    //                             match block {
    //                                 BlockType::Loop(_) => Relooped::LoopBlock(Self::fold_bytecode(&in_block[..], true, depth + 1)),
    //                                 BlockType::Block(_) => Relooped::Block(Self::fold_bytecode(&in_block[..], true, depth + 1))
    //                             }
    //                         );
    //
    //                         index += length + 1;
    //                     }
    //                 }
    //             }
    //             BytecodeOrBlock::Bytecode(bytecode) => {
    //                 bytecodes.push(bytecode.clone());
    //                 index += 1;
    //             }
    //         }
    //     }
    //
    //     if bytecodes.len() > 0 {
    //         folded.push(Relooped::Instructions(bytecodes));
    //     }
    //
    //     folded
    // }

    pub fn convert(bytecode: &[u8], _method: &str) -> Option<Vec<Relooped>> {
        let mut control_flow = Self {
            source_bytecode_to_byte: HashMap::new(),
            source_byte_to_bytecode: HashMap::new(),
            bytecode: Vec::new(),
        };

        let bytecodes = Bytecode::from_bytes_with_indices(0, &bytecode).ok()?;

        bytecodes.into_iter().for_each(|(bytecode, index)| {
            let _length = Bytecode::size_of(&bytecode);

            control_flow
                .source_bytecode_to_byte
                .insert(control_flow.bytecode.len(), index as usize);
            control_flow
                .source_byte_to_bytecode
                .insert(index as usize, control_flow.bytecode.len());

            control_flow.bytecode.push((bytecode, index as usize));
        });

        // let graph = control_flow.make_control_flow_graph();
        // let flagged_bytecode = control_flow.flag_bytecode(graph);

        // if method == "main" {
        //     dbg!(&flagged_bytecode);
        // }

        // let folded_bytecode = ControlFlow::fold_bytecode(&flagged_bytecode[..], false, 0);

        // if method == "main" {
        //     dbg!(&folded_bytecode);
        // }

        // Option::Some(folded_bytecode)
        Option::None
    }
}

impl From<Vec<u8>> for ControlFlow {
    fn from(_bytes: Vec<u8>) -> Self {
        unimplemented!()
    }
}
