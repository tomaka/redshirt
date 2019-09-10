// Copyright(c) 2019 Pierre Krieger

#![feature(never_type)]

fn main() {
    let module = core::module::Module::from_bytes(&include_bytes!("../../modules/preloaded/target/wasm32-unknown-unknown/release/preloaded.wasm")[..]);
    let mut system = core::system::System::<!>::new()
        .with_main_program(module)
        .build();

    loop {
        let result = futures::executor::block_on(system.run());
        println!("{:?}", result);
    }
}
