extern crate jvm;

use jvm::vm::linker::loader::load_class;
use std::fs;

fn main() {
    // let bytes: Vec<u8> = vec![
    //     0xCA, 0xFE, 0xBA, 0xBE, //Magic u32
    //     0x00, 0x00, //Major u16
    //     0x00, 0x33 //Minor u16
    // ];

    let bytes = fs::read("C:/Users/Birb/rust-test/Main.class");

    if !bytes.is_err() {
        let stream = bytes.unwrap();
        let result = load_class(&stream);
    } else {
        println!("{}", bytes.unwrap_err());
    }
}
