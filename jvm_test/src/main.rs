use clap::{App, Arg};
use std::collections::HashMap;
use std::sync::{Arc, mpsc, RwLock};
use std::{thread, io};
use tui::Terminal;
use tui::layout::{Layout, Direction, Constraint};
use tui::widgets::{Block, Borders, List, ListItem, Paragraph};
use tui::backend::TermionBackend;
use termion::raw::IntoRawMode;
use tui::text::{Span, Spans};
use tui::style::{Style, Color};
use termion::input::TermRead;
use termion::event::Key;
use std::process::exit;
use std::io::{Write, stdout, Cursor};
use jvm::class::{ClassBuilder, MethodBuilder, MethodDescriptor};
use jvm::class::constant::Constant;
use jvm::bytecode::Bytecode;
use jvm::linker::loader::ClassProvider;
use jvm::{VirtualMachine, JvmError};

fn main() {
    let app = App::new("Rust JVM Test")
        .version("0.1")
        .about("Tests the JVM in either WASM-compilation mode or interpreted mode")
        .arg(
            Arg::with_name("mode")
                .long("mode")
                .short("m")
                .help("Modes: `i` (interpreted), 'serialization', and 'stepper'")
                .takes_value(true)
                .required(true),
        )
        .get_matches();

    let mode = app.value_of("mode").unwrap();

    match mode {
        "i" => {
            run_vm();
        }
        "serialization" => {
            let mut class_builder = ClassBuilder::new(String::from("Main"), Option::None);
            let mut method_builder = MethodBuilder::new(
                String::from("main"),
                MethodDescriptor::parse("(Ljava/lang/String;)V").unwrap(),
                1,
            );

            let print_string_method_utf8 =
                class_builder.add_constant(Constant::Utf8(String::from("print_string")));
            let print_string_method_type =
                class_builder.add_constant(Constant::Utf8(String::from("(Ljava/lang/String);")));
            let print_string_method_descriptor =
                class_builder.add_constant(Constant::MethodType(print_string_method_type));

            let name_and_type = class_builder.add_constant(Constant::NameAndType(
                print_string_method_utf8,
                print_string_method_descriptor,
            ));
            let class_name_utf8 = class_builder.add_constant(Constant::Utf8(String::from("Main")));
            let class = class_builder.add_constant(Constant::Class(class_name_utf8));

            let method_constant =
                class_builder.add_constant(Constant::MethodRef(class, name_and_type));

            method_builder.set_instructions(vec![
                Bytecode::Aload_n(1),
                Bytecode::Iconst_n_m1(0),
                Bytecode::Aaload,
                Bytecode::Invokestatic(method_constant),
                Bytecode::Areturn,
            ]);

            class_builder.add_method(method_builder);

            dbg!(class_builder.serialize());
        },
        "stepper" => {
            stepper();
        } ,
        _ => panic!("Unknown execution method. Must be `wasm` or `i` (interpreted)"),
    }
}

struct HashMapClassProvider {
    map: HashMap<String, Vec<u8>>,
}

impl ClassProvider for HashMapClassProvider {
    fn get_classfile(&self, classpath: &str) -> Option<&[u8]> {
        self.map.get(classpath).map(|vec| &vec[..])
    }
}

macro_rules! insert_class {
    ($provider:ident, $name:literal) => {
        $provider.map.insert(
            $name.into(),
            Vec::from(&include_bytes!(concat!("../java/", $name, ".class"))[..]),
        );
    };
}

fn create_vm<Stdout: Write + Send + Sync>(stdout: Stdout) -> Arc<RwLock<VirtualMachine<Stdout>>> {
    let mut class_provider = HashMapClassProvider {
        map: HashMap::new(),
    };

    insert_class!(class_provider, "Main");
    insert_class!(class_provider, "Test");
    insert_class!(class_provider, "java/lang/Object");
    insert_class!(class_provider, "java/lang/String");

    let vm = Arc::new(
        RwLock::new(VirtualMachine::new(Box::new(class_provider), stdout)
        )
    );

    {
        let mut vm = vm.write().unwrap();

        vm.spawn_thread(
            "thread 1",
            "Main",
            "main",
            "([Ljava/lang/String;)V",
            vec![String::from("Hello world!")],
        )
            .unwrap();

        vm.spawn_thread(
            "thread 2",
            "Main",
            "main1",
            "([Ljava/lang/String;)V",
            vec![String::from("Hello world!")],
        )
            .unwrap();
    }

    vm
}

fn run_vm() {

    let vm = create_vm(Box::new(stdout()));

    let vm1 = vm.clone();

    let handle1 = thread::spawn(move || {
        let jvm = vm1.read().unwrap();
        let mut thread = jvm.threads.get("thread 1").unwrap().write();

        while thread.get_stack_count() > 0 {
            match thread.step() {
                Ok(_) => {}
                Err(e) => panic!("The Java Virtual Machine has encountered an unrecoverable error and cannot continue.\n{:?}", e)
            }
        }
    });

    let vm2 = vm;

    let handle2 = thread::spawn(move || {
        let jvm = vm2.read().unwrap();
        let mut thread = jvm.threads.get("thread 2").unwrap().write();

        while thread.get_stack_count() > 0 {
            match thread.step() {
                Ok(_) => {}
                Err(e) => panic!("The Java Virtual Machine has encountered an unrecoverable error and cannot continue.\n{:?}", e)
            }
        }
    });

    handle1.join().unwrap();
    handle2.join().unwrap();

    // let elapsed = SystemTime::now().duration_since(start).unwrap().as_nanos();

    // println!("\n[Execution has finished.]\nAverage step time: {}ns\nSteps: {}\nTime elapsed: {}ms\nObject count: {}", elapsed/steps, steps, SystemTime::now().duration_since(start).unwrap().as_millis(), vm.heap.read().unwrap().objects.len());
}

fn stepper() {
    let jvm_stdout = Cursor::new(Vec::<u8>::new());

    let vm = create_vm(jvm_stdout);

    let stdout = io::stdout().into_raw_mode().unwrap();

    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.clear().unwrap();

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let stdin = io::stdin();
        for event in stdin.keys() {
            tx.send(event.unwrap()).unwrap();
        }
    });

    loop {

        let key = rx.recv().unwrap();

        let vm_write = vm.read().unwrap();

        let thread_rw = vm_write.get_thread("thread 1").unwrap();
        let mut thread = thread_rw.write();

        match key {
            Key::Char('\n') => {
                match thread.step() {
                    Ok(_) => {}
                    Err(err) => match err {
                        JvmError::EmptyFrameStack => exit(0),
                        _ => panic!("{:?}", err)
                    }
                }
            },
            Key::Esc => exit(0),
            _ => {}
        }

        if thread.get_stack_count() == 0 {
            exit(0);
        }

        let frame = thread.get_frames().last().unwrap().read();

        let mut index = 0;

        let instruction_items: Vec<ListItem> = frame.code.iter().map(|(bytecode, offset)| {
            let mut style = Style::default();

            if index == frame.pc {
                style = style.fg(Color::Black).bg(Color::White);
            }

            index += 1;

            let span = Span::styled(
                format!("{:?} {}", bytecode, offset), style
            );

            ListItem::new(Spans::from(span))
        }).collect();

        terminal.draw(|frame| {
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Percentage(60),
                        Constraint::Percentage(40),
                    ].as_ref()
                )
                .split(frame.size());

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(
                    [
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                        Constraint::Percentage(25)
                    ].as_ref()
                )
                .split(main_chunks[0]);

            let instructions_block = Block::default()
                .title("Instructions")
                .borders(Borders::ALL);

            let frame_stack_block = Block::default()
                .title("Frames")
                .borders(Borders::ALL);

            let instructions_list = List::new(
                instruction_items
            )
                .block(instructions_block);

            let frame_stack_list = List::new(
                thread.get_frames().iter().rev().map(|frame| {
                    let frame_read = frame.read();

                    let name = format!("{}#{}", frame_read.get_class().this_class, frame_read.get_method_name());

                    ListItem::new(Spans::from(
                        Span::styled(name, Style::default())
                    ))
                }).collect::<Vec<ListItem>>()
            ).block(frame_stack_block);

            let operand_stack_block = Block::default()
                .title("Operands")
                .borders(Borders::ALL);

            let operands_list = List::new(
                thread.get_frames().last().unwrap().read().get_operand_stack().iter().rev().map(|&op| {
                    ListItem::new(Spans::from(
                        Span::styled(format!("{:?}", op), Style::default())
                    ))
                }).collect::<Vec<ListItem>>()
            ).block(operand_stack_block);

            let stdout_block = Block::default()
                .title("Stdout")
                .borders(Borders::ALL);

            let stdout_arc = vm_write.get_stdout();
            let stdout_read = stdout_arc.read();

            let stdout_paragraph = Paragraph::new(
                Spans::from(
                    Span::raw(
                        String::from_utf8_lossy(stdout_read.get_ref())
                    )
                )
            ).block(stdout_block);

            frame.render_widget(instructions_list, chunks[0]);
            frame.render_widget(frame_stack_list, chunks[1]);
            frame.render_widget(operands_list, chunks[2]);
            frame.render_widget(stdout_paragraph, main_chunks[1]);
        }).unwrap();
    }
}