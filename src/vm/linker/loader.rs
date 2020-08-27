use std::io::Cursor;
use byteorder::{ReadBytesExt, BigEndian};
use crate::vm::class::constant::{Constant, PoolTag};
use std::any::Any;
use crate::vm::class::{ClassFile, FieldInfo};
use std::ops::Deref;

pub fn load_class(bytes: &Vec<u8>) -> Result<ClassFile, &str> {
    let mut rdr = Cursor::new(bytes);

    let magic = rdr.read_u32::<BigEndian>().unwrap();

    if magic != 0xCAFEBABE {
        return Err("Magic number does not match .class specification.");
    }

    let major = rdr.read_u16::<BigEndian>().unwrap();
    let minor = rdr.read_u16::<BigEndian>().unwrap();

    if major > 51 {
        return Err(".class version is above specification.");
    }

    let constant_pool_count = rdr.read_u16::<BigEndian>().unwrap(); //The constant pool count is equal to the number of constants + 1

    let mut constant_pool: Vec<Constant> = Vec::new();

    println!("Constant pool count {}", constant_pool_count-1);

    for i in 1..constant_pool_count {
        if i == constant_pool_count { break; } //An index is valid if it is greater than 0 and less than the count
        let constant = Constant::from_bytes(&mut rdr);

        println!("{:?}", &constant.tag);

        constant_pool.push(constant);
    }

    let access_flags     = rdr.read_u16::<BigEndian>().unwrap();
    let this_class       = rdr.read_u16::<BigEndian>().unwrap();
    let super_class      = rdr.read_u16::<BigEndian>().unwrap();
    let interfaces_count = rdr.read_u16::<BigEndian>().unwrap();

    println!("interfaces count {}", interfaces_count);

    let mut interfaces = Vec::new();

    for i in 0..interfaces_count {
        let constant_index = rdr.read_u16::<BigEndian>().unwrap();
        let constant: &Constant = constant_pool.get(constant_index as usize).unwrap();

        if constant.tag != PoolTag::Class {
            panic!("bruh moment");
        } else {
            interfaces.push(constant_index);
        }
    }

    let field_count = rdr.read_u16::<BigEndian>().unwrap();
    let fields: Vec<FieldInfo> = Vec::new();

    for i in 0..field_count {

    }

    Ok(ClassFile {
        magic,
        minor_version: minor,
        major_version: major,
        constant_pool_count,
        constant_pool,
        access_flags,
        this_class,
        super_class,
        interfaces_count,
        interfaces,
        field_count: 0,
        fields: vec![],
        method_count: 0,
        methods: vec![],
        attribute_count: 0,
        attributes: vec![]
    })

    // ClassFile {
    //     magic: 0,
    //     minor_version: 0,
    //     major_version: 0,
    //     constant_pool_count: 0,
    //     constant_pool_info: vec![],
    //     access_flags: 0,
    //     this_class: 0,
    //     super_class: 0,
    //     interfaces_count: 0,
    //     interfaces: vec![],
    //     field_count: 0,
    //     fields: vec![],
    //     method_count: 0,
    //     methods: vec![],
    //     attribute_count: 0,
    //     attributes: vec![]
    // }
}