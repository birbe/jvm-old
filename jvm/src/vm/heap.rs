use std::rc::Rc;
use crate::vm::class::{Class, FieldDescriptor};
use std::alloc::{Layout, dealloc};
use core::mem;
use crate::vm::vm::{Operand, OperandType, JvmError};
use std::ptr;
use std::collections::{HashMap, HashSet};
use std::mem::size_of;

use std::alloc::alloc;
use std::sync::{Arc, RwLock};
use std::fmt::Debug;
use std::sync::atomic::{AtomicUsize, Ordering};
use sharded_slab::Slab;
use std::cell::{UnsafeCell};

pub struct Heap {
    pub strings: RwLock<HashMap<String, usize>>,
    pub raw: RawHeap,
    pub objects: Slab<Object>,
    pub static_field_map: UnsafeCell<HashMap<(String, String), *mut u8>>, //TODO: &(String, String) for lookups sucks
    // available_object_ids: RwLock<Vec<usize>>
}

pub struct RawHeap {
    pub size: usize,
    pub ptr: *mut u8,
    pub used: AtomicUsize,
    pub layout: Layout
}

impl Heap {
    pub fn new(size: usize) -> Self {
        Self {
            strings: RwLock::new(HashMap::new()),
            raw: unsafe { Self::allocate_heap(size) },
            objects: Slab::new(),
            static_field_map: UnsafeCell::new(HashMap::new())
        }
    }

    pub fn create_string(&self, string: &str, str_class: Arc<Class>) -> Result<usize, JvmError> {
        let mut strings = self.strings.write()?;

        if strings.contains_key(string) {
            return Result::Ok(*strings.get(string).unwrap());
        }

        let id = self.create_object(str_class.clone())?;
        let chars_ptr = self.allocate_chars(string);

        self.put_field::<usize>(
            id,
            str_class.clone(),
            "chars",
            chars_ptr as usize
        );

        strings.insert(String::from(string), id);

        Result::Ok(id)
    }

    pub fn allocate_heap(size: usize) -> RawHeap {
        let layout = Layout::from_size_align(size, 2).unwrap();
        let ptr = unsafe { alloc(layout) };

        RawHeap {
            size,
            ptr,
            used: AtomicUsize::new(0),
            layout
        }
    }

    pub fn allocate(&self, size: usize) -> *mut u8 {
        let ptr = unsafe { self.raw.ptr.offset(self.raw.used.fetch_add(size, Ordering::Relaxed) as isize) }; //TODO: bounds check
        ptr
    }

    pub fn allocate_class(&self, class: Arc<Class>) -> *mut u8 {
        self.allocate(class.full_heap_size)
    }

    pub fn deallocate_class(class: Arc<Class>, ptr: *mut u8) {
        //...
    }

    pub fn create_object(&self, class: Arc<Class>) -> Result<usize, JvmError> {
        let object = Object {
            ptr: self.allocate_class(class.clone()),
            class: class.clone()
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

        let field_offset = class.field_map.get(
            field
        );

        unsafe {
            let offset_ptr = ptr.offset(
                field_offset.unwrap().offset as isize
            ) as *mut T;

            *offset_ptr = value;
        }
    }

    pub fn get_field<T>(&self, id: usize, class: Arc<Class>, field: &str) -> Result<*mut T, JvmError> {
        unsafe {
            let ptr = self.objects.get(id).ok_or(JvmError::InvalidObjectReference)?.ptr;

            let offset_ptr = ptr.offset(
                class.field_map.get(
                    field
                ).unwrap().offset as isize
            ) as *mut T;

            return Result::Ok(offset_ptr)
        }
    }

    pub fn allocate_array(&self, intern_type: InternArrayType, length: usize) -> *mut ArrayHeader {
        let id = InternArrayType::convert_to_u8(intern_type);

        let header = Layout::new::<ArrayHeader>();
        let body = Layout::array::<u8>(length).unwrap();

        let (layout, offset) = header.extend(body).unwrap();

        assert_eq!(offset, mem::size_of::<ArrayHeader>());
        assert!(length < u16::MAX as usize);

        unsafe {
            let ptr = self.raw.ptr.offset(self.raw.used.fetch_add(layout.size(), Ordering::Relaxed) as isize);

            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }

            let header = ptr.cast::<ArrayHeader>();
            (*header).id = id;
            (*header).size = length as u16;

            ptr.cast::<ArrayHeader>()
        }
    }

    pub fn get_array<T>(ptr: *mut u8) -> (*mut ArrayHeader, *mut T) {
        let header_ptr = ptr.cast::<ArrayHeader>();
        // let body_ptr = header_ptr.offset(size_of::<ArrayHeader<T>>() as isize).cast::<T>();
        let body_ptr = unsafe { header_ptr.offset(1).cast::<T>() };

        (
            header_ptr,
            body_ptr
        )
    }

    pub fn allocate_chars(&self, string: &str) -> *mut ArrayHeader {
        unsafe {
            let header = self.allocate_array(InternArrayType::Char, string.len());

            let (arr_header, arr_body) = Self::get_array::<u8>(header as *mut u8);

            ptr::copy(string.as_bytes().as_ptr(), arr_body, string.as_bytes().len());

            arr_header
        }
    }

    pub unsafe fn put_static<T: Debug>(&self, class: &str, field_name: &str, field_descriptor: &FieldDescriptor, value: T) {
        let key = &(String::from(class), String::from(field_name));

        let ptr = if !(*self.static_field_map.get()).contains_key(key) {
            let size = match field_descriptor {
                FieldDescriptor::BaseType(bt) => bt.size_of(),
                _ => size_of::<usize>()
            };

            let ptr = self.allocate(size);
            (*self.static_field_map.get()).insert((String::from(class), String::from(field_name)), ptr);
            ptr
        } else {
            *(*self.static_field_map.get()).get(key).unwrap()
        } as *mut T;

        unsafe {
            *ptr = value;
        }
    }

    pub unsafe fn get_static<T: Copy>(&self, class: &str, field_name: &str) -> Option<T> {
        (*self.static_field_map.get()).get(&(String::from(class), String::from(field_name))).map(|&ptr| {
            unsafe {
                *(ptr as *mut T)
            }
        })
    }
}

unsafe impl Send for Heap {}
unsafe impl Sync for Heap {}

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
    UnknownReference
}

#[repr(C)]
pub struct ArrayHeader {
    pub id: u8,
    pub size: u16
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
            InternArrayType::UnknownReference => 9
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
            _ => panic!(format!("Invalid array type [{}] from u8!", t))
        }
    }

    pub fn from_type(t: Type) -> InternArrayType {
        match t {
            Type::Char(_) => InternArrayType::Char,
            Type::Int(_) => InternArrayType::Int,
            Type::Float(_) => InternArrayType::Float,
            Type::LongHalf(_) => InternArrayType::Long,
            Type::DoubleHalf(_) => InternArrayType::Double,
            Type::Reference(r) => {
                match r {
                    Reference::Interface(_) => InternArrayType::InterfaceReference,
                    Reference::Null => panic!("Cannot use null as an array type!"),
                    Reference::Class(_) => InternArrayType::ClassReference,
                    Reference::Array(_) => InternArrayType::ArrayReference
                }
            }
        }
    }

    pub fn size_of(t: InternArrayType) -> usize {
        match t {
            InternArrayType::Char => 16,
            InternArrayType::Int => 32,
            InternArrayType::Float => 32,
            InternArrayType::Long => 64,
            InternArrayType::Double => 64,
            InternArrayType::NullReference => size_of::<usize>(),
            InternArrayType::InterfaceReference => size_of::<usize>(),
            InternArrayType::ClassReference => size_of::<usize>(),
            InternArrayType::ArrayReference => size_of::<usize>(),
            InternArrayType::UnknownReference => size_of::<usize>()
        }
    }
}

pub struct Object {
    pub class: Arc<Class>,
    pub ptr: *mut u8
}

#[derive(Debug)]
pub enum Type {
    Char(u16),
    Int(i32),
    Float(f32),
    LongHalf(u32),
    DoubleHalf(u32),
    Reference(Reference)
}

impl Type {
    pub fn as_operand(t: Self) -> Operand {
        match t {
            Type::Char(c) => Operand(OperandType::Char, c as usize),
            Type::Int(i) => Operand(OperandType::Int, i as usize),
            Type::Float(f) => Operand(OperandType::Float, f as usize),
            Type::LongHalf(h) => Operand(OperandType::Long, h as usize),
            Type::DoubleHalf(h) => Operand(OperandType::Double, h as usize),
            Type::Reference(r) => {
                match r {
                    Reference::Class(ptr) => Operand(OperandType::ClassReference, ptr as usize),
                    Reference::Null => Operand(OperandType::NullReference, 0),
                    Reference::Interface(ptr) => Operand(OperandType::InterfaceReference, ptr as usize),
                    Reference::Array(ptr) => Operand(OperandType::ArrayReference, ptr as usize)
                }
            }
        }
    }

    pub fn get_size(t: Type) -> usize {
        match t {
            Type::Char(_) => size_of::<u16>(),
            Type::Int(_) => size_of::<i32>(),
            Type::Float(_) => size_of::<f32>(),
            Type::LongHalf(_) => size_of::<u32>(),
            Type::DoubleHalf(_) => size_of::<u32>(),
            Type::Reference(_) => size_of::<usize>() //Size of a null reference should never be checked, so this is a fair assumption.
        }
    }
}

#[derive(Debug)]
pub enum Reference {
    Null,
    Interface(*mut u8),
    Class(usize),
    Array(*mut u8)
}

unsafe impl Send for Reference {}
unsafe impl Sync for Reference {}