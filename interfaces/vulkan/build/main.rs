use std::env;
use std::fs::File;
use std::io::{Cursor, Write};
use std::path::Path;

const VK_XML: &[u8] = include_bytes!("../vk.xml");

mod parse;

fn main() {
    let registry = parse::parse(Cursor::new(VK_XML));

    let mut out = {
        let dest_path = Path::new(&env::var("OUT_DIR").unwrap()).join("vk.rs");
        File::create(&dest_path).unwrap()
    };

    for (name, typedef) in &registry.type_defs {
        write_type_def(out.by_ref(), name, typedef);
    }

    write_commands_enum(out.by_ref(), &registry);
    write_commands_wrappers(out.by_ref(), &registry);
    write_get_instance_proc_addr(out.by_ref(), &registry);
}

fn write_type_def(mut out: impl Write, name: &str, type_def: &parse::VkTypeDef) {
    match type_def {
        parse::VkTypeDef::Enum | parse::VkTypeDef::Bitmask | parse::VkTypeDef::Handle => {
            writeln!(out, "type {} = u32;", name).unwrap();
        },
        parse::VkTypeDef::Struct { fields } => {
            //writeln!(out, "#[derive(Encode, Decode)]").unwrap();
            writeln!(out, "struct {} {{", name).unwrap();
            for (field_ty, field_name) in fields {
                writeln!(out, "    r#{}: {},", field_name, print_ty(&field_ty)).unwrap();
            }
            writeln!(out, "}}").unwrap();
        },
    }
}

fn write_commands_enum(mut out: impl Write, registry: &parse::VkRegistry) {
    out.write_all(br#"#[allow(non_camel_case_types)]"#).unwrap();
    out.write_all(br#"pub enum VulkanMessage {"#).unwrap();

    for command in &registry.commands {
        write!(out, "{},", command.name).unwrap();
    }

    out.write_all(br#"}"#).unwrap();
}

fn write_commands_wrappers(mut out: impl Write, registry: &parse::VkRegistry) {
    for command in &registry.commands {
        let params = command.params
            .iter()
            .map(|(ty, n)| format!("r#{}: {}, ", n, print_ty(ty)))
            .collect::<String>();

        writeln!(out, "extern \"system\" fn wrapper_{}({}) -> {} {{", command.name, params, print_ty(&command.ret_ty)).unwrap();
        /*write!(out, "   let msg = VulkanMessage::{};", command.name).unwrap();
        write!(out, "   syscalls::block_on(syscalls::emit_message_with_response(INTERFACE, msg));").unwrap();*/
        writeln!(out, "    panic!()").unwrap();
        writeln!(out, "}}").unwrap();
    }
}

fn write_get_instance_proc_addr(mut out: impl Write, registry: &parse::VkRegistry) {
    writeln!(out, "#[allow(non_snake_case)]").unwrap();
    writeln!(out, "pub unsafe extern \"C\" fn vkGetInstanceProcAddr(_instance: usize, name: *const u8) -> PFN_vkVoidFunction {{").unwrap();
    writeln!(out, "    let name = match CStr::from_ptr(name as *const _).to_str() {{").unwrap();
    writeln!(out, "        Ok(n) => n,").unwrap();
    writeln!(out, "        Err(_) => return mem::transmute(ptr::null::<c_void>())").unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(out, "").unwrap();
    writeln!(out, "    match name {{").unwrap();
    for command in &registry.commands {
        let params_tys = command.params
            .iter()
            .map(|(ty, _)| format!("{}, ", print_ty(ty)))
            .collect::<String>();

        writeln!(out, "        \"{}\" => {{", command.name).unwrap();
        writeln!(out, "            mem::transmute::<_, PFN_vkVoidFunction>(wrapper_{} as extern \"system\" fn({}) -> {})", command.name, params_tys, print_ty(&command.ret_ty)).unwrap();
        writeln!(out, "        }}").unwrap();
    }
    writeln!(out, "        _ => mem::transmute(ptr::null::<c_void>())").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
}

fn print_ty(ty: &parse::VkType) -> String {
    match ty {
        parse::VkType::Ident(ident) if ident == "void" => "()".to_string(),
        parse::VkType::Ident(ident) if ident == "int" => "i32".to_string(),
        parse::VkType::Ident(ident) if ident == "int32_t" => "i32".to_string(),
        parse::VkType::Ident(ident) if ident == "uint8_t" => "u8".to_string(),
        parse::VkType::Ident(ident) if ident == "uint16_t" => "u16".to_string(),
        parse::VkType::Ident(ident) if ident == "uint32_t" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "uint64_t" => "u64".to_string(),
        parse::VkType::Ident(ident) if ident == "size_t" => "usize".to_string(),
        parse::VkType::Ident(ident) if ident == "float" => "f32".to_string(),

        parse::VkType::Ident(ident) if ident == "VkSampleMask" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "VkBool32" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "VkDeviceAddress" => "u64".to_string(),
        parse::VkType::Ident(ident) if ident == "VkDeviceSize" => "u64".to_string(),

        parse::VkType::Ident(ident) if ident == "ANativeWindow" => "c_void".to_string(),
        parse::VkType::Ident(ident) if ident == "AHardwareBuffer" => "c_void".to_string(),
        parse::VkType::Ident(ident) if ident == "CAMetalLayer" => "c_void".to_string(),
        parse::VkType::Ident(ident) if ident == "wl_display" => "c_void".to_string(),
        parse::VkType::Ident(ident) if ident == "wl_surface" => "c_void".to_string(),
        parse::VkType::Ident(ident) if ident == "Display" => "c_void".to_string(),

        parse::VkType::Ident(ty) => ty.to_string(),

        parse::VkType::Array(t, arr) => format!("[{}; {}]", print_ty(t), arr),
        parse::VkType::MutPointer(t) if **t == parse::VkType::Ident("void".into()) => "*mut c_void".to_owned(),
        parse::VkType::ConstPointer(t) if **t == parse::VkType::Ident("void".into()) => "*const c_void".to_owned(),
        parse::VkType::MutPointer(t) => format!("*mut {}", print_ty(t)),
        parse::VkType::ConstPointer(t) => format!("*const {}", print_ty(t)),
    }
}
