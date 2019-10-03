use std::env;
use std::fs::File;
use std::io::{Cursor, Write};
use std::path::Path;

const VK_XML: &[u8] = include_bytes!("../vk.xml");

mod parse;

fn main() {
    let parse = parse::parse(Cursor::new(VK_XML));
    //panic!("{:?}", parse);

    let dest_path = Path::new(&env::var("OUT_DIR").unwrap()).join("vk.rs");
    let mut out = File::create(&dest_path).unwrap();

    out.write_all(br#"
        /*pub enum {

        }
        pub fn message() -> &'static str {
            "Hello, World!"
        }*/
    "#).unwrap();

    out.write_all(b"
        pub fn message() -> &'static str {
            \"Hello, World!\"
        }
    ").unwrap();
}
