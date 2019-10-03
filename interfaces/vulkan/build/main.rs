// Copyright(c) 2019 Pierre Krieger

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
        writeln!(out, "").unwrap();
    }

    write_commands_enum(out.by_ref(), &registry);
    writeln!(out, "").unwrap();

    write_commands_wrappers(out.by_ref(), &registry);
    writeln!(out, "").unwrap();

    write_result_structs(out.by_ref(), &registry);
    writeln!(out, "").unwrap();

    write_get_instance_proc_addr(out.by_ref(), &registry);
    writeln!(out, "").unwrap();
}

fn write_type_def(mut out: impl Write, name: &str, type_def: &parse::VkTypeDef) {
    match type_def {
        parse::VkTypeDef::Enum | parse::VkTypeDef::Bitmask | parse::VkTypeDef::Handle => {
            writeln!(out, "type {} = u32;", name).unwrap();
        },
        parse::VkTypeDef::Struct { fields } => {
            writeln!(out, "struct {} {{", name).unwrap();
            for (field_ty, field_name) in fields {
                writeln!(out, "    r#{}: {},", field_name, print_ty(&field_ty)).unwrap();
            }
            writeln!(out, "}}").unwrap();
        },
    }
}

fn write_commands_enum(mut out: impl Write, registry: &parse::VkRegistry) {
    //writeln!(out, "#[derive(Encode, Decode)]").unwrap();
    writeln!(out, "#[allow(non_camel_case_types)]").unwrap();
    writeln!(out, "pub enum VulkanMessage {{").unwrap();

    for command in &registry.commands {
        if command.name == "vkGetDeviceProcAddr" || command.name == "vkGetInstanceProcAddr" {
            continue;
        }

        writeln!(out, "    {} {{", &command.name[2..]).unwrap();
        for (param_ty, param_name) in &command.params {
            let param_ty = match param_ty {
                // Parameters that are a mutable pointers are skipped, because that's where we'll
                // write the result.
                parse::VkType::MutPointer(_, _) => continue,
                parse::VkType::ConstPointer(t, parse::VkTypePtrLen::One) => print_ty(t),
                parse::VkType::ConstPointer(t, parse::VkTypePtrLen::NullTerminated) |
                parse::VkType::ConstPointer(t, parse::VkTypePtrLen::OtherField(_)) =>
                    format!("Vec<{}>", print_ty(t)),
                t => print_ty(t),
            };

            writeln!(out, "        r#{}: {},", param_name, param_ty).unwrap();
        }
        writeln!(out, "    }},").unwrap();
    }

    writeln!(out, "}}").unwrap();
}

fn write_result_structs(mut out: impl Write, registry: &parse::VkRegistry) {
    for command in &registry.commands {
        if command.name == "vkGetDeviceProcAddr" || command.name == "vkGetInstanceProcAddr" {
            continue;
        }

        let mut fields = Vec::new();

        if command.ret_ty != parse::VkType::Ident("void".to_string()) {
            fields.push(("return_value".to_owned(), &command.ret_ty));
        }

        for (param_ty, param_name) in &command.params {
            if let parse::VkType::MutPointer(ty, len) = param_ty {
                match len {
                    parse::VkTypePtrLen::One => {
                        fields.push((format!("r#{}", param_name), ty));
                    }
                    parse::VkTypePtrLen::NullTerminated |
                    parse::VkTypePtrLen::OtherField(_) => {
                        // TODO: Vec
                        fields.push((format!("r#{}", param_name), ty));
                    }
                }
            }
        }

        // Don't generate any struct if there's no field.
        if fields.is_empty() {
            continue;
        }

        //writeln!(out, "#[derive(Encode, Decode)]").unwrap();
        writeln!(out, "#[allow(non_camel_case_types)]").unwrap();
        writeln!(out, "pub struct VkResponse{} {{", &command.name[2..]).unwrap();
        for (param_name, param_ty) in fields {
            writeln!(out, "    pub {}: {},", param_name, print_ty(&param_ty)).unwrap();
        }
        writeln!(out, "}}").unwrap();
        writeln!(out, "").unwrap();
    }
}

fn write_commands_wrappers(mut out: impl Write, registry: &parse::VkRegistry) {
    for command in &registry.commands {
        if command.name == "vkGetDeviceProcAddr" || command.name == "vkGetInstanceProcAddr" {
            continue;
        }

        writeln!(out, "extern \"system\" fn wrapper_{}(", command.name).unwrap();
        for (ty, n) in &command.params {
            writeln!(out, "    r#{}: {},", n, print_ty(ty)).unwrap();
        }
        writeln!(out, ") -> {} {{", print_ty(&command.ret_ty)).unwrap();

        writeln!(out, "    let msg = VulkanMessage::{} {{", &command.name[2..]).unwrap();
        for (param_ty, param_name) in &command.params {
            // Parameters that are a mutable pointer are skipped, because that's where we'll
            // write the result.
            match param_ty {
                //parse::VkType::ConstPointer(_, _) => continue,
                parse::VkType::MutPointer(_, _) => continue,
                _ => {}
            }
            writeln!(out, "        r#{}, ", param_name).unwrap();
        }
        writeln!(out, "    }};").unwrap();

        //write!(out, "    syscalls::block_on(syscalls::emit_message_with_response(INTERFACE, msg));").unwrap();*/
        writeln!(out, "    panic!()").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out, "").unwrap();
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
        if command.name == "vkGetDeviceProcAddr" || command.name == "vkGetInstanceProcAddr" {
            continue;
        }

        let params_tys = command.params
            .iter()
            .map(|(ty, _)| format!("{}, ", print_ty(ty)))
            .collect::<String>();

        writeln!(out, "        \"{}\" => {{", command.name).unwrap();
        writeln!(out, "            let ptr = wrapper_{} as extern \"system\" fn({}) -> {};", command.name, params_tys, print_ty(&command.ret_ty)).unwrap();
        writeln!(out, "            mem::transmute::<_, PFN_vkVoidFunction>(ptr)").unwrap();
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
        parse::VkType::MutPointer(t, _) if **t == parse::VkType::Ident("void".into()) => "*mut c_void".to_owned(),
        parse::VkType::ConstPointer(t, _) if **t == parse::VkType::Ident("void".into()) => "*const c_void".to_owned(),
        parse::VkType::MutPointer(t, _) => format!("*mut {}", print_ty(t)),
        parse::VkType::ConstPointer(t, _) => format!("*const {}", print_ty(t)),
    }
}
