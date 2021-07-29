mod control_graph;
mod ir;

use jvm::vm::class::{Class, FieldDescriptor, JavaType, Method};

use byteorder::{ByteOrder, LittleEndian};
use std::collections::HashMap;

use jvm::vm::linker::loader::ClassLoader;

use walrus::{FunctionId, ImportId, LocalId, MemoryId, Module, ModuleLocals, ValType};

use std::io;

use std::cell::RefCell;

fn format_method_name(class: &Class, method: &Method) -> String {
    format!(
        "{}!{}!{}",
        class.this_class, &method.name, &method.descriptor
    )
}

fn base_type_as_value_type(bt: &JavaType) -> ValType {
    match bt {
        JavaType::Byte => ValType::I32,
        JavaType::Char => ValType::I32,
        JavaType::Double => ValType::F64,
        JavaType::Float => ValType::F32,
        JavaType::Int => ValType::I32,
        JavaType::Long => ValType::I64,
        JavaType::Reference => ValType::I64,
        JavaType::Bool => ValType::I32,
        JavaType::Short => ValType::I32,
    }
}

fn field_descriptor_as_value_type(fd: &FieldDescriptor) -> ValType {
    match fd {
        FieldDescriptor::JavaType(bt) => base_type_as_value_type(bt),
        FieldDescriptor::ObjectType(_) => ValType::I32,
        FieldDescriptor::ArrayType(_) => ValType::I32,
    }
}

pub enum ResourceType {
    U32(u32),
    I32(i32),
    UTF8(String),
    ObjectInstance {
        classpath_offset: u32,
        fields: Vec<u8>,
        fields_length: i32,
    }, //Manually created data in the memory layout, like a pre-allocated class
    Null,
}

impl ResourceType {
    pub fn size_of(&self) -> u32 {
        match &self {
            ResourceType::U32(_) => 4,
            ResourceType::I32(_) => 4,
            ResourceType::UTF8(str) => (str.len() * 2) as u32 + 4,
            ResourceType::ObjectInstance {
                classpath_offset: _,
                fields_length,
                ..
            } => 4 + *fields_length as u32,
            ResourceType::Null => 1,
        }
    }

    pub fn as_vec(&self) -> Vec<u8> {
        match self {
            ResourceType::U32(u) => {
                let mut buf = [0u8; 4];
                LittleEndian::write_i32(&mut buf[..], *u as i32);
                buf.into()
            }
            ResourceType::I32(i) => {
                let mut buf = [0u8; 4];
                LittleEndian::write_i32(&mut buf[..], *i);
                buf.into()
            }
            ResourceType::UTF8(string) => {
                let mut vec = vec![0, 0, 0, 0];
                LittleEndian::write_u32(&mut vec[..], string.len() as u32);

                for char in string.as_bytes() {
                    vec.push(*char);
                    vec.push(0);
                }

                vec
            }
            ResourceType::ObjectInstance {
                classpath_offset,
                fields,
                fields_length,
            } => {
                let mut vec = vec![0, 0, 0, 0];
                LittleEndian::write_i32(&mut vec[..], *classpath_offset as i32);
                vec.extend_from_slice(&fields[..*fields_length as usize]);

                vec
            }
            ResourceType::Null => vec![0u8],
        }
    }
}

pub struct Resource {
    pub res_type: ResourceType,
    pub offset: u32,
}

/*

Layout of allocated classes in memory

+---------------+----------------------------+------------------------------------+
| <header>: u32 | <[u32]>: this-class fields | [u32]: recursive superclass fields |
+---------------+----------------------------+------------------------------------+

header: an offset pointer which points to the class info, located somewhere in the linear memory
`
 */

struct MemoryLayout {
    pub resources: Vec<Resource>,
    pub strings: HashMap<String, usize>,
    pub constant_resource_map: HashMap<u16, usize>,
    pub null: u32,
    pub heap_size: u32,
    pub memory_id: MemoryId,
}

impl MemoryLayout {
    pub fn new(memory_id: MemoryId) -> Self {
        Self {
            resources: vec![
                Resource {
                    res_type: ResourceType::U32(1 + 16 + 4),
                    offset: 0,
                }, //Heap size, including the heap size u32
                Resource {
                    res_type: ResourceType::Null,
                    offset: 4,
                }, //Only needs to be 1 byte long
                Resource {
                    res_type: ResourceType::UTF8(String::from("java/lang/String")),
                    offset: 5,
                },
            ],
            strings: HashMap::new(),
            constant_resource_map: HashMap::new(),
            null: 1,
            heap_size: 0,
            memory_id,
        }
    }

    pub fn append_resource(&mut self, resource_type: ResourceType) -> usize {
        let last = self.resources.last().unwrap();
        let offset = last.offset + last.res_type.size_of();

        let resource_size = resource_type.size_of();

        self.resources.push(Resource {
            res_type: resource_type,
            offset,
        });

        let resource: &mut Resource = self.resources.get_mut(self.heap_size as usize).unwrap();

        if let ResourceType::U32(size) = &mut resource.res_type {
            *size += resource_size;
        }

        self.resources.len() - 1
    }

    pub fn allocate_string(&mut self, string: String, string_class: &Class) -> usize {
        let _size = string_class.full_heap_size + 4; //Following the memory layout

        let resource_index = self.append_resource(ResourceType::UTF8(string));
        let utf8 = self.resources.get(resource_index).unwrap().offset;

        let mut fields = vec![0, 0, 0, 0];

        LittleEndian::write_i32(&mut fields[..], utf8 as i32);

        self.append_resource(ResourceType::ObjectInstance {
            classpath_offset: 5,
            fields,
            fields_length: string_class.full_heap_size as i32,
        })
    }

    pub fn as_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.resources.last().unwrap().offset as usize);
        let mut last_index = 0;

        for res in self.resources.iter() {
            out.resize(out.len() + (last_index..res.offset).len(), 0u8);

            out.append(&mut res.res_type.as_vec());
            last_index = res.offset + res.res_type.size_of();
        }

        out
    }
}

struct StackHelper<'locals> {
    vars: HashMap<ValType, Vec<LocalId>>,
    types: Vec<ValType>,
    locals: &'locals mut ModuleLocals,
}

impl<'locals> StackHelper<'locals> {
    pub fn new(module: &'locals mut ModuleLocals) -> Self {
        Self {
            vars: HashMap::new(),
            types: Vec::new(),
            locals: module,
        }
    }

    pub fn get(&mut self, t: ValType, offset: usize) -> LocalId {
        self.vars.entry(t).or_insert_with(Vec::new);

        let vec = self.vars.get_mut(&t).unwrap();

        if offset + 1 > vec.len() {
            let diff = offset + 1 - vec.len();

            for _ in 0..diff {
                vec.push(self.locals.add(t));

                self.types.push(t)
            }
        }

        *vec.get(offset).unwrap()
    }
}

#[derive(Debug)]
pub enum MethodFunctionType {
    Normal(FunctionId, Vec<LocalId>),
    NativeImport(FunctionId, ImportId),
}

impl MethodFunctionType {
    fn unwrap_normal(&self) -> (FunctionId, &Vec<LocalId>) {
        match self {
            MethodFunctionType::Normal(func, vec) => (*func, vec),
            MethodFunctionType::NativeImport(_, _) => {
                panic!("Tried to unwrap normal function, was an import!")
            }
        }
    }
}

pub struct SerializedMemory {
    pub data: Vec<u8>,
    pub strings: HashMap<String, usize>,
}

macro_rules! xload_n {
    ($n:expr, $md:ident, $variables:ident) => {
        $variables
            .get(&$n)
            .ok_or(CompilationError::UnknownLocal($n))?
            .clone()
    };
}

pub struct WasmEmitter<'classloader> {
    pub(crate) module: RefCell<Module>,
    pub(crate) memory: RefCell<MemoryLayout>,
    pub(crate) method_function_map: HashMap<String, MethodFunctionType>,
    pub(crate) class_loader: &'classloader ClassLoader,

    pub class_resource_map: HashMap<String, u32>, //String being the classpath, and the u32 being a location in the resources
    pub main_export: String,
}

#[derive(Debug)]
pub enum CompilationError {
    UnknownLocal(usize),
    InvalidBytecode,
    EOF,
    NoneError,
    MethodNotFound,
    InvalidIntermediate,
}

impl From<io::Error> for CompilationError {
    fn from(_: io::Error) -> Self {
        Self::EOF
    }
}

pub enum Intermediate1 {}
