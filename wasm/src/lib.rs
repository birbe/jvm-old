use parity_wasm::builder;
use parity_wasm::builder::{ModuleBuilder, SignatureBuilder, FunctionBuilder, FuncBodyBuilder, signature, FunctionDefinition, ImportBuilder, SignaturesBuilder};
use jvm;
use jvm::vm::class::{Class, Method, MethodDescriptor, FieldDescriptor, BaseType, MethodReturnType};
use std::rc::Rc;
use parity_wasm::elements::{ValueType, Instruction, Instructions, FuncBody, External, Local};
use crypto::sha2::Sha256;
use crypto::digest::Digest;
use jvm::vm::class::attribute::AttributeItem::Signature;
use std::error::Error;
use jvm::vm::class::attribute::{AttributeItem, Code};
use std::io::Cursor;
use jvm::vm::class::constant::Constant;
use std::collections::HashMap;
use byteorder::{ReadBytesExt, BigEndian};
use jvm::vm::vm::VirtualMachine;
use jvm::vm::linker::loader::ClassLoader;
use parity_wasm::elements::Error::InvalidSectionId;
use core::num::flt2dec::Sign;

fn format_method_name(class: &Class, method: &Method) -> String {
    format!("{}!{}!{}", class.this_class, &method.name, &method.descriptor)
}

fn base_type_as_value_type(bt: &BaseType) -> ValueType {
    match bt {
        BaseType::Byte => ValueType::I32,
        BaseType::Char => ValueType::I32,
        BaseType::Double => ValueType::F64,
        BaseType::Float => ValueType::F32,
        BaseType::Int => ValueType::I32,
        BaseType::Long => ValueType::I64,
        BaseType::Reference => ValueType::I64,
        BaseType::Bool => ValueType::I32,
        BaseType::Short => ValueType::I32
    }
}

fn field_descriptor_as_value_type(fd: &FieldDescriptor) -> ValueType {
    match fd {
        FieldDescriptor::BaseType(bt) => base_type_as_value_type(bt),
        FieldDescriptor::ObjectType(_) => ValueType::I32,
        FieldDescriptor::ArrayType(_) => ValueType::I32
    }
}

pub enum ResourceType {
    U32(u32),
    I32(i32),
    String(String),
    Class {
        classpath_offset: u32,
        fields: Vec<i32>,
        fields_length: i32
    }, //Manually created data in the memory layout, like a pre-allocated class
    Null
}

impl ResourceType {
    pub fn size_of(&self) -> u32 {
        match &self {
            ResourceType::U32(_) => 1,
            ResourceType::I32(_) => 1,
            ResourceType::String(str) => str.len() as u32 + 1,
            ResourceType::Class { classpath_offset, fields_length, .. } => 1 + *fields_length as u32,
            ResourceType::Null => 1
        }
    }
}

impl Into<Vec<i32>> for ResourceType {

    fn into(self) -> Vec<i32> {
        match self {
            ResourceType::U32(u) => vec![ u as i32 ],
            ResourceType::I32(i) => vec![ i ],
            ResourceType::String(string) => {
                let mut out = Vec::with_capacity(string.len() + 1);

                out.push(string.len() as i32);

                for byte in string.into_bytes().iter() {
                    out.push(*byte as i32);
                }

                out
            }
            ResourceType::Class { classpath_offset, fields, fields_length } => {
                let mut out = Vec::new();
                out.push(classpath_offset as i32);
                out.extend_from_slice(&fields[..fields_length as usize]);
                out
            }
            ResourceType::Null => vec![ 0i32 ]
        }
    }

}

pub struct Resource {
    pub res_type: ResourceType,
    pub offset: u32
}

/*

Layout of allocated classes in memory

+---------------+----------------------------+------------------------------------+
| <header>: u32 | <[u32]>: this-class fields | [u32]: recursive superclass fields |
+---------------+----------------------------+------------------------------------+

header: an offset pointer which points to the class info, located somewhere in the linear memory
`
 */

pub struct MemoryLayout {
    pub resources: Vec<Resource>,
    pub strings: HashMap<String, usize>,
    pub constant_resource_map: HashMap<u16, usize>,
    pub null: u32,
    pub heap_size_offset: u32
}

impl MemoryLayout {
    pub fn new() -> Self {
        Self {
            resources: vec![
                Resource { res_type: ResourceType::Null, offset: 0 },
                Resource { res_type: ResourceType::U32(0), offset: 1 },
                Resource { res_type: ResourceType::String(String::from("java/lang/String")), offset: 2 }
            ],
            strings: HashMap::new(),
            constant_resource_map: HashMap::new(),
            null: 0,
            heap_size_offset: 1
        }
    }

    pub fn append_resource(&mut self, resource_type: ResourceType) -> usize {
        let last = self.resources.last().unwrap();
        let offset = last.offset + last.res_type.size_of();

        self.resources.push(Resource {
            res_type: resource_type,
            offset
        });

        self.resources.len() - 1
    }

    pub fn allocate_string(&mut self, string: String, string_class: &Class) -> usize {
        let size = string_class.full_heap_size + 1; //Following the memory layout

        let utf8 = self.append_resource(ResourceType::String(string));

        self.append_resource(ResourceType::Class {
            classpath_offset: 2,
            fields: vec![ utf8 as i32 ],
            fields_length: string_class.full_heap_size as i32
        })
    }
}

enum QueuedFunction {
    Normal(FunctionDefinition),
    Import(SignatureBuilder)
}

pub struct WasmEmitter<'classloader> {
    pub module_builder: ModuleBuilder,
    pub memory: MemoryLayout,
    pub function_count: u32,
    pub method_function_map: HashMap<String, u32>,
    pub class_loader: &'classloader ClassLoader,
    
    function_queue: Vec<QueuedFunction>,

    pub class_resource_map: HashMap<String, u32> //String being the classpath, and the u32 being a location in the resources
}

impl<'classloader> WasmEmitter<'classloader> {
    pub fn new(class_loader: &'classloader ClassLoader) -> Self {
        let mut emitter = WasmEmitter {
            module_builder: builder::module(),
            memory: MemoryLayout::new(),
            function_count: 0,
            method_function_map: HashMap::new(),
            class_loader,
            function_queue: Vec::new(),
            class_resource_map: HashMap::new()
        };

        emitter.function_queue.push(QueuedFunction::Normal(
            FunctionBuilder::new().with_signature(
                SignatureBuilder::new().with_param(ValueType::I32).with_result(ValueType::I32).build_sig()
            ).with_body(
                FuncBody::new(vec![], Instructions::new(vec![
                    Instruction::I32Const(emitter.memory.heap_size_offset as i32),
                    Instruction::I32Load(0, 0), //Load the heap size
                    Instruction::GetLocal(0), //Requested allocate size
                    Instruction::I32Add, //new size
                    Instruction::SetLocal(1), //store the new size

                    Instruction::I32Const(emitter.memory.heap_size_offset as i32), //Offset of the heap size
                    Instruction::GetLocal(1), //Get the new size
                    Instruction::I32Store(0, 0), //Store it

                    Instruction::GetLocal(1), //Get it again
                    Instruction::I32Const(emitter.memory.resources.len() as i32), //Start of the heap
                    Instruction::I32Add, //Add them together
                    Instruction::Return
                ]))
            ).build()
        ));

        emitter.function_queue.push(QueuedFunction::Normal(
            FunctionBuilder::new().with_signature(
                SignatureBuilder::new().with_param(ValueType::I32).with_result(ValueType::I32).build_sig()
            ).with_body(FuncBody::new(vec![], Instructions::new(vec![
                Instruction::GetLocal(0), //Get the offset to the class size
                Instruction::I32Load(0, 0), //Get the size of the class

                Instruction::I32Const(1),
                Instruction::I32Add, //Increment the size by one so we can remember which class this is

                Instruction::Call(0), //Call allocate_heap which returns the offset to the allocated memory
                Instruction::Return, //Return the offset
            ]))).build()
        ));

        emitter.method_function_map.insert(String::from("allocate_heap"), 0);
        emitter.method_function_map.insert(String::from("allocate_class"), 1);

        emitter
    }

    pub fn process_classes(&mut self) {
        for (classpath, class) in self.class_loader.class_map.iter() {
            let offset = self.memory.append_resource(ResourceType::String(classpath.clone()));

            let clazz = class.unwrap();

            println!("Class {} fields length {}", classpath, clazz.full_heap_size);

            self.memory.append_resource(ResourceType::Class {
                classpath_offset: 0,
                fields: Vec::new(),
                fields_length: clazz.full_heap_size as i32
            });

            self.process_class(clazz.as_ref());
        }
    }

    pub fn process_class(&mut self, class: &Class) {
        for (method_name, overloaded) in class.method_map.iter() {
            for (descriptor, method) in overloaded.iter() {
                self.process_method(class, method.as_ref());
            }
        }
    }

    pub fn process_method(&mut self, class: &Class, method: &Method) -> Option<()> {
        let method_descriptor = MethodDescriptor::parse(&method.descriptor);

        let params: Vec<ValueType> = method_descriptor.parameters.iter().map(|f| field_descriptor_as_value_type(f)).collect();

        let mut signature_builder = SignatureBuilder::new().with_params(params);

        if let MethodReturnType::FieldDescriptor(fd) = &method_descriptor.return_type {
            signature_builder = signature_builder.with_result(field_descriptor_as_value_type(fd))
        }

        let signature = signature_builder.build_sig();

        if let AttributeItem::Code(code) = &method.attribute_map.get("Code")?.info {
            let (instructions, locals) = self.bytecode_to_instructions(code, class);

            self.function_queue.push(QueuedFunction::Normal(
                FunctionBuilder::new().with_signature(signature).with_body(
                    FuncBody::new(
                        locals.iter().map(|x| Local::new(1, x.clone())).collect(), instructions
                    )
                )
            ))

            println!("Method {}\nInstructions: {:?}", method.name, instructions);
        } else {
            //Native method (but we should probably check the flags as well but eh)

            //Native methods are treated as an export function

            let params: Vec<ValueType> = method_descriptor.parameters.iter().map(|f| field_descriptor_as_value_type(f)).collect();
            let mut sig = SignatureBuilder::new().with_params(params);

            match &method_descriptor.return_type {
                MethodReturnType::Void => {}
                MethodReturnType::FieldDescriptor(fd) => { sig = sig.with_result(field_descriptor_as_value_type(fd)); }
            }

            function = function.with_signature(sig.build_sig());

            let id = self.function_count;

            self.module_builder.push_function(function.build());

            self.module_builder = self.module_builder.with_import(ImportBuilder::new().with_external(
                External::Function(id)
            ).build());

            // function.with_signature(SignatureBuilder::new().with)
        }

        Option::Some(())
    }

    fn bytecode_to_instructions(&mut self, code: &Code, class: &Class) -> (Instructions, Vec<ValueType>) {
        let mut bytes = Cursor::new(code.code.clone());
        let len = code.code.len();

        let mut instructions = Vec::new();

        let locals = Vec::new();
        let mut jvm_locals = Vec::new();

        //I'm calling them jugglers because they're local variables which are used to re-arrange the
        //stack so that WASM instructions will be able to use what's on the stack in the right order

        let i32_jugglers = 0;
        let i64_jugglers = 0;
        let f32_jugglers = 0;
        let f64_jugglers = 0;

        while bytes.position() < len as u64 {
            let opcode = bytes.read_u8().unwrap();

            instructions.extend(match opcode {
                0x1 => vec![ //aconst_null
                             Instruction::I32Const(self.memory.null as i32)
                ],
                0x2..=0x5 => vec![ //iconst_<n-1>
                    Instruction::I32Const(opcode as i32 - 0x3)
                ],
                0x10 => vec![
                    Instruction::I32Const(bytes.read_u8().unwrap() as i32)
                ],
                0x12 => { //ldc
                    let index = bytes.read_u8().unwrap();
                    vec![
                        match class.constant_pool.get(index as usize).unwrap() {
                            Constant::Integer(int) => Instruction::I32Const(*int),
                            Constant::Float(float) => Instruction::F32Const(*float as u32),
                            Constant::Long(long) => Instruction::I64Const(*long),
                            Constant::Double(double) => Instruction::F64Const(*double as u64),
                            Constant::String(string) => {
                                let utf8 = class.constant_pool.resolve_utf8(*string).unwrap();

                                let offset = if !self.memory.strings.contains_key(utf8) {
                                    self.memory.allocate_string(String::from(utf8), self.class_loader.get_class("java/lang/String").unwrap().as_ref())
                                } else {
                                    *self.memory.strings.get(utf8).unwrap()
                                };

                                Instruction::I32Const(offset as i32)
                            }
                            _ => Instruction::Nop
                        }
                    ]
                },
                0x15 => { //iload
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValueType::I32);

                    vec![
                        Instruction::GetLocal(bytes.read_u8().unwrap() as u32)
                    ]
                },
                0x16 => { //lload
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValueType::I64);

                    vec![
                        Instruction::GetLocal(bytes.read_u8().unwrap() as u32)
                    ]
                },
                0x17 => { //fload
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValueType::F32);

                    vec![
                        Instruction::GetLocal(bytes.read_u8().unwrap() as u32)
                    ]
                },
                0x18 => { //dload
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValueType::F64);

                    vec![
                          Instruction::GetLocal(bytes.read_u8().unwrap() as u32)
                    ]
                },
                0x19 => { //aload
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValueType::I32);

                    vec![
                        Instruction::GetLocal(bytes.read_u8().unwrap() as u32)
                    ]
                },
                0x1a..=0x1d => { //iload_<n>
                    let index = (opcode as u32) - 0x1a;

                    jvm_locals.insert(index as usize, ValueType::I32);

                    vec![
                        Instruction::GetLocal(index)
                    ]
                },
                0x1e..=0x21 => { //iload_<n>
                    let index = (opcode as u32) - 0x1e;

                    jvm_locals.insert(index as usize, ValueType::I64);

                    vec![
                        Instruction::GetLocal(index)
                    ]
                },
                0x22..=0x25 => { //fload_<n>
                    let index = (opcode as u32) - 0x22;

                    jvm_locals.insert(index as usize, ValueType::F32);

                    vec![
                        Instruction::GetLocal(index)
                    ]
                },
                0x26..=0x29 => { //dload_<n>
                    let index = (opcode as u32) - 0x26;

                    jvm_locals.insert(index as usize, ValueType::F64);

                    vec![
                          Instruction::GetLocal(index)
                    ]
                },
                0x2a..=0x2d => { //aload_<n>
                    let index = (opcode as u32) - 0x2a;

                    jvm_locals.insert(index as usize, ValueType::I32);

                    vec![
                        Instruction::GetLocal(index)
                    ]
                },
                0x32 => vec![ //aaload
                    Instruction::I32Add,
                    Instruction::I32Load(0, 0)
                ],
                0x33 => vec![ //baload
                    Instruction::I32Const(1),
                    Instruction::I32Add, //Increment by one

                    //Arrayref is now at the top of the stack
                    Instruction::I32Add,
                    //The proper offset to the element is now at the top
                    Instruction::I32Load(0, 0) //Load the byte/bool
                ],
                0x3a => { //astore
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValueType::I32);

                    vec![
                        Instruction::SetLocal(index)
                    ]
                },
                0x3b..=0x3e => vec![ //istore_<n>
                    Instruction::SetLocal(opcode as u32 - 0x3b)
                ],
                0x4b..=0x4e => vec![
                    Instruction::SetLocal(opcode as u32 - 0x4b)
                ],
                0x53 => vec![ //aastore
                    Instruction::SetLocal(0),
                    Instruction::I32Add,
                    Instruction::GetLocal(0),
                    Instruction::I32Store(0, 0)
                ],
                0x54 => vec![ //bastore
                              Instruction::SetLocal(0), //Stash the value
                              Instruction::SetLocal(1), //Stash the index

                              //arrayref is on top

                              //TODO: null check

                              Instruction::I32Const(1), //Increment by one because the length is first
                              Instruction::I32Add, //index + 1
                              Instruction::I32Add, //(index + 1) + arrayref

                              Instruction::GetLocal(0), //Unshelve the value
                              Instruction::I32Store(0, 0)
                ],
                0xac => vec![ //ireturn
                              Instruction::Return
                ],
                0xb0 => vec![ //areturn
                              Instruction::Return
                ],
                0xb1 => { //return (void)
                    vec![]
                },
                0xb5 => { //putfield
                    let index = bytes.read_u16::<BigEndian>().unwrap();

                    if let Constant::FieldRef(class_index, nat_index) = class.constant_pool.get(index as usize).unwrap() {
                        let clazz = class.constant_pool.resolve_class_info(*class_index).unwrap();
                        let (name, descriptor) = class.constant_pool.resolve_name_and_type(*nat_index).unwrap();

                        let field_offset = class.field_map.get(name).unwrap().offset;
                        let fd = FieldDescriptor::parse(descriptor);

                        vec![
                            Instruction::SetLocal(0), //Stash the value

                            Instruction::I32Const(field_offset as i32),
                            Instruction::I32Const(1),

                            Instruction::I32Add,
                            Instruction::I32Add, //pointer + 1 + field offset

                            Instruction::GetLocal(0), //Get the value
                            { //What kind of value we're storing
                                match fd {
                                    FieldDescriptor::BaseType(bt) => {
                                        match bt {
                                            BaseType::Byte => Instruction::I32Store(0, 0),
                                            BaseType::Char => Instruction::I32Store(0, 0),
                                            BaseType::Double => Instruction::F64Store(0, 0),
                                            BaseType::Float => Instruction::F32Store(0, 0),
                                            BaseType::Int => Instruction::I32Store(0, 0),
                                            BaseType::Long => Instruction::I64Store(0, 0),
                                            BaseType::Reference => Instruction::I32Store(0, 0),
                                            BaseType::Bool => Instruction::I32Store(0, 0),
                                            BaseType::Short => Instruction::I32Store(0, 0)
                                        }
                                    }
                                    FieldDescriptor::ObjectType(_) => Instruction::I32Store(0, 0),
                                    FieldDescriptor::ArrayType(_) => Instruction::I32Store(0, 0)
                                }
                            }
                        ]
                    } else { panic!("Constant did not resolve to FieldRef!"); }
                },
                0xb7 => { //TODO: invokespecial
                    let index = bytes.read_u16::<BigEndian>().unwrap();

                    vec![
                        Instruction::Nop
                    ]
                },
                0xb8 => { //TODO: invokestatic
                    let index = bytes.read_u16::<BigEndian>().unwrap();

                    vec![
                        Instruction::Nop
                    ]
                },
                0xbd => vec![ //anewarray
                    Instruction::I32Const(1), //Size of the array
                    Instruction::I32Add,
                    Instruction::Call(0)
                ],
                0xbe => vec![ //arraylength
                    Instruction::I32Load(0, 0)
                ],
                _ => unimplemented!("Unimplemented opcode {}", opcode)
                // _ => vec![
                //     Instruction::Nop
                // ]
            });
        }

        (Instructions::new(instructions), locals)
    }
}