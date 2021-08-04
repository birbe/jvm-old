use std::alloc::{Layout, dealloc};
use std::collections::HashMap;
use std::convert::TryInto;
use std::mem::{align_of, size_of};
use std::ptr;

use sharded_slab::Slab;
use std::alloc::alloc;
use std::fmt::Debug;
use std::sync::Arc;
use crate::class::{Class, FieldDescriptor, JavaType};
use crate::{JvmError, Operand, OperandType};
use parking_lot::RwLock;

pub struct Heap {
    pub strings: RwLock<HashMap<String, usize>>,
    pub objects: Slab<Object>,
    pub arrays: Slab<*mut ArrayHeader>,

    //TODO: this is not okay!
    pub static_field_map: RwLock<HashMap<(String, String), *mut u8>>, //TODO: &(String, String) for lookups sucks
}

// pub struct RawHeap {
//     pub size: usize,
//     pub ptr: *mut u8,
//     pub used: AtomicUsize,
//     pub layout: Layout,
// }

impl Heap {
    pub fn new(size: usize) -> Self {
        Self {
            strings: RwLock::new(HashMap::new()),
            objects: Slab::new(),
            arrays: Slab::new(),
            static_field_map: RwLock::new(HashMap::new()),
        }
    }

    ///Allocates a java/lang/String object onto the heap, allocates a char[], and inserts
    ///the char[] into the chars field of the String object, returning the object reference
    pub fn create_string(&self, string: &str, str_class: Arc<Class>) -> Result<usize, JvmError> {
        let mut strings = self.strings.write();

        if strings.contains_key(string) {
            return Result::Ok(*strings.get(string).unwrap());
        }

        let id = self.create_object(str_class.clone())?;
        let chars_ptr = self.allocate_chars(string);

        self.put_field::<usize>(id, str_class, "chars", chars_ptr as usize);

        strings.insert(String::from(string), id);

        Result::Ok(id)
    }

    pub fn allocate(&self, size: usize) -> *mut u8 {
        unsafe { alloc(Layout::from_size_align(size, std::mem::align_of::<usize>() * 2).unwrap()) }
    }

    pub fn allocate_class(&self, class: Arc<Class>) -> *mut u8 {
        self.allocate(class.full_heap_size)
    }

    pub fn deallocate_class(_class: Arc<Class>, _ptr: *mut u8) {
        //...
    }

    pub fn create_object(&self, class: Arc<Class>) -> Result<usize, JvmError> {
        let object = Object {
            ptr: self.allocate_class(class.clone()),
            class,
        };

        self.objects.insert(object).ok_or(JvmError::HeapFull)
    }

    pub fn destroy_object(&mut self, id: usize) {
        let object = self.objects.get(id).unwrap();

        Self::deallocate_class(object.class.clone(), object.ptr);

        self.objects.remove(id);
    }

    pub fn put_field<T>(&self, id: usize, class: Arc<Class>, field: &str, value: T) {
        let ptr = self.objects.get(id).unwrap().ptr;

        let field_offset = class.field_map.get(field);

        unsafe {
            let offset_ptr = ptr.offset(field_offset.unwrap().offset) as *mut T;

            *offset_ptr = value;
        }
    }

    pub fn allocate_array(&self, intern_type: InternArrayType, length: u32) -> *mut ArrayHeader {
        let id = InternArrayType::convert_to_u8(intern_type);

        let header = Layout::new::<ArrayHeader>()
            .align_to(align_of::<usize>())
            .unwrap();

        let body_size = intern_type.size_of() * length as usize;

        let body = Layout::array::<u8>(body_size)
            .unwrap()
            .align_to(size_of::<usize>() * 2)
            .unwrap();

        let (layout, offset) = header.extend(body).unwrap();

        unsafe {
            let ptr = alloc(layout);

            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }

            let header = ptr.cast::<ArrayHeader>();
            (*header).id = id;
            (*header).size = body_size.try_into().unwrap();
            (*header).body = ptr.offset(offset.try_into().unwrap());
            (*header).full_size = layout.size();

            self.arrays.insert(header);

            ptr.cast::<ArrayHeader>()
        }
    }

    /// # Safety
    /// ptr must be a *mut ArrayHeader with all the field filled out accurately.
    pub unsafe fn get_array<T>(ptr: *mut u8) -> (*mut ArrayHeader, *mut T) {
        let header_ptr = ptr.cast::<ArrayHeader>();
        // let body_ptr = header_ptr.offset(size_of::<ArrayHeader<T>>() as isize).cast::<T>();
        let body_ptr = unsafe { (*header_ptr).body.cast::<T>() };

        (header_ptr, body_ptr)
    }

    pub fn allocate_chars(&self, string: &str) -> *mut ArrayHeader {
        let char_vec: Vec<u16> = string.encode_utf16().collect();
        let header = self.allocate_array(
            InternArrayType::Char,
            char_vec.len()
                .try_into()
                .unwrap(),
        );
        unsafe {
            let (arr_header, arr_body) = Self::get_array::<u16>(header as *mut u8);
            assert_eq!(
                char_vec.len() * size_of::<u16>(),
                (*arr_header).size as usize
            );
            ptr::copy(char_vec.as_ptr(), arr_body, char_vec.len());

            arr_header
        }
    }

    pub fn put_static<T: Debug>(
        &self,
        class: &str,
        field_name: &str,
        field_descriptor: &FieldDescriptor,
        value: T,
    ) {
        let key = &(String::from(class), String::from(field_name));

        let mut static_field_map = self.static_field_map
            .write();

        unsafe {
            let ptr = if !static_field_map.contains_key(key) {
                let size = match field_descriptor {
                    FieldDescriptor::JavaType(bt) => bt.size_of(),
                    _ => size_of::<usize>(),
                };

                let ptr = self.allocate(size);
                static_field_map
                    .insert((String::from(class), String::from(field_name)), ptr);
                ptr
            } else {
                *static_field_map.get(key).unwrap()
            } as *mut T;

            *ptr = value;
        }
    }

    pub fn get_static<T: Copy>(&self, class: &str, field_name: &str) -> Option<T> {
        let static_field_map = self.static_field_map
            .read();

        unsafe {
            static_field_map
                .get(&(String::from(class), String::from(field_name)))
                .map(|&ptr| *(ptr as *mut T))
        }
    }
}

impl Drop for Heap {
    fn drop(&mut self) {

        self.arrays.unique_iter().for_each(|&array| {
            unsafe {
                let layout = Layout::from_size_align((*array).full_size, align_of::<usize>()).unwrap();
                dealloc(array as *mut u8, layout);
            }
        });

        self.objects.unique_iter().for_each(|object| {
            let size = object.class.full_heap_size;
            let layout = Layout::from_size_align(size, std::mem::align_of::<usize>() * 2).unwrap();

            unsafe { dealloc(object.ptr, layout); }
        });

    }
}

unsafe impl Send for Heap {}
unsafe impl Sync for Heap {}

#[derive(Copy, Clone)]
pub enum InternArrayType {
    Char,
    Int,
    Float,
    Long,
    Double,

    NullReference,
    InterfaceReference,
    ClassReference,
    ArrayReference,
    UnknownReference,
}

#[derive(Debug)]
#[repr(C)]
pub struct ArrayHeader {
    pub id: u8,
    pub size: u32,
    pub body: *mut u8,
    pub full_size: usize
}

impl InternArrayType {
    pub fn convert_to_u8(t: InternArrayType) -> u8 {
        match t {
            InternArrayType::Char => 0,
            InternArrayType::Int => 1,
            InternArrayType::Float => 2,
            InternArrayType::Long => 3,
            InternArrayType::Double => 4,
            InternArrayType::NullReference => 5,
            InternArrayType::InterfaceReference => 6,
            InternArrayType::ClassReference => 7,
            InternArrayType::ArrayReference => 8,
            InternArrayType::UnknownReference => 9,
        }
    }

    pub fn from_u8(t: u8) -> InternArrayType {
        match t {
            0 => InternArrayType::Char,
            1 => InternArrayType::Int,
            2 => InternArrayType::Float,
            3 => InternArrayType::Long,
            4 => InternArrayType::Double,
            5 => InternArrayType::NullReference,
            6 => InternArrayType::InterfaceReference,
            7 => InternArrayType::ClassReference,
            8 => InternArrayType::ArrayReference,
            9 => InternArrayType::UnknownReference,
            _ => panic!("Invalid array type [{}] from u8!", t),
        }
    }

    pub fn from_type(t: Type) -> InternArrayType {
        match t {
            Type::Char(_) => InternArrayType::Char,
            Type::Int(_) => InternArrayType::Int,
            Type::Float(_) => InternArrayType::Float,
            Type::LongHalf(_) => InternArrayType::Long,
            Type::DoubleHalf(_) => InternArrayType::Double,
            Type::Reference(r) => match r {
                Reference::Interface(_) => InternArrayType::InterfaceReference,
                Reference::Null => panic!("Cannot use null as an array type!"),
                Reference::Class(_) => InternArrayType::ClassReference,
                Reference::Array(_) => InternArrayType::ArrayReference,
            },
        }
    }

    pub fn size_of(&self) -> usize {
        match self {
            InternArrayType::Char => size_of::<u16>(),
            InternArrayType::Int => size_of::<i32>(),
            InternArrayType::Float => size_of::<f32>(),
            InternArrayType::Long => size_of::<i64>(),
            InternArrayType::Double => size_of::<f64>(),
            InternArrayType::NullReference => size_of::<usize>(),
            InternArrayType::InterfaceReference => size_of::<usize>(),
            InternArrayType::ClassReference => size_of::<usize>(),
            InternArrayType::ArrayReference => size_of::<usize>(),
            InternArrayType::UnknownReference => size_of::<usize>(),
        }
    }
}

pub enum Value {
    Byte(i8),
    Char(u16),
    Double(f64),
    Float(f32),
    Int(i32),
    Long(i64),
    Reference(usize),
    Bool(bool),
    Short(i16),
}

impl Value {
    pub fn as_byte(&self) -> Option<i8> {
        match &self {
            Self::Byte(v) => Option::Some(*v),
            _ => Option::None,
        }
    }

    pub fn as_char(&self) -> Option<u16> {
        match &self {
            Self::Char(v) => Option::Some(*v),
            _ => Option::None,
        }
    }

    pub fn as_double(&self) -> Option<f64> {
        match &self {
            Self::Double(v) => Option::Some(*v),
            _ => Option::None,
        }
    }

    pub fn as_float(&self) -> Option<f32> {
        match &self {
            Self::Float(v) => Option::Some(*v),
            _ => Option::None,
        }
    }

    pub fn as_int(&self) -> Option<i32> {
        match &self {
            Self::Int(v) => Option::Some(*v),
            _ => Option::None,
        }
    }

    pub fn as_long(&self) -> Option<i64> {
        match &self {
            Self::Long(v) => Option::Some(*v),
            _ => Option::None,
        }
    }

    pub fn as_reference(&self) -> Option<usize> {
        match &self {
            Self::Reference(v) => Option::Some(*v),
            _ => Option::None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match &self {
            Self::Bool(v) => Option::Some(*v),
            _ => Option::None,
        }
    }

    pub fn as_short(&self) -> Option<i16> {
        match &self {
            Self::Short(v) => Option::Some(*v),
            _ => Option::None,
        }
    }
}

///Utility class for interfacing with objects within the heap.
#[derive(Clone)]
pub struct Object {
    pub class: Arc<Class>,
    pub ptr: *mut u8,
}

impl Object {
    pub fn get_field(&self, field_name: &str) -> Option<Value> {
        let field_offset = self.class.get_field(field_name).ok()?;
        let field_ptr = unsafe { self.ptr.offset(field_offset.offset) };
        unsafe {
            Option::Some(match &field_offset.info.field_descriptor {
                FieldDescriptor::JavaType(jt) => match jt {
                    JavaType::Byte => Value::Byte(*(field_ptr as *mut i8)),
                    JavaType::Char => Value::Char(*(field_ptr as *mut u16)),
                    JavaType::Double => Value::Double(*(field_ptr as *mut f64)),
                    JavaType::Float => Value::Float(*(field_ptr as *mut f32)),
                    JavaType::Int => Value::Int(*(field_ptr as *mut i32)),
                    JavaType::Long => Value::Long(*(field_ptr as *mut i64)),
                    JavaType::Reference => Value::Reference(*(field_ptr as *mut usize)),
                    JavaType::Bool => Value::Bool(*(field_ptr as *mut bool)),
                    JavaType::Short => Value::Short(*(field_ptr as *mut i16)),
                },
                FieldDescriptor::ObjectType(_) => Value::Reference(*(field_ptr as *mut usize)),
                FieldDescriptor::ArrayType(_) => Value::Reference(*(field_ptr as *mut usize)),
            })
        }
    }

    pub fn as_jstring(&self) -> Option<JString> {
        if self.class.this_class == "java/lang/String" {
            Option::Some(JString {
                internal: self.clone(),
            })
        } else {
            Option::None
        }
    }
}

pub struct JString {
    internal: Object,
}
// TODO: Make this from
impl Into<String> for JString {
    fn into(self) -> String {
        let array_pointer = self
            .internal
            .get_field("chars")
            .unwrap()
            .as_reference()
            .unwrap() as *mut u8;

        let (header, content) = unsafe { Heap::get_array::<u16>(array_pointer) };
        let slice = unsafe {
            std::slice::from_raw_parts(content, ((*header).size as usize) / size_of::<u16>())
        };
        String::from_utf16_lossy(slice)
    }
}

#[derive(Debug)]
pub enum Type {
    Char(u16),
    Int(i32),
    Float(f32),
    LongHalf(u32),
    DoubleHalf(u32),
    Reference(Reference),
}

impl Type {
    pub fn as_operand(t: Self) -> Operand {
        match t {
            Type::Char(c) => Operand(OperandType::Char, c as usize),
            Type::Int(i) => Operand(OperandType::Int, i as usize),
            Type::Float(f) => Operand(OperandType::Float, f as usize),
            Type::LongHalf(h) => Operand(OperandType::Long, h as usize),
            Type::DoubleHalf(h) => Operand(OperandType::Double, h as usize),
            Type::Reference(r) => match r {
                Reference::Class(ptr) => Operand(OperandType::ClassReference, ptr as usize),
                Reference::Null => Operand(OperandType::NullReference, 0),
                Reference::Interface(ptr) => Operand(OperandType::InterfaceReference, ptr as usize),
                Reference::Array(ptr) => Operand(OperandType::ArrayReference, ptr as usize),
            },
        }
    }

    pub fn get_size(t: Type) -> usize {
        match t {
            Type::Char(_) => size_of::<u16>(),
            Type::Int(_) => size_of::<i32>(),
            Type::Float(_) => size_of::<f32>(),
            Type::LongHalf(_) => size_of::<u32>(),
            Type::DoubleHalf(_) => size_of::<u32>(),
            Type::Reference(_) => size_of::<usize>(), //Size of a null reference should never be checked, so this is a fair assumption.
        }
    }
}

#[derive(Debug)]
pub enum Reference {
    Null,
    Interface(*mut u8),
    Class(usize),
    Array(*mut u8),
}

unsafe impl Send for Reference {}
unsafe impl Sync for Reference {}
