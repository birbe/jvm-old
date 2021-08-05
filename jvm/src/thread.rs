use crate::heap::{Heap, Type, Reference, InternArrayType, JString};
use crate::linker::loader::{ClassLoader, ClassLoadState};
use std::ops::DerefMut;
use crate::class::{Method, Class, ClassError, MethodDescriptor, AccessFlags, ConstantPool};
use crate::class::attribute::AttributeItem;
use std::fmt::{Debug, Formatter};
use crate::class::constant::Constant;
use std::mem::size_of;
use std::convert::TryInto;
use bimap::BiMap;
use crate::class::FieldDescriptor;
use crate::{JavaType, LocalVariableMap};
use crate::{Operand, OperandType, JvmError};
use crate::bytecode::Bytecode;
use crate::class::{MethodReturnType, RefInfoType};
use crate::MethodError;
use std::time::{UNIX_EPOCH, SystemTime};

use std::io::Write;
use std::sync::Arc;
use parking_lot::RwLock;

pub struct Frame {
    pub local_vars: LocalVariableMap,
    pub method_name: String,
    pub op_stack: Vec<Operand>,
    pub code: Arc<Vec<(Bytecode, u16)>>,
    pub offset_bytecode_bimap: BiMap<u16, u16>,
    pub pc: u16,
    pub class: Arc<Class>,
}

impl Frame {

    pub fn get_code_cursor(&self) -> Arc<Vec<(Bytecode, u16)>> {
        self.code.clone()
    }

    pub fn get_class(&self) -> Arc<Class> {
        self.class.clone()
    }

    pub fn get_method_name(&self) -> &str {
        &self.method_name
    }

    pub fn get_operand_stack(&self) -> &[Operand] {
        &self.op_stack[..]
    }

    pub fn get_local_variables(&self) -> &LocalVariableMap {
        &self.local_vars
    }

}

impl Debug for Frame {

    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Class/Method: {}/{}\nOp stack: {:?}", &self.class.this_class, &self.method_name, &self.op_stack)
    }

}

///Attempt to resolve a class by reading first, then writing, finally erroring if necessary
macro_rules! lazy_class_resolve {
    ($classloader_rw:expr, $classname:expr) => {
        {
            use crate::linker::loader::ClassLoadReport;
            use std::collections::HashSet;

            let classloader_read = $classloader_rw.read();

            match classloader_read.get_class($classname) {
                None => {
                    drop(classloader_read);
                    $classloader_rw
                        .write()
                        .load_and_link_class($classname)?
                }
                Some(class) => {
                    drop(classloader_read);
                    ClassLoadReport {
                        just_loaded: false,
                        class,
                        all_loaded_classes: HashSet::new()
                    }
                }
            }
        }
    }
}

///Initializes classes whenever necessary
///This macro will step the instruction counter backwards if static(s) were loaded
///
///The operand stack should be unchanged at all times before this macro is invoked.
///
/// ```Rust
/// init_static!(frame, frame_borrow, class_load_report, self.frame_stack, { code });
/// ```
macro_rules! init_static {
    ($frame:ident, $frame_borrow:ident, $class_load_report:ident, $frame_stack:expr, $stdout:ident, $body:block) => {
        {
            let statics_to_init: Vec<Arc<Class>> = $class_load_report.all_loaded_classes.iter().filter(|&class|
                        class.has_method("<clinit>", "()V")
                    ).cloned().collect();

            if statics_to_init.len() > 0 {
                $frame.pc -= 1;

                drop($frame);
                drop($frame_borrow);

                statics_to_init.iter().for_each(|class| {
                    $frame_stack.push(RwLock::new(
                        RuntimeThread::<Stdout>::create_frame(
                            class.get_method("<clinit>", "()V").unwrap().clone(),
                            class.clone()
                        )
                    ));
                });
            } else {
                $body
            }
        }
    }
}

pub struct RuntimeThread<Stdout: Write + Send + Sync> {
    heap: Arc<Heap>,
    classloader: Arc<RwLock<ClassLoader>>,
    std_out: Arc<RwLock<Stdout>>,

    thread_name: String,
    frame_stack: Vec<RwLock<Frame>>,
}

impl<Stdout: Write + Send + Sync> RuntimeThread<Stdout> {
    pub fn new(
        name: String,
        heap: Arc<Heap>,
        classloader: Arc<RwLock<ClassLoader>>,
        std_out: Arc<RwLock<Stdout>>
    ) -> RuntimeThread<Stdout> {
        RuntimeThread {
            heap,
            classloader,

            std_out,
            thread_name: name,
            frame_stack: Vec::new(),
        }
    }

    pub fn create_frame(method: Arc<Method>, class: Arc<Class>) -> Frame {
        let attr = method
            .attribute_map
            .get("Code")
            .expect("Method did not have a Code attribute! Is native?");

        let code_attr = if let AttributeItem::Code(code) = &attr.info {
            code
        } else {
            unreachable!("Code attribute did not resolve to Attribute::Code variant!")
        };

        let mut index: u16 = 0;
        let offset_bytecode_bimap = code_attr.code.iter().map(|(_, offset)| {
            index += 1;
            (index - 1, *offset)
        }).collect();

        Frame {
            class,
            code: code_attr.code.clone(),
            method_name: method.name.clone(),
            local_vars: LocalVariableMap::new(),
            op_stack: Vec::new(),
            offset_bytecode_bimap,
            pc: 0
        }
    }

    pub fn add_frame(&mut self, frame: Frame) {
        self.frame_stack.push(RwLock::new(frame))
    }

    pub fn step(&mut self) -> Result<(), JvmError> {
        let mut frame_write = self
            .frame_stack
            .last()
            .ok_or(JvmError::EmptyFrameStack)?
            .write();

        let frame = frame_write.deref_mut();

        let opcode_pos = frame.pc;

        if opcode_pos as usize == frame.code.len() {
            drop(frame_write);

            self.frame_stack.pop();

            return Result::Ok(());
        }

        let opcode = &frame.code.get(frame.pc as usize).unwrap().0;

        frame.pc += 1;

        match opcode {
            Bytecode::Nop => (),
            Bytecode::Aconst_null => frame.op_stack.push(Operand(OperandType::NullReference, 0)), //aconst_null,
            Bytecode::Iconst_n_m1(int) => frame
                .op_stack
                .push(Operand(OperandType::Int, *int as usize)),
            Bytecode::Lconst_n(long) => {
                frame.op_stack.push(Operand(OperandType::Long, 0));
                frame
                    .op_stack
                    .push(Operand(OperandType::Long, *long as usize));
            }
            Bytecode::Fconst_n(float) => frame
                .op_stack
                .push(Operand(OperandType::Float, *float as usize)), //fconst_<n>
            Bytecode::Dconst_n(double) => {
                //dconst_<n>
                frame.op_stack.push(Operand(OperandType::Double, 0));
                frame
                    .op_stack
                    .push(Operand(OperandType::Double, *double as usize));
            }
            Bytecode::Bipush(byte) => frame
                .op_stack
                .push(Operand(OperandType::Int, *byte as usize)), //bipush
            Bytecode::Sipush(short) => frame.op_stack.push(Operand(
                OperandType::Int,
                *short as usize,
            )),
            Bytecode::Ldc(index) => {
                let constant = frame
                    .class
                    .constant_pool
                    .get(*index as usize)
                    .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;

                match constant {
                    Constant::Integer(int) => frame
                        .op_stack
                        .push(Operand(OperandType::Int, *int as usize)),
                    Constant::Float(float) => frame
                        .op_stack
                        .push(Operand(OperandType::Float, *float as usize)),
                    Constant::String(str_index) => {
                        let string = frame
                            .class
                            .constant_pool
                            .resolve_utf8(*str_index)
                            .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;

                        let string_map_read = self.heap.strings.read();

                        match string_map_read.get(string) {
                            None => {
                                drop(string_map_read);

                                let string_reference = self.heap.create_string(
                                    string,
                                    self.classloader
                                        .read()
                                        .get_class("java/lang/String")
                                        .ok_or(JvmError::ClassLoadError(
                                            ClassLoadState::NotLoaded,
                                        ))?,
                                )?;

                                self.heap
                                    .strings
                                    .write()
                                    .insert(String::from(string), string_reference);

                                frame.op_stack.push(Operand(
                                    OperandType::ClassReference,
                                    string_reference as usize,
                                ));
                            }
                            Some(&string_reference) => {
                                frame.op_stack.push(Operand(
                                    OperandType::ClassReference,
                                    string_reference as usize,
                                ));
                            }
                        }
                    }
                    Constant::Utf8(_) => {}
                    Constant::Long(_) => {}
                    Constant::Double(_) => {}
                    Constant::Class(_) => {}
                    Constant::FieldRef(_, _) => {}
                    Constant::MethodRef(_, _) => {}
                    Constant::InterfaceMethodRef(_, _) => {}
                    Constant::NameAndType(_, _) => {}
                    Constant::MethodHandle(_, _) => {}
                    Constant::MethodType(_) => {}
                    Constant::InvokeDynamic(_, _) => {}
                }
            }
            ////iload_<n> ; Load int from local variables
            Bytecode::Iload(local) => {
                let local_var = frame
                    .local_vars
                    .get(&(*local as u16))
                    .ok_or(JvmError::InvalidLocalVariable)?;

                if let Type::Int(int) = local_var {
                    //<n> = 0..3
                    frame
                        .op_stack
                        .push(Operand(OperandType::Int, *int as usize))
                } else {
                    panic!("iload_n command did not resolve to an int!")
                }
            }
            //lload_<n> ; Load long from local variables
            Bytecode::Lload_n(local) => {
                if let Type::LongHalf(lhalf1) = frame
                    .local_vars
                    .get(&(*local as u16))
                    .ok_or(JvmError::InvalidLocalVariable)?
                {
                    //<n> = 0..3
                    if let Type::LongHalf(lhalf2) = frame
                        .local_vars
                        .get(&(*local as u16 + 1))
                        .ok_or(JvmError::InvalidLocalVariable)?
                    {
                        //<n> = 0..3
                        frame
                            .op_stack
                            .push(Operand(OperandType::Long, *lhalf1 as usize));
                        frame
                            .op_stack
                            .push(Operand(OperandType::Long, *lhalf2 as usize));
                    } else {
                        panic!("lload_n command did not resolve to a long!")
                    }
                } else {
                    panic!("lload_n command did not resolve to a long!")
                }
            }
            //fload_<n> ; Load float from local variables
            Bytecode::Fload_n(local) => {
                if let Type::Float(float) = frame
                    .local_vars
                    .get(&(*local as u16))
                    .ok_or(JvmError::InvalidLocalVariable)?
                {
                    //<n> = 0..3
                    frame
                        .op_stack
                        .push(Operand(OperandType::Float, *float as usize));
                } else {
                    panic!("fload_n command did not resolve to an float!")
                }
            }
            //dload_<n> ; Load double from local variables
            Bytecode::Dload_n(double) => {
                if let Type::DoubleHalf(dhalf1) = frame
                    .local_vars
                    .get(&(*double as u16))
                    .ok_or(JvmError::InvalidLocalVariable)?
                {
                    if let Type::DoubleHalf(dhalf2) = frame
                        .local_vars
                        .get(&(*double as u16 + 1))
                        .ok_or(JvmError::InvalidLocalVariable)?
                    {
                        frame
                            .op_stack
                            .push(Operand(OperandType::Double, *dhalf1 as usize));
                        frame
                            .op_stack
                            .push(Operand(OperandType::Double, *dhalf2 as usize));
                    } else {
                        panic!("dload_n command did not resolve to a double!")
                    }
                } else {
                    panic!("dload_n command did not resolve to a double!")
                }
            }
            //aload_<n> ; Load reference
            Bytecode::Aload_n(local) => {
                let var = frame.local_vars.get(&(*local as u16)).unwrap_or_else(|| {
                    panic!(
                        "Class: {}\nMethod: {}\nIndex: {}",
                        frame.class.this_class, frame.method_name, opcode_pos
                    )
                });

                if let Type::Reference(reference) = var {
                    match reference {
                        Reference::Class(ptr) => frame
                            .op_stack
                            .push(Operand(OperandType::ClassReference, (*ptr) as usize)),
                        Reference::Null => {
                            frame.op_stack.push(Operand(OperandType::NullReference, 0))
                        }
                        Reference::Interface(ptr) => frame
                            .op_stack
                            .push(Operand(OperandType::InterfaceReference, *ptr as usize)),
                        Reference::Array(ptr) => frame
                            .op_stack
                            .push(Operand(OperandType::ArrayReference, *ptr as usize)),
                    }
                } else {
                    panic!("aload_<n> local variable did not resolve to a reference!")
                };
            }
            Bytecode::Faload => {
                let arr_ptr = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as *mut u8;
                let index = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as usize;

                unsafe {
                    let float = (arr_ptr.offset((size_of::<f32>() * index).try_into().unwrap()))
                        as *mut f32;
                    frame
                        .op_stack
                        .push(Operand(OperandType::Float, *float as usize));
                }
            }
            Bytecode::Daload => {
                let arr_ptr = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as *mut u8;
                let index = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as usize;

                unsafe {
                    let double = *((arr_ptr.offset((size_of::<u64>() * index).try_into().unwrap()))
                        as *mut u64);

                    let dhalf1 = (double >> 32) as u32 as usize;
                    let dhalf2 = double as u32 as usize;

                    frame.op_stack.push(Operand(OperandType::Double, dhalf1));
                    frame.op_stack.push(Operand(OperandType::Double, dhalf2));
                }
            }
            Bytecode::Aaload => {
                //aaload (load reference from an array)
                let index: isize = frame
                    .op_stack
                    .pop()
                    .ok_or(JvmError::EmptyOperandStack)?
                    .1
                    .try_into()
                    .unwrap();
                let arr_ptr = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as *mut u8;

                let (header, body) = unsafe { Heap::get_array::<usize>(arr_ptr) };

                let header_id = unsafe { (*header).id };

                let ref_type = match InternArrayType::from_u8(header_id) {
                    InternArrayType::ArrayReference => OperandType::ArrayReference,
                    InternArrayType::ClassReference => OperandType::ClassReference,
                    InternArrayType::InterfaceReference => OperandType::InterfaceReference,
                    _ => panic!("Reference in array was not a reference."),
                };

                let usize_size: isize = size_of::<usize>().try_into().unwrap();
                let element = unsafe { body.offset(usize_size * index) };

                frame.op_stack.push(Operand(ref_type, element as usize));
            }
            //caload
            Bytecode::Caload => {
                let index = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                let arrayref = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;

                let (_, ptr) = unsafe { Heap::get_array::<u16>(arrayref.1 as *mut u8) };

                let val = unsafe { *ptr.offset(index.1.try_into().unwrap()) };
                frame.op_stack.push(Operand(OperandType::Int, val as usize));
            }
            //istore_<n>
            Bytecode::Istore_n(local) => {
                let value = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as i32;
                frame.local_vars.insert(*local as u16, vec![Type::Int(value)]);
            }
            //lstore_<n>
            Bytecode::Lstore_n(local) => {
                let long = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1;
                frame.local_vars.insert(
                    *local as u16,
                    vec![
                        Type::LongHalf((long >> 32) as u32),
                        Type::LongHalf((long & 0x7fffffff) as u32),
                    ],
                );
            }
            Bytecode::Astore_n(local) | Bytecode::Astore(local) => {
                let object_ref = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;

                frame
                    .local_vars
                    .insert(*local as u16, Operand::as_type(object_ref));
            }
            //castore
            Bytecode::Castore => {
                let val = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                let index: isize = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1.try_into().unwrap();
                let arrayref = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;

                unsafe {
                    let (header, ptr) = Heap::get_array::<u16>(arrayref.1 as *mut u8);
                    if index >= (*header).size.try_into().unwrap() {
                        panic!("Array access out of bounds");
                    }
                    let offset = ptr.offset(index);
                    *offset = val.1 as u16;
                }
            }
            Bytecode::Pop => {
                frame.op_stack.pop();
            }
            Bytecode::Pop2 => {
                let first = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                if first.get_category() == 1 {
                    frame.op_stack.pop();
                }
            }
            //dup
            Bytecode::Dup => {
                let op = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                frame.op_stack.push(op);
                frame.op_stack.push(op);
            }
            Bytecode::Iadd => {
                let int1 = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as i32;
                let int2 = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as i32;

                frame
                    .op_stack
                    .push(Operand(OperandType::Int, int1.wrapping_add(int2) as usize));
            }
            Bytecode::Ladd => {
                let long1 = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as i32;
                let long2 = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as i32;

                frame
                    .op_stack
                    .push(Operand(OperandType::Int, long1.wrapping_add(long2) as usize));
            }
            Bytecode::Iinc(local, byte) => {
                let var = frame
                    .local_vars
                    .get(&(*local as u16))
                    .ok_or(JvmError::InvalidLocalVariable)?;
                if let Type::Int(int) = var {
                    let num = *int + *byte as i32;
                    frame.local_vars.insert(*byte as u16, vec![Type::Int(num)]);
                } else {
                    panic!("Local variable used in iinc was not an int!")
                }
            }
            Bytecode::Ifeq(branch)
            | Bytecode::Ifne(branch)
            | Bytecode::Iflt(branch)
            | Bytecode::Ifge(branch)
            | Bytecode::Ifgt(branch)
            | Bytecode::Ifle(branch) => {
                let value = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as i32;

                if match opcode {
                    Bytecode::Ifeq(_) => value == 0,
                    Bytecode::Ifne(_) => value != 0,
                    Bytecode::Iflt(_) => value < 0,
                    Bytecode::Ifle(_) => value >= 0,
                    Bytecode::Ifgt(_) => value > 0,
                    Bytecode::Ifge(_) => value >= 0,
                    _ => unreachable!()
                } {
                    frame.pc.wrapping_add(*branch as u16);
                }
            },
            Bytecode::If_icmpeq(branch)
            | Bytecode::If_icmpne(branch)
            | Bytecode::If_icmplt(branch)
            | Bytecode::If_icmpge(branch)
            | Bytecode::If_icmpgt(branch)
            | Bytecode::If_icmple(branch) => {
                let value2 = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as i32;
                let value1 = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1 as i32;

                if match opcode {
                    Bytecode::If_icmpeq(_) => value1 == value2,
                    Bytecode::If_icmpne(_) => value1 != value2,
                    Bytecode::If_icmplt(_) => value1 < value2,
                    Bytecode::If_icmple(_) => value1 >= value2,
                    Bytecode::If_icmpgt(_) => value1 > value2,
                    Bytecode::If_icmpge(_) => value1 >= value2,
                    _ => unreachable!()
                } {
                    frame.pc.wrapping_add(*branch as u16);
                }
            },
            Bytecode::Goto(index) => {
                frame.pc.wrapping_add(*index as u16);
            }
            Bytecode::Ireturn => {
                let op = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                drop(frame_write);
                self.frame_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                let mut invoker = self
                    .frame_stack
                    .last()
                    .ok_or(JvmError::EmptyOperandStack)?
                    .write();
                invoker.op_stack.push(op);
            }
            //areturn
            Bytecode::Areturn => {
                let op = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                drop(frame_write);
                self.frame_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                let mut invoker = self
                    .frame_stack
                    .last()
                    .ok_or(JvmError::EmptyOperandStack)?
                    .write();
                invoker.op_stack.push(op);
            }
            Bytecode::Return => { //return void
            }
            //getstatic
            Bytecode::Getstatic(index) => {
                let field = frame
                    .class
                    .constant_pool
                    .resolve_ref_info(*index as usize)
                    .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;

                let class_name = field.class_name.to_owned();

                let class_load_report = lazy_class_resolve!(self.classloader, &class_name);

                init_static!(frame, frame_write, class_load_report, self.frame_stack, Stdout, {
                    let fd = FieldDescriptor::parse(field.descriptor)?;

                    let class = &class_load_report.class;

                    match &fd {
                        FieldDescriptor::JavaType(bt) => match bt {
                            JavaType::Byte => {
                                let result = unsafe {
                                    self.heap.get_static::<u8>(&class.this_class, field.name)
                                };
                                let ot = match result {
                                    None => OperandType::NullReference,
                                    Some(_) => OperandType::Int,
                                };
                                frame
                                    .op_stack
                                    .push(Operand(ot, result.unwrap_or(0) as usize));
                            }
                            JavaType::Char => {
                                let result = unsafe {
                                    self.heap.get_static::<u16>(&class.this_class, field.name)
                                };
                                let ot = match result {
                                    None => OperandType::NullReference,
                                    Some(_) => OperandType::Int,
                                };
                                frame
                                    .op_stack
                                    .push(Operand(ot, result.unwrap_or(0) as usize));
                            }
                            JavaType::Double => {
                                let result = unsafe {
                                    self.heap.get_static::<u64>(&class.this_class, field.name)
                                };
                                let ot = match result {
                                    None => OperandType::NullReference,
                                    Some(_) => OperandType::Double,
                                };
                                frame
                                    .op_stack
                                    .push(Operand(ot, result.unwrap_or(0) as usize));
                            }
                            JavaType::Float => {
                                let result = unsafe {
                                    self.heap.get_static::<u32>(&class.this_class, field.name)
                                };
                                let ot = match result {
                                    None => OperandType::NullReference,
                                    Some(_) => OperandType::Float,
                                };
                                frame
                                    .op_stack
                                    .push(Operand(ot, result.unwrap_or(0) as usize));
                            }
                            JavaType::Int => {
                                let result = unsafe {
                                    self.heap.get_static::<u32>(&class.this_class, field.name)
                                };
                                let ot = match result {
                                    None => OperandType::NullReference,
                                    Some(_) => OperandType::Int,
                                };
                                frame
                                    .op_stack
                                    .push(Operand(ot, result.unwrap_or(0) as usize));
                            }
                            JavaType::Long => {
                                let result = unsafe {
                                    self.heap.get_static::<u64>(&class.this_class, field.name)
                                };
                                let ot = match result {
                                    None => OperandType::NullReference,
                                    Some(_) => OperandType::Long,
                                };
                                frame
                                    .op_stack
                                    .push(Operand(ot, result.unwrap_or(0) as usize));
                            }
                            JavaType::Reference => unreachable!(),
                            JavaType::Bool => {
                                let result = unsafe {
                                    self.heap.get_static::<u8>(&class.this_class, field.name)
                                };
                                let ot = match result {
                                    None => OperandType::NullReference,
                                    Some(_) => OperandType::Int,
                                };
                                frame
                                    .op_stack
                                    .push(Operand(ot, result.unwrap_or(0) as usize));
                            }
                            JavaType::Short => {
                                let result = unsafe {
                                    self.heap.get_static::<u16>(&class.this_class, field.name)
                                };
                                let ot = match result {
                                    None => OperandType::NullReference,
                                    Some(_) => OperandType::Int,
                                };
                                frame
                                    .op_stack
                                    .push(Operand(ot, result.unwrap_or(0) as usize));
                            }
                        },
                        FieldDescriptor::ObjectType(_) => {
                            let result = unsafe {
                                self.heap.get_static::<usize>(&class.this_class, field.name)
                            };
                            let ot = match result {
                                None => OperandType::NullReference,
                                Some(_) => OperandType::ClassReference,
                            };
                            frame
                                .op_stack
                                .push(Operand(ot, result.unwrap_or(0) as usize));
                        }
                        FieldDescriptor::ArrayType(_) => {
                            let result = unsafe {
                                self.heap.get_static::<usize>(&class.this_class, field.name)
                            };
                            let ot = match result {
                                None => OperandType::NullReference,
                                Some(_) => OperandType::ArrayReference,
                            };
                            frame
                                .op_stack
                                .push(Operand(ot, result.unwrap_or(0) as usize));
                        }
                    };
                });
            }
            //putstatic
            Bytecode::Putstatic(index) => {
                let field = frame
                    .class
                    .constant_pool
                    .resolve_ref_info(*index as usize)
                    .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;

                let class_load_report = lazy_class_resolve!(self.classloader, field.class_name);

                init_static!(frame, frame_write, class_load_report, self.frame_stack, Stdout, {
                    let value = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                    let fd = FieldDescriptor::parse(field.descriptor)?;

                    let class = class_load_report.class.clone();

                    unsafe {
                        match &fd {
                            FieldDescriptor::JavaType(bt) => match bt {
                                JavaType::Byte => self.heap.put_static(
                                    &class.this_class,
                                    field.name,
                                    &fd,
                                    value.1 as u8,
                                ),
                                JavaType::Char => self.heap.put_static(
                                    &class.this_class,
                                    field.name,
                                    &fd,
                                    value.1 as u16,
                                ),
                                JavaType::Double => self.heap.put_static(
                                    &class.this_class,
                                    field.name,
                                    &fd,
                                    value.1 as u64,
                                ),
                                JavaType::Float => self.heap.put_static(
                                    &class.this_class,
                                    field.name,
                                    &fd,
                                    value.1 as u32,
                                ),
                                JavaType::Int => self.heap.put_static(
                                    &class.this_class,
                                    field.name,
                                    &fd,
                                    value.1 as u32,
                                ),
                                JavaType::Long => self.heap.put_static(
                                    &class.this_class,
                                    field.name,
                                    &fd,
                                    value.1 as u64,
                                ),
                                JavaType::Reference => self.heap.put_static(
                                    &class.this_class,
                                    field.name,
                                    &fd,
                                    value.1,
                                ),
                                JavaType::Bool => self.heap.put_static(
                                    &class.this_class,
                                    field.name,
                                    &fd,
                                    value.1 as u8,
                                ),
                                JavaType::Short => self.heap.put_static(
                                    &class.this_class,
                                    field.name,
                                    &fd,
                                    value.1 as u16,
                                ),
                            },
                            FieldDescriptor::ObjectType(_) => {
                                self.heap
                                    .put_static(&class.this_class, field.name, &fd, value.1)
                            }
                            FieldDescriptor::ArrayType(_) => {
                                self.heap
                                    .put_static(&class.this_class, field.name, &fd, value.1)
                            }
                        };
                    }
                });
            },
            Bytecode::Getfield(index) => {
                //TODO: type checking, exceptions

                let fieldref = frame
                    .class
                    .constant_pool
                    .resolve_ref_info(*index as usize)
                    .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;


                let object_ref = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1;

                let object = self
                    .heap
                    .objects
                    .get(object_ref)
                    .ok_or(JvmError::InvalidObjectReference)?;

                let reference = object
                    .get_field(fieldref.name)
                    .ok_or_else(|| JvmError::InvalidObjectField(String::from(fieldref.name)))?
                    .as_reference()
                    .ok_or(JvmError::InvalidObjectReference)?;

                let fd = FieldDescriptor::parse(fieldref.descriptor)?;

                let operand = match fd {
                    FieldDescriptor::JavaType(base_type) => {
                        Operand(OperandType::from_base_type(base_type), reference)
                    }
                    FieldDescriptor::ObjectType(_object_type) => {
                        Operand(OperandType::ClassReference, reference)
                    }
                    FieldDescriptor::ArrayType(_) => panic!("Not allowed."),
                };

                frame.op_stack.push(operand);
            },
            Bytecode::Putfield(index) => {
                //TODO: type checking, exceptions

                let fieldref = frame
                    .class
                    .constant_pool
                    .resolve_ref_info(*index as usize)
                    .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;

                let class_load_report = lazy_class_resolve!(self.classloader, fieldref.class_name);

                init_static!(frame, frame_write, class_load_report, self.frame_stack, Stdout, {
                    let value = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                    let object_ref = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1;

                    self.heap
                        .put_field(object_ref, class_load_report.class, fieldref.name, value.1);
                });
            }
            //invokevirtual
            Bytecode::Invokevirtual(index) => {
                let method_ref = frame
                    .class
                    .constant_pool
                    .resolve_ref_info(*index as usize)
                    .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;

                let class_load_report = lazy_class_resolve!(self.classloader, method_ref.class_name);

                init_static!(frame, frame_write, class_load_report, self.frame_stack, Stdout, {

                    let class = class_load_report.class;

                    let method_descriptor = MethodDescriptor::parse(method_ref.descriptor)?;

                    let param_len = method_descriptor.parameters.len();

                    let mut parameter_stack = Vec::with_capacity(param_len);

                    for i in 0..param_len {
                        let param = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                        let descriptor = method_descriptor
                            .parameters
                            .get(i)
                            .ok_or(JvmError::EmptyOperandStack)?;

                        if !descriptor.matches_operand(param.0) {
                            panic!("Operand did not match parameter requirements.");
                        }

                        parameter_stack.insert(param_len - i - 1, param);
                    }

                    let object_ref = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;

                    let object_info = self
                        .heap
                        .objects
                        .get(object_ref.1)
                        .ok_or(JvmError::InvalidObjectReference)?;

                    if method_ref.info_type != RefInfoType::MethodRef {
                        panic!("Invokevirtual must reference a MethodRef!");
                    }

                    if method_ref.name == "<init>" || method_ref.name == "<clinit>" {
                        panic!("Invokevirtual must not invoke an initialization method!");
                    }

                    let method = class.get_method(method_ref.name, method_ref.descriptor)?;

                    let method_descriptor = MethodDescriptor::parse(&method.descriptor)?;

                    // if AccessFlags::is_protected(method.access_flags) {
                    //     let is_superclass = jvm.recurse_is_superclass(frame.class.clone(), &method_ref.class_name);
                    //     let are_siblings = VirtualMachine::are_classpaths_siblings(&frame.class.this_class, &method_ref.class_name);
                    //
                    //     if is_superclass && !are_siblings {
                    //         if frame.class.this_class != object_info.class.this_class && !jvm.recurse_is_superclass(jvm.get_class(&object_info.class.this_class).clone(), &frame.class.this_class) {
                    //             // panic!("AbstractMethodError");
                    //         }
                    //     }
                    // }

                    //Is polymorphic
                    //There has to be a better way to do this
                    let polymorphic: bool = {
                        if class.this_class == "java/lang/invoke/MethodHandle"
                            && method_descriptor.parameters.len() == 1
                        {
                            if let FieldDescriptor::ArrayType(at) = method_descriptor
                                .parameters
                                .first()
                                .ok_or(JvmError::MethodError(MethodError::InvalidDescriptor))?
                            {
                                if at.dimensions == 1 {
                                    if let FieldDescriptor::ObjectType(ot) = &*at.field_descriptor {
                                        if ot == "java/lang/Object" {
                                            match method_descriptor.return_type {
                                                MethodReturnType::FieldDescriptor(return_fd) => {
                                                    match return_fd {
                                                        FieldDescriptor::ObjectType(return_ot)
                                                            if return_ot == "java/lang/Object" =>
                                                        {
                                                            method.access_flags & 0x180 == 0x180
                                                        }

                                                        _ => false,
                                                    }
                                                }
                                                _ => false,
                                            }
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };

                    if !polymorphic {
                        let (resolved_class, method_to_invoke) = self
                            .classloader
                            .read()
                            .recurse_resolve_overridding_method(
                                object_info.class.clone(),
                                &method.name,
                                &method.descriptor,
                            )
                            .ok_or(JvmError::ClassLoadError(ClassLoadState::NotLoaded))?;

                        let mut new_frame =
                            RuntimeThread::<Stdout>::create_frame(method_to_invoke, resolved_class);

                        new_frame
                            .local_vars
                            .insert(0, vec![Type::Reference(Reference::Class(object_ref.1))]);

                        let mut index = 1;

                        for item in parameter_stack {
                            new_frame.local_vars.insert(index, Operand::as_type(item));
                            index += 1;
                        }

                        drop(frame_write);
                        self.frame_stack.push(RwLock::new(new_frame));
                    }
                });
            }
            //invokespecial https://docs.oracle.com/javase/specs/jvms/se7/html/jvms-6.html#jvms-6.5.invokespecial
            Bytecode::Invokespecial(index) => {
                let method_ref = frame
                    .class
                    .constant_pool
                    .resolve_ref_info(*index as usize)
                    .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;

                let method_descriptor = MethodDescriptor::parse(method_ref.descriptor)?;

                // println!("{}", method_d);

                let class_load_report = lazy_class_resolve!(self.classloader, method_ref.class_name);

                init_static!(frame, frame_write, class_load_report, self.frame_stack, Stdout, {

                    let method_class = &class_load_report.class;

                    let classloader = self.classloader.read();

                    //TODO: clean this up, this sucks
                    let resolved_method =
                        method_class.get_method(method_ref.name, method_ref.descriptor)?;

                    let to_invoke: (Arc<Class>, Arc<Method>) = if (method_class.access_flags & 0x20
                        == 0x20)
                        && classloader
                            .recurse_is_superclass(frame.class.clone(), method_class.clone())
                        && resolved_method.name != "<init>"
                    {
                        classloader
                            .recurse_resolve_supermethod_special(
                                classloader
                                    .get_class(frame.class.super_class.as_deref().unwrap())
                                    .ok_or(JvmError::ClassLoadError(ClassLoadState::NotLoaded))?,
                                &resolved_method.name,
                                &resolved_method.descriptor,
                            )
                            .ok_or(JvmError::UnresolvedSuper)?
                    } else {
                        (method_class.clone(), resolved_method)
                    };

                    let mut new_frame = RuntimeThread::<Stdout>::create_frame(to_invoke.1, to_invoke.0);

                    let param_len = method_descriptor.parameters.len();

                    for i in 0..param_len {
                        let param = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                        let descriptor = method_descriptor
                            .parameters
                            .get(i)
                            .ok_or(JvmError::MethodError(MethodError::InvalidDescriptor))?;

                        if !descriptor.matches_operand(param.0) {
                            panic!("Operand did not match parameter requirements.");
                        }

                        new_frame
                            .local_vars
                            .insert((param_len - i) as u16, Operand::as_type(param));
                    }

                    new_frame.local_vars.insert(
                        0,
                        Operand::as_type(frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?),
                    ); //the objectref

                    drop(frame_write);

                    self.frame_stack.push(RwLock::new(new_frame));
                });
            }
            Bytecode::Invokestatic(index) => {
                let method_ref = frame
                    .class
                    .constant_pool
                    .resolve_ref_info(*index as usize)
                    .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;

                let class_load_report = lazy_class_resolve!(self.classloader, method_ref.class_name);

                init_static!(frame, frame_write, class_load_report, self.frame_stack, Stdout, {

                    let class = &class_load_report.class;

                    let md = MethodDescriptor::parse(method_ref.descriptor)?;
                    let method = class.get_method(method_ref.name, method_ref.descriptor)?;

                    if AccessFlags::is_native(method.access_flags) {
                        //Is native
                        let mut argument_stack: Vec<Operand> = Vec::new();
                        for _ in md.parameters.iter() {
                            argument_stack
                                .push(frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?);
                        }

                        let name = &format!("{}|{}", class.this_class, method.name)[..];

                        let operands_out = match name {
                            "Main|panic" => panic!("Java caused panic"),
                            "Main|print_int" => {
                                println!(
                                    "Main_print_int({})",
                                    argument_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1
                                );

                                Option::None
                            }
                            "Main|print_string" => {
                                let operand =
                                    argument_stack.pop().ok_or(JvmError::EmptyOperandStack)?;
                                let string_ref = operand.into_class_reference().unwrap();
                                let string_obj = self.heap.objects.get(string_ref).unwrap();

                                if string_obj.class.this_class == "java/lang/String" {
                                    let jstring: JString = string_obj.as_jstring().unwrap();
                                    let mut string: String = jstring.into();
                                    string.push('\n');

                                    self.std_out.write().write(string.as_bytes()).unwrap();
                                } else {
                                    return Result::Err(JvmError::InvalidObjectReference) //Shouldn't happen
                                }

                                Option::None
                            }
                            "Main|get_time" => {
                                let epoch = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as u64;

                                let ops_out: Vec<Operand> =
                                    vec![Operand(OperandType::Long, epoch as usize)];

                                Option::Some(ops_out)
                            }
                            "Main|print_long" => {
                                println!("{:?}", frame.op_stack);
                                let long = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?.1;

                                println!("Long: {}", long);

                                Option::None
                            }
                            "Main|get_int" => Option::Some(vec![Operand(OperandType::Int, 1)]),
                            _ => unimplemented!("Unimplemented native method \"{}\"", name),
                        };

                        match operands_out {
                            None => {}
                            Some(operands) => {
                                for x in operands.into_iter() {
                                    frame.op_stack.push(x);
                                }
                            }
                        };
                    } else {
                        let mut new_frame = RuntimeThread::<Stdout>::create_frame(
                            class.get_method(&method.name, &method.descriptor)?,
                            class.clone(),
                        );

                        new_frame.local_vars = (0..md.parameters.len())
                            .map(|i| {
                                Result::Ok((
                                    i as u16,
                                    Operand::as_type(
                                        frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?,
                                    ),
                                ))
                            })
                            .collect::<Result<LocalVariableMap, JvmError>>()?;

                        drop(frame_write);

                        self.frame_stack.push(RwLock::new(new_frame));
                    }
                });
            }
            //new
            Bytecode::New(index) => {
                let classpath = frame
                    .class
                    .constant_pool
                    .resolve_class_info(*index)
                    .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;

                let class_load_report = lazy_class_resolve!(self.classloader, classpath);

                init_static!(frame, frame_write, class_load_report, self.frame_stack, Stdout, {
                    //TODO: there's probably some type checking and rules that need to be followed here

                    let id = self.heap.create_object(class_load_report.class.clone())?;

                    frame
                        .op_stack
                        .push(Operand(OperandType::ClassReference, id));
                });
            }
            Bytecode::Newarray(atype) => {
                let length = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;

                let iat: InternArrayType = match atype {
                    4 => InternArrayType::Int,
                    5 => InternArrayType::Char,
                    6 => InternArrayType::Float,
                    7 => InternArrayType::Double,
                    8 => InternArrayType::Int,
                    9 => InternArrayType::Int,
                    10 => InternArrayType::Int,
                    11 => InternArrayType::Long,
                    _ => unreachable!("Array atype must be between 4 & 11!"),
                };

                let ptr = self.heap.allocate_array(iat, length.1.try_into().unwrap());
                frame
                    .op_stack
                    .push(Operand(OperandType::ArrayReference, ptr as usize));
            },
            Bytecode::Checkcast(index) => {
                let class_name = frame.class.constant_pool
                    .resolve_class_info(*index)
                    .ok_or(JvmError::ClassError(ClassError::ConstantNotFound))?;

                let class_load_report = lazy_class_resolve!(self.classloader, class_name);

                init_static!(frame, frame_write, class_load_report, self.frame_stack, Stdout, {
                    let object_ref = frame.op_stack
                        .pop()
                        .ok_or(JvmError::EmptyOperandStack)?;

                    if matches!(object_ref.0, OperandType::NullReference) {
                        return Result::Ok(()); //Operand stack is unchanged if the object reference is null
                    }

                    match object_ref.0 {
                        OperandType::InterfaceReference | OperandType::ClassReference => {
                            let object = self.heap.objects.get(object_ref.1).unwrap();

                            let t_class = class_load_report.class;
                            let s_class = object.class.clone();

                            let can_cast = self.classloader.read().check_cast(s_class, t_class);

                            if can_cast {
                                frame.op_stack.push(object_ref);
                            } else {
                                unimplemented!();
                            }
                        },
                        OperandType::ArrayReference => {
                            unimplemented!()
                        },
                        _ => unreachable!()
                    }

                });
            },
            Bytecode::Arraylength => {
                let array_ref = frame.op_stack.pop().ok_or(JvmError::EmptyOperandStack)?;

                if matches!(array_ref.0, OperandType::NullReference) {
                    unimplemented!("NullPointerException");
                }

                //The type of the array doesn't actually matter here as we just need the length,
                //which is located in the ArrayHeader which is independent of the type.
                let (header, _) = unsafe { Heap::get_array::<usize>(array_ref.1 as *mut u8) };
                let length = unsafe { (*header).length };

                frame.op_stack.push(Operand(OperandType::Int, length as usize));
            },
            _ => {
                unimplemented!(
                    "\n\nOpcode: {:?}\nClass: {}\nMethod: {}\nIndex: {}\n\n",
                    opcode,
                    frame.class.this_class,
                    frame.method_name,
                    frame.pc
                );
            }
        }

        Result::Ok(())
    }

    pub fn get_stack_count(&self) -> usize {
        self.frame_stack.len()
    }

    pub fn get_frames(&self) -> &Vec<RwLock<Frame>> {
        &self.frame_stack
    }
}