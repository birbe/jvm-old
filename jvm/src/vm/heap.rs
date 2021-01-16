use std::rc::Rc;
use crate::vm::class::{Class, Method};
use std::alloc::{Layout, dealloc};
use core::mem;
use crate::vm::vm::{Operand, OperandType};
use std::ptr;
use std::collections::HashMap;
use std::mem::size_of;

use std::alloc::alloc;
use crate::vm::linker::loader::are_classpaths_siblings;

pub struct Heap {
    pub strings: HashMap<String, usize>,

    pub objects: Vec<ObjectInfo>,
    available_object_ids: Vec<usize>
}

impl Heap {
    pub fn new() -> Self {
        Self {
            strings: HashMap::new(),
            objects: Vec::new(),
            available_object_ids: Vec::new()
        }
    }

    pub fn create_string(&mut self, string: &str, str_class: Rc<Class>) -> usize {
        if self.strings.contains_key(string) {
            return *self.strings.get(string).unwrap();
        }

        let id = self.create_object(str_class.clone());
        let chars_ptr = Self::allocate_chars(string);

        self.put_field::<usize>(
            id,
            str_class.clone(),
            "chars",
            chars_ptr as usize
        );

        self.strings.insert(String::from(string), id);

        id
    }

    pub fn allocate_class(class: Rc<Class>) -> *mut u8 {
        unsafe {
            //Make a layout with the exact size of the object
            let layout = Layout::from_size_align(class.full_heap_size, 2).unwrap();
            //Allocate
            alloc(layout)
        }
    }

    pub fn deallocate_class(class: Rc<Class>, ptr: *mut u8) {
        unsafe {
            let layout = Layout::from_size_align(class.full_heap_size, 2).unwrap();

            dealloc(ptr, layout);
        }
    }

    pub fn create_object(&mut self, class: Rc<Class>) -> usize {
        let info = ObjectInfo {
            ptr: Self::allocate_class(class.clone()),
            class: class.clone()
        };

        if self.available_object_ids.is_empty() {
            self.objects.push(
                info
            );
        } else {
            self.objects.insert(
                self.available_object_ids.pop().unwrap(),
                info
            );
        }

        self.objects.len()-1
    }

    pub fn destroy_object(&mut self, id: usize) {
        let info = self.objects.remove(id);

        Self::deallocate_class(info.class.clone(), info.ptr);

        if id < self.objects.len() { //There's an empty space somewhere in the Vec that can be used
            self.available_object_ids.push(id);
        }
    }

    pub fn put_field<T>(&self, id: usize, class: Rc<Class>, field: &str, value: T) {
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

    pub fn get_field<T>(&self, id: usize, class: Rc<Class>, field: &str) -> *mut T {
        unsafe {
            let ptr = self.objects.get(id).unwrap().ptr;

            let offset_ptr = ptr.offset(
                class.field_map.get(
                    field
                ).unwrap().offset as isize
            ) as *mut T;

            offset_ptr
        }
    }

    pub fn allocate_array(intern_type: InternArrayType, length: usize) -> *mut ArrayHeader {
        let id = InternArrayType::convert_to_u8(intern_type);

        let header = Layout::new::<ArrayHeader>();
        let body = Layout::array::<u8>(length).unwrap();

        let (layout, offset) = header.extend(body).unwrap();

        assert_eq!(offset, mem::size_of::<ArrayHeader>());
        assert!(length < u16::MAX as usize);

        unsafe {
            let ptr = alloc(layout);

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

    pub fn allocate_chars(string: &str) -> *mut ArrayHeader {
        unsafe {
            let header = Self::allocate_array(InternArrayType::Char, string.len());

            let (arr_header, arr_body) = Self::get_array::<u8>(header as *mut u8);

            ptr::copy(string.as_bytes().as_ptr(), arr_body, string.as_bytes().len());

            arr_header
        }
    }

}

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

pub struct ObjectInfo {
    pub(crate) class: Rc<Class>,
    ptr: *mut u8
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