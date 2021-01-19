use crate::vm::class::{Class, FieldDescriptor, MethodDescriptor, AccessFlags, RefInfoType, Method, MethodReturnType, ArrayType};
use std::collections::HashMap;
use crate::vm::linker::loader::{load_class, ClassLoader, are_classpaths_siblings, ClassLoadState};
use std::{fs, mem};
use std::path::{PathBuf};
use std::ops::{Deref};
use crate::vm::class::attribute::{AttributeItem};
use crate::vm::class::BaseType;
use std::rc::Rc;
use std::cell::{RefCell, RefMut};
use std::io::{Cursor};
use byteorder::{ReadBytesExt, BigEndian};
use crate::vm::class::constant::Constant;
use std::alloc::{Layout, dealloc};
use std::alloc::alloc;
use std::mem::size_of;
use core::ptr;
use std::time::{SystemTime, UNIX_EPOCH};
use std::iter::FromIterator;
use crate::vm::heap::{Heap, InternArrayType, Type, Reference, ObjectInfo};
use std::sync::Mutex;

static mut BENCHMARKS: u16 = 0;

pub struct VirtualMachine {
    pub start_time: SystemTime,

    pub threads: HashMap<String, RuntimeThread>,

    pub class_loader: Rc<RefCell<ClassLoader>>,

    pub heap: Rc<RefCell<Heap>>,
}

impl VirtualMachine {
    pub fn new(classpath_root: PathBuf) -> VirtualMachine {
        VirtualMachine {
            threads: HashMap::new(),
            start_time: SystemTime::now(),

            class_loader: Rc::new(RefCell::new(ClassLoader::new(classpath_root))),
            heap: Rc::new(RefCell::new(Heap::new()))
        }
    }

    pub fn spawn_thread(&mut self, name: String, method_name: &str, method_descriptor: &str, class: Rc<Class>, args: Vec<String>) -> &RuntimeThread {

        let mut thread = RuntimeThread::new(String::from(&name), self.heap.clone(), self.class_loader.clone());

        let mut frame = RuntimeThread::create_frame(
            class.get_method(method_name, method_descriptor),
            class.clone()
        );

        let mut index: u16 = 0;

        let string_arr_ptr = Heap::allocate_array(InternArrayType::ClassReference, args.len());

        let (header, body) = unsafe { Heap::get_array::<usize>(string_arr_ptr as *mut u8) };

        frame.local_vars.insert(0, vec![ Type::Reference(Reference::Array(header as *mut u8)) ]);

        for arg in args.iter() {
            let str = self.heap.borrow_mut().create_string(arg, self.class_loader.borrow_mut().load_and_link_class("java/lang/String").ok().unwrap().1);

            unsafe {
                *body.offset(size_of::<usize>() as isize * index as isize) = str as usize;
            }

            index += 1;
        }

        thread.add_frame(
            frame
        );

        self.threads.insert(String::from(&name), thread);

        self.threads.get(&name).unwrap()
    }
}

#[derive(Debug)]
#[derive(PartialEq)]
pub enum OperandType {
    Char,
    Int,
    Float,
    Long,
    Double,

    NullReference,
    InterfaceReference,
    ClassReference,
    ArrayReference
}

impl OperandType {
    pub fn from_base_type(bt: BaseType) -> Self {
        match bt {
            BaseType::Byte => OperandType::Int,
            BaseType::Char => OperandType::Int,
            BaseType::Double => OperandType::Double,
            BaseType::Float => OperandType::Float,
            BaseType::Int => OperandType::Int,
            BaseType::Long => OperandType::Long,
            BaseType::Reference => OperandType::ArrayReference,
            BaseType::Bool => OperandType::Int,
            BaseType::Short => OperandType::Int
        }
    }
}

impl Clone for OperandType {
    fn clone(&self) -> Self {
        match self {
            OperandType::Char => OperandType::Char,
            OperandType::Int => OperandType::Int,
            OperandType::Float => OperandType::Float,
            OperandType::Long => OperandType::Long,
            OperandType::Double => OperandType::Double,
            OperandType::NullReference => OperandType::NullReference,
            OperandType::InterfaceReference => OperandType::InterfaceReference,
            OperandType::ClassReference => OperandType::ClassReference,
            OperandType::ArrayReference => OperandType::ArrayReference
        }
    }
}

#[derive(Debug)]
pub struct Operand (pub OperandType, pub usize);

impl Clone for Operand {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}

impl Operand {
    pub fn as_type(op: Operand) -> Vec<Type> {
        match op.0 {
            OperandType::Char => vec![Type::Char(op.1 as u16)],
            OperandType::Int => vec![Type::Int(op.1 as i32)],
            OperandType::Float => vec![Type::Float(op.1 as f32)],
            OperandType::Long => vec![Type::LongHalf(op.1 as u32)],
            OperandType::Double => vec![Type::DoubleHalf(op.1 as u32)],
            OperandType::NullReference => vec![Type::Reference(Reference::Null)],
            OperandType::InterfaceReference => vec![Type::Reference(Reference::Interface(op.1 as *mut u8))],
            OperandType::ClassReference => vec![Type::Reference(Reference::Class(op.1))],
            OperandType::ArrayReference => vec![Type::Reference(
                Reference::Array(op.1 as *mut u8)
            )]
        }
    }

    pub fn get_category(&self) -> u8 {
        match self.0 {
            OperandType::Char => 1,
            OperandType::Int => 1,
            OperandType::Float => 1,
            OperandType::Long => 2,
            OperandType::Double => 2,
            OperandType::NullReference => 1,
            OperandType::InterfaceReference => 1,
            OperandType::ClassReference => 1,
            OperandType::ArrayReference => 1
        }
    }
}

pub struct LocalVariableMap {
    map: HashMap<u16, Type>
}

impl LocalVariableMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new()
        }
    }

    fn insert(&mut self, key: u16, mut value: Vec<Type>) {
        if value.len() == 1 {
            self.map.insert(key, value.pop().unwrap());
        } else if value.len() == 2 {
            self.map.insert(key+1, value.pop().unwrap());
            self.map.insert(key, value.pop().unwrap());
        }
    }

    fn get(&self, key: &u16) -> Option<&Type> {
        self.map.get(&key)
    }

    fn contains_key(&self, key: u16) -> bool {
        self.map.contains_key(&key)
    }
}

pub struct Frame {
    code: Cursor<Vec<u8>>,
    local_vars: LocalVariableMap,
    method_name: String,
    op_stack: Vec<Operand>,
    class: Rc<Class>
}

pub struct RuntimeThread {
    heap: Rc<RefCell<Heap>>,
    classloader: Rc<RefCell<ClassLoader>>,

    thread_name: String,
    stack: Vec<Frame>
}

impl RuntimeThread {
    pub fn new(name: String, heap: Rc<RefCell<Heap>>, classloader: Rc<RefCell<ClassLoader>>) -> RuntimeThread {
        RuntimeThread {
            heap,
            classloader,

            thread_name: name,
            stack: Vec::new()
        }
    }

    pub fn create_frame(method: Rc<Method>, class: Rc<Class>) -> Frame {
        let attr = method.attribute_map.get("Code").expect("Method did not have a Code attribute! Is native?");

        let code_cursor= if let AttributeItem::Code(code) = &attr.info {
            Cursor::new(code.code.clone())
        } else { unreachable!("Code attribute did not resolve to Attribute::Code variant!") };

        Frame {
            class: class.clone(),
            code: code_cursor,
            method_name: method.name.clone(),
            local_vars: LocalVariableMap::new(),
            op_stack: Vec::new()
        }
    }

    pub fn add_frame(&mut self, frame: Frame) {
        self.stack.push(frame)
    }

    pub fn step(&mut self) -> Result<(), std::io::Error> {
        let mut pending_frames: Option<Vec<Frame>> = Option::None;

        let frame = self.stack.last_mut().unwrap();

        let opcode_pos = frame.code.position();
        let opcode = frame.code.read_u8()?;
        
        let mut heap = self.heap.borrow_mut();
        let classloader = self.classloader.borrow();

        match opcode {
            0x0 => (), //nop, do nothing
            0x1 => frame.op_stack.push(Operand(OperandType::NullReference, 0)), //aconst_null,
            0x2..=0x8 => frame.op_stack.push(Operand(OperandType::Int, (opcode - 0x3) as usize)),
            0x9..=0xa => { //lconst_<n>
                frame.op_stack.push(Operand(OperandType::Long, 0 ));
                frame.op_stack.push(Operand(OperandType::Long, (opcode-0x9) as usize ));
            },
            0xb..=0xd => frame.op_stack.push(Operand(OperandType::Float, (opcode-0xb) as usize)), //fconst_<n>
            0xe..=0xf => { //dconst_<n>
                frame.op_stack.push(Operand(OperandType::Double, 0 ));
                frame.op_stack.push(Operand(OperandType::Double, (opcode-0xe) as usize));
            },
            0x10 => frame.op_stack.push(Operand(OperandType::Int, frame.code.read_u8()? as usize)), //bipush
            0x11 => frame.op_stack.push(Operand(OperandType::Int, frame.code.read_u16::<BigEndian>()? as usize)), //sipush
            //ldc
            0x12 => { //ldc (load constant)
                let index = frame.code.read_u8()?;
                let constant = frame.class.constant_pool.get(index as usize).unwrap();

                match constant {
                    Constant::Integer(int) => frame.op_stack.push(Operand(OperandType::Int, *int as usize)),
                    Constant::Float(float) => frame.op_stack.push(Operand(OperandType::Float, *float as usize)),
                    Constant::String(str_index) => {
                        if let Constant::Utf8(string) = frame.class.constant_pool.get(*str_index as usize).unwrap() {
                            let allocated_string = heap.create_string(string, classloader.get_class("java/lang/String").unwrap());

                            frame.op_stack.push(Operand(OperandType::ClassReference, allocated_string as usize));
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
            0x1a..=0x1d => {
                let local_var = frame.local_vars.get(&((opcode - 0x1a) as u16)).unwrap();

                if let Type::Int(int) = local_var { //<n> = 0..3
                    frame.op_stack.push(Operand(OperandType::Int, *int as usize))
                } else { panic!("iload_n command did not resolve to an int!") }
            },
            //lload_<n> ; Load long from local variables
            0x1e..=0x21 => {
                if let Type::LongHalf(lhalf1) = frame.local_vars.get(&((opcode-0x1e) as u16)).unwrap() { //<n> = 0..3
                    if let Type::LongHalf(lhalf2) = frame.local_vars.get(&((opcode-0x1e) as u16 + 1)).unwrap() { //<n> = 0..3
                        frame.op_stack.push(Operand(OperandType::Long, *lhalf1 as usize));
                        frame.op_stack.push(Operand(OperandType::Long, *lhalf2 as usize));
                    } else { panic!("lload_n command did not resolve to a long!") }
                } else { panic!("lload_n command did not resolve to a long!") }
            },
            //fload_<n> ; Load float from local variables
            0x22..=0x25 => {
                if let Type::Float(float) = frame.local_vars.get(&((opcode-0x22) as u16)).unwrap() { //<n> = 0..3
                    frame.op_stack.push(Operand(OperandType::Float, *float as usize));
                } else { panic!("fload_n command did not resolve to an float!") }
            },
            //dload_<n> ; Load double from local variables
            0x26..=0x29 => {
                if let Type::DoubleHalf(dhalf1) = frame.local_vars.get(&((opcode-0x26) as u16)).unwrap() { //<n> = 0..3
                    if let Type::DoubleHalf(dhalf2) = frame.local_vars.get(&((opcode-0x1e) as u16 + 1)).unwrap() { //<n> = 0..3
                        frame.op_stack.push(Operand(OperandType::Double, *dhalf1 as usize));
                        frame.op_stack.push(Operand(OperandType::Double, *dhalf2 as usize));
                    } else { panic!("dload_n command did not resolve to a double!") }
                } else { panic!("dload_n command did not resolve to a double!") }
            },
            //aload_<n> ; Load reference
            0x2a..=0x2d => {
                let index = opcode-0x2a;

                let var = frame.local_vars.get(&(index as u16)).expect(
                    &format!("Class: {}\nMethod: {}\nIndex: {}", frame.class.this_class, frame.method_name, opcode_pos)
                );

                if let Type::Reference(reference) = var {
                    match reference {
                        Reference::Class(ptr) => {
                            frame.op_stack.push(Operand(OperandType::ClassReference, (*ptr) as usize))
                        },
                        Reference::Null => frame.op_stack.push(Operand(OperandType::NullReference, 0)),
                        Reference::Interface(ptr) => frame.op_stack.push(Operand(OperandType::InterfaceReference, *ptr as usize)),
                        Reference::Array(ptr) => frame.op_stack.push(Operand(OperandType::ArrayReference, *ptr as usize))
                    }
                } else { panic!("aload_<n> local variable did not resolve to a reference!") };
            },
            0x30 => { //lconst_f
                let arr_ptr = frame.op_stack.pop().unwrap().1 as *mut u8;
                let index = frame.op_stack.pop().unwrap().1 as usize;

                unsafe {
                    let float = (arr_ptr.offset((size_of::<f32>() * index) as isize)) as *mut f32;
                    frame.op_stack.push(Operand(OperandType::Float, *float as usize));
                }
            },
            0x31 => { //lconst_d
                let arr_ptr = frame.op_stack.pop().unwrap().1 as *mut u8;
                let index = frame.op_stack.pop().unwrap().1 as usize;

                unsafe {
                    let double = *((arr_ptr.offset((size_of::<u64>() * index) as isize)) as *mut u64);

                    let dhalf1 = (double >> 32) as u32 as usize;
                    let dhalf2 = double as u32 as usize;

                    frame.op_stack.push(Operand(OperandType::Double, dhalf1));
                    frame.op_stack.push(Operand(OperandType::Double, dhalf2));
                }
            },
            0x32 => { //aaload (load reference from an array)
                let index = frame.op_stack.pop().unwrap().1 as isize;
                let arr_ptr = frame.op_stack.pop().unwrap().1 as *mut u8;

                let (header, body) = unsafe { Heap::get_array::<usize>(arr_ptr) };

                let header_id = unsafe { (*header).id };

                let ref_type = match InternArrayType::from_u8(header_id) {
                    InternArrayType::ArrayReference => OperandType::ArrayReference,
                    InternArrayType::ClassReference => OperandType::ClassReference,
                    InternArrayType::InterfaceReference => OperandType::InterfaceReference,
                    _ => panic!("Reference in array was not a reference.")
                };

                let element = unsafe {
                    body.offset(size_of::<usize>() as isize * index)
                };

                frame.op_stack.push(Operand(ref_type, element as usize));
            },
            //caload
            0x34 => {
                let index = frame.op_stack.pop().unwrap();
                let arrayref = frame.op_stack.pop().unwrap();

                let (_, ptr) = Heap::get_array::<u16>(arrayref.1 as *mut u8);

                let val = unsafe { *ptr.offset(index.1 as isize) };
                frame.op_stack.push(Operand(OperandType::Int, val as usize));
            },
            //istore_<n>
            0x3b..=0x3e => {
                let index = (opcode - 0x3b) as u16;
                let value = frame.op_stack.pop().unwrap().1 as i32;
                frame.local_vars.insert(index, vec![Type::Int(value)]);
            },
            //lstore_<n>
            0x3f..=0x42 => {
                let index = (opcode - 0x3f) as u16;
                let long = frame.op_stack.pop().unwrap().1;
                frame.local_vars.insert(index, vec![
                    Type::LongHalf((long >> 32) as u32),
                    Type::LongHalf((long & 0x7fffffff) as u32)
                ]);
            },
            //astore_<n>
            0x4b..=0x4e => {
                let index = opcode-0x4b;

                let object_ref = frame.op_stack.pop().unwrap();

                frame.local_vars.insert(index as u16, Operand::as_type(object_ref));
            },
            //castore
            0x55 => {
                let val = frame.op_stack.pop().unwrap();
                let index = frame.op_stack.pop().unwrap();
                let arrayref = frame.op_stack.pop().unwrap();

                let (_, ptr) = Heap::get_array::<u16>(arrayref.1 as *mut u8);

                unsafe {
                    let offset = ptr.offset(index.1 as isize);
                    *offset = val.1 as u16;
                }
            },
            0x57 => {
                frame.op_stack.pop();
            },
            0x58 => {
                let first = frame.op_stack.pop().unwrap();
                if first.get_category() == 1 {
                    frame.op_stack.pop();
                }
            },
            //dup
            0x59 => {
                let op = frame.op_stack.pop().unwrap();
                frame.op_stack.push(op.clone());
                frame.op_stack.push(op);
            }
            0x60 => {
                let int1 = frame.op_stack.pop().unwrap().1 as i32;
                let int2 = frame.op_stack.pop().unwrap().1 as i32;

                frame.op_stack.push(Operand(OperandType::Int,(int1 + int2) as usize));
            },
            0x61 => {
                // let a = frame.op_stack.pop().unwrap();
                // let b = frame.op_stack.pop().unwrap();
                

            },
            0x84 => { //inc
                let index = frame.code.read_u8()?;
                let byte = frame.code.read_u8()?;

                let var = frame.local_vars.get(&(index as u16)).unwrap();
                if let Type::Int(int) = var {
                    let num = *int + byte as i32;
                    frame.local_vars.insert(byte as u16, vec![Type::Int(num)]);
                } else { panic!("Local variable used in iinc was not an int!") }
            },
            0x99..=0x9e => {
                let offset = frame.code.read_u16::<BigEndian>()?- 2;
                let val = frame.op_stack.pop().unwrap().1 as i32;

                if match opcode {
                    0x99 => val == 0, //ifeq
                    0x9a => val != 0, //ifne
                    0x9b => val < 0, //iflt
                    0x9c => val >= 0, //iflte
                    0x9d => val > 0, //ifgt
                    0x9e => val >= 0, //ifge
                    _ => unreachable!()
                } {
                    frame.code.set_position(frame.code.position() + offset as u64);
                }
            }
            0x9f..=0xa4 => {
                let offset = frame.code.read_u16::<BigEndian>()? - 3; //Subtract the two bytes read and the opcode
                //Subtract two because the offset will be used relative to the opcode, not the last byte.

                let i2 = frame.op_stack.pop().unwrap().1 as u32;
                let i1 = frame.op_stack.pop().unwrap().1 as u32;

                let branch = match opcode {
                    0x9f => i1 == i2, //if_icmpeq
                    0xa0 => i1 != i2, //if_icmpne
                    0xa1 => i1 < i2, //if_icmplt
                    0xa2 => i1 >= i2, //if_icmpge
                    0xa3 => i1 > i2, //if_icmpgt
                    0xa4 => i1 <= i2, //if_icmple
                    _ => unreachable!()
                };
                if branch {
                    frame.code.set_position(frame.code.position() + offset as u64);
                }
            },
            0xa7 => { //Goto
                let offset = ( frame.code.read_i16::<BigEndian>()?) as i64;
                let pos = (frame.code.position() as i64) + offset;
                frame.code.set_position(( pos as u64 ) - 3);
            },
            //ireturn
            0xac => {
                let op = frame.op_stack.pop().unwrap();
                self.stack.pop();
                let invoker = self.stack.last_mut().unwrap();
                invoker.op_stack.push(op);
            },
            //areturn
            0xb0 => {
                let op = frame.op_stack.pop().unwrap();
                self.stack.pop();
                let invoker = self.stack.last_mut().unwrap();
                invoker.op_stack.push(op);
            },
            0xb1 => { //return void
                self.stack.pop();
            },
            //getfield
            0xb4 => {
                //TODO: type checking, exceptions

                let index = frame.code.read_u16::<BigEndian>()?;

                let fieldref = frame.class.constant_pool.resolve_ref_info(index as usize).unwrap();

                let class = classloader.get_class(&fieldref.class_name).unwrap();

                let object_ref = frame.op_stack.pop().unwrap().1;

                let value = self.heap.borrow().get_field::<usize>(object_ref, class, &fieldref.name);
                let fd = FieldDescriptor::parse(&fieldref.descriptor);

                let operand = match fd {
                    FieldDescriptor::BaseType(base_type) => {
                        Operand(OperandType::from_base_type(base_type), unsafe { *value } )
                    },
                    FieldDescriptor::ObjectType(object_type) => {
                        Operand(OperandType::ClassReference, unsafe { *value })
                    },
                    FieldDescriptor::ArrayType(_) => panic!("Not allowed.")
                };

                frame.op_stack.push(operand);
            },
            0xb5 => { //putfield
                //TODO: type checking, exceptions
                let index = frame.code.read_u16::<BigEndian>()?;

                let fieldref = frame.class.constant_pool.resolve_ref_info(index as usize).unwrap();

                let class = classloader.get_class(&fieldref.class_name);

                let value = frame.op_stack.pop().unwrap();
                let object_ref = frame.op_stack.pop().unwrap().1;

                heap.put_field(object_ref, class.unwrap(), &fieldref.name, value.1);
            },
            //invokevirtual
            0xb6 => {
                let index = frame.code.read_u16::<BigEndian>()?;

                let method_ref = frame.class.constant_pool.resolve_ref_info(index as usize).unwrap();

                let method_descriptor = MethodDescriptor::parse(&method_ref.descriptor);

                let param_len = method_descriptor.parameters.len();

                let mut parameter_stack = Vec::with_capacity(param_len);

                println!("Parameter count: {}", &param_len);

                for i in 0..param_len {
                    let param = frame.op_stack.pop().unwrap();
                    let descriptor = method_descriptor.parameters.get(i).unwrap();

                    if !descriptor.matches_operand(param.0.clone()) {
                        panic!("Operand did not match parameter requirements.");
                    }

                    parameter_stack.insert(
                        param_len-i-1,
                        param
                    );
                }

                let object_ref = frame.op_stack.pop().unwrap();
                let object_info = heap.objects.get(object_ref.1).unwrap();

                if method_ref.info_type != RefInfoType::MethodRef { panic!("Invokevirtual must reference a MethodRef!"); }
                if method_ref.name == "<init>" || method_ref.name == "<clinit>" { panic!("Invokevirtual must not invoke an initialization method!"); }

                let class = classloader.get_class(&method_ref.class_name).unwrap();
                let method = class.get_method(&method_ref.name, &method_ref.descriptor);

                let method_descriptor = MethodDescriptor::parse(&method.descriptor);

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
                let polymorphic: bool = {
                    if class.this_class == "java/lang/invoke/MethodHandle" {

                        if method_descriptor.parameters.len() == 1 {
                            if let FieldDescriptor::ArrayType(at) = method_descriptor.parameters.first().unwrap() {
                                if at.dimensions == 1 {
                                    if let FieldDescriptor::ObjectType(ot) = &*at.field_descriptor {
                                        if ot == "java/lang/Object" {
                                            if let MethodReturnType::FieldDescriptor(return_fd) = method_descriptor.return_type {
                                                if let FieldDescriptor::ObjectType(return_ot) = return_fd {
                                                    if return_ot == "java/lang/Object" {
                                                        if method.access_flags & 0x180 == 0x180 { //NATIVE | VARARGS == 0x180
                                                            true
                                                        } else { false }
                                                    } else { false }
                                                } else { false }
                                            } else { false }
                                        } else { false }
                                    } else { false }
                                } else { false }
                            } else { false }
                        } else { false }
                    } else { false }
                };

                if !polymorphic {

                    let (resolved_class, method_to_invoke) = classloader.recurse_resolve_overridding_method(
                        object_info.class.clone(),
                        &method.name,
                        &method.descriptor
                    ).expect("Could not resolve method!");

                    let mut new_frame = RuntimeThread::create_frame(
                        method_to_invoke,
                        resolved_class
                    );

                    new_frame.local_vars.insert(0, vec![Type::Reference(Reference::Class(
                        object_ref.1
                    ))]);

                    let mut index = 1;

                    for item in parameter_stack {
                        new_frame.local_vars.insert(index, Operand::as_type(item));
                        index += 1;
                    }

                    pending_frames = Option::Some(
                        vec![new_frame]
                    );
                }

            },
            //invokespecial https://docs.oracle.com/javase/specs/jvms/se7/html/jvms-6.html#jvms-6.5.invokespecial
            0xb7 => {
                let index = frame.code.read_u16::<BigEndian>()?;

                let method_ref = frame.class.constant_pool.resolve_ref_info(index as usize).unwrap();

                let method_descriptor = MethodDescriptor::parse(&method_ref.descriptor);

                // println!("{}", method_d);

                let method_class = classloader.get_class(&method_ref.class_name).unwrap();
                let resolved_method = method_class.get_method(&method_ref.name, &method_ref.descriptor);
                let to_invoke: (Rc<Class>, Rc<Method>);

                if {
                    (method_class.access_flags & 0x20 == 0x20)
                    && classloader.recurse_is_superclass(frame.class.as_ref(), &method_class.this_class)
                    && resolved_method.name != "<init>"
                } {
                    to_invoke = classloader.recurse_resolve_supermethod_special(
                       classloader.get_class(&frame.class.super_class).unwrap(),
                       &resolved_method.name,
                       &resolved_method.descriptor,
                    ).unwrap();
                } else {
                    to_invoke = (method_class, resolved_method);
                }

                let mut new_frame = RuntimeThread::create_frame(
                    to_invoke.1,
                    to_invoke.0
                );

                let param_len = method_descriptor.parameters.len();

                for i in 0..param_len {
                    let param = frame.op_stack.pop().unwrap();
                    let descriptor = method_descriptor.parameters.get(i).unwrap();

                    if !descriptor.matches_operand(param.0.clone()) {
                        panic!("Operand did not match parameter requirements.");
                    }

                    new_frame.local_vars.insert(
                        (param_len-i) as u16,
                        Operand::as_type(param)
                    );
                }

                new_frame.local_vars.insert(0, Operand::as_type(
                    frame.op_stack.pop().unwrap()
                )); //the objectref

                pending_frames = Option::Some(vec![
                    new_frame
                ]);

                // if AccessFlags::is_protected(method.access_flags) {
                //     let is_superclass = jvm.recurse_is_superclass(&frame.class, &method_ref.class_name);
                //     let are_siblings = VirtualMachine::are_sibling_classes(&frame.class.this_class, &method_ref.class_name);
                //
                //     if is_superclass && !are_siblings {
                //         if frame.class.this_class != object_info.class && !jvm.recurse_is_superclass(&jvm.get_class(&object_info.class), &frame.class.this_class) {
                //             panic!("Uhh what");
                //         }
                //     }
                // }
            },
            0xb8 => { //invokestatic
                let index = frame.code.read_u16::<BigEndian>()?;

                let method_ref = frame.class.constant_pool.resolve_ref_info(index as usize).unwrap();

                let class = classloader.get_class(method_ref.class_name.as_str()).unwrap(); //Load the class if it isn't already loaded

                let md = MethodDescriptor::parse(method_ref.descriptor.as_str());
                let method = class.get_method(method_ref.name.as_str(), method_ref.descriptor.as_str());

                if AccessFlags::is_native(method.access_flags) { //Is native
                    let mut argument_stack: Vec<Operand> = Vec::new();
                    for _ in md.parameters.iter() {
                        argument_stack.push({
                            let operand = frame.op_stack.pop();
                            if operand.is_some() {
                                operand.unwrap()
                            } else {
                                panic!(
                                    format!("Attempted to pop operand stack, is empty. Bytecode {}\n Method name \"{}\"", opcode_pos, method.name)
                                )
                            }
                        });
                    }

                    let name = &*format!("{}|{}", class.this_class, method.name);

                    let operands_out = match name {
                        "Main|print_int" => {
                            println!("Main_print_int({})", argument_stack.pop().unwrap().1);

                            Option::None
                        },
                        "Main|print_string" => {
                            let string_reference = argument_stack.pop().unwrap().1;

                            let chars_ptr = heap.get_field::<usize>(string_reference, self.classloader.borrow().get_class("java/lang/String").unwrap(), "chars");
                            let mut string_bytes: Vec<u8> = Vec::new();

                            unsafe {
                                let (header, body) = Heap::get_array::<u8>(*chars_ptr as *mut u8);
                                let length = (*header).size;

                                println!("Array length {}", length);

                                for i in 0..length {
                                    let char = *(body.offset(i as isize));
                                    string_bytes.push(char);
                                }

                                let str = String::from_utf8(string_bytes).unwrap();
                                println!("{}", str);

                                Option::None
                            }
                        },
                        "Main|get_time" => {
                            let epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;

                            let ops_out: Vec<Operand> = vec![
                                Operand(OperandType::Long, epoch as usize),
                            ];

                            Option::Some(ops_out)
                        },
                        "Main|print_long" => {
                            println!("{:?}", frame.op_stack);
                            let long = frame.op_stack.pop().unwrap().1;

                            println!("Long: {}", long);

                            Option::None
                        },
                        "Main|get_int" => {
                            Option::Some(vec![
                                Operand(OperandType::Int, 1)
                            ])
                        },
                        _ => unimplemented!("Unimplemented native method \"{}\"", name)
                    };

                    if operands_out.is_some() {
                        let operands = operands_out.unwrap();

                        println!("{:?}", operands);

                        for x in operands.iter() {
                            frame.op_stack.push(x.clone());
                        }
                    }
                } else if !*class.static_been_seen.read().unwrap() {
                    let mut new_frame = RuntimeThread::create_frame(
                        class.get_method("<init>", "()V"),
                        class.clone()
                    );

                    for i in 0..md.parameters.len() {
                        new_frame.local_vars.insert(
                            i as u16,
                            Operand::as_type(frame.op_stack.pop().unwrap())
                        );
                    }

                    pending_frames = Option::Some(vec![ //First is at top of stack
                        RuntimeThread::create_frame(
                            class.get_method("<clinit>", "()V"),
                            class.clone()
                        ),
                        new_frame
                    ]);
                } else {
                    pending_frames = Option::Some(vec![
                        {
                            let mut new_frame = RuntimeThread::create_frame(
                                class.get_method(&method.name, &method.descriptor),
                                class.clone()
                            );

                            for i in 0..md.parameters.len() {
                                new_frame.local_vars.insert(
                                    i as u16,
                                    Operand::as_type(frame.op_stack.pop().unwrap())
                                );
                            }

                            new_frame
                        }
                    ]);
                }
            },
            //new
            0xbb => {
                let index = frame.code.read_u16::<BigEndian>()?;
                let classpath = frame.class.constant_pool.resolve_class_info(index).unwrap();
                let class = classloader.get_class(classpath).unwrap();

                //TODO: there's probably some type checking and rules that need to be followed here

                let id = heap.create_object(class.clone());

                //Because of how pending_frames adds frames to the stack, you put it in the reverse order, so the top would end up being at the end of the vec.

                if !*class.static_been_seen.read().unwrap() && class.field_map.contains_key("<clinit>") { //Just loaded, we need to run <clinit>
                    {
                        let mut seen = class.static_been_seen.write().unwrap();
                        *seen = true;
                    }

                    pending_frames = Option::Some(vec![
                        RuntimeThread::create_frame(
                            class.get_method("<clinit>", "()V"),
                            class.clone()
                        )
                    ]);
                }

                frame.op_stack.push(Operand(OperandType::ClassReference, id));
            },
            0xbc => {
                let atype = frame.code.read_u8()?;

                let length = frame.op_stack.pop().unwrap();

                let iat: InternArrayType = match atype {
                    4 => InternArrayType::Int,
                    5 => InternArrayType::Char,
                    6 => InternArrayType::Float,
                    7 => InternArrayType::Double,
                    8 => InternArrayType::Int,
                    9 => InternArrayType::Int,
                    10 => InternArrayType::Int,
                    11 => InternArrayType::Long,
                    _ => unreachable!("Array atype must be between 4 & 11!")
                };

                let ptr = Heap::allocate_array(iat, length.1);
                frame.op_stack.push(Operand(OperandType::ArrayReference, ptr as usize));
            },
            //Breakpoint
            0xca => {
                // println!("Breakpoint!");
            }
            _ => {
                unimplemented!("\n\nOpcode: {}\nClass: {}\nMethod: {}\nIndex: {}\n\n", opcode, frame.class.this_class, frame.method_name, frame.code.position()-1);
            }
        }

        if pending_frames.is_some() {
            let mut frames = pending_frames.unwrap();
            for _ in 0..frames.len() {
                self.stack.push(frames.pop().unwrap());
            }
        }

        Result::Ok(())
    }

    pub fn get_stack_count(&self) -> usize {
        self.stack.len()
    }

}

pub mod bytecode {
    #[allow(non_camel_case_types)]
    pub enum Instruction {
        aaload = 0x32,
        aastore = 0x53,
        aconst_null = 0x1,
        aload = 0x19,
        aload_0 = 0x2a,
        aload_1 = 0x2b,
        aload_2 = 0x2c,
        aload_3 = 0x2d,
        anewarray = 0xbd,
        areturn = 0xb0,
        arraylength = 0xbe,
        astore = 0x3a,
        astore_0 = 0x4b,
        astore_1 = 0x4c,
        astore_2 = 0x4d,
        astore_3 = 0x4e,
        athrow = 0xbf
    }
}