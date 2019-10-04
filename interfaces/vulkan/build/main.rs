// Copyright(c) 2019 Pierre Krieger

use std::collections::HashSet;
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

    write_enum_values(out.by_ref(), &registry);
    writeln!(out, "").unwrap();

    write_commands_wrappers(out.by_ref(), &registry);
    writeln!(out, "").unwrap();

    write_get_instance_proc_addr(out.by_ref(), &registry);
    writeln!(out, "").unwrap();
}

fn write_enum_values(mut out: impl Write, registry: &parse::VkRegistry) {
    // Some of these constant values are used for constant array lengths, so we have to print them
    // out.
    //
    // Printing *all* constants (instead of just the ones we need) could be an option, but the
    // Vulkan definition files include some annoying values such as `(~0ULL)` or `(~0U-2)` that
    // are not Rust-friendly and that we don't want to bother parsing.

    let mut to_print = HashSet::new();

    fn visit_type(ty: &parse::VkType, to_print: &mut HashSet<String>) {
        match ty {
            parse::VkType::Ident(_) => {},
            parse::VkType::MutPointer(t, _) => visit_type(t, to_print),
            parse::VkType::ConstPointer(t, _) => visit_type(t, to_print),
            parse::VkType::Array(t, len) => {
                visit_type(t, to_print);
                if !len.chars().next().unwrap().is_digit(10) {
                    to_print.insert(len.clone());
                }
            },
        }
    }

    for command in &registry.commands {
        visit_type(&command.ret_ty, &mut to_print);
        for (param_ty, _) in &command.params {
            visit_type(param_ty, &mut to_print);
        }
    }

    for typedef in registry.type_defs.values() {
        match typedef {
            parse::VkTypeDef::Struct { fields } |
            parse::VkTypeDef::Union { fields } => {
                for (param_ty, _) in fields {
                    visit_type(&param_ty, &mut to_print);
                }
            },
            _ => {}
        }
    }

    for to_print in to_print {
        let value = match registry.enums.get(&to_print) {
            Some(v) => v,
            None => panic!("Can't find definition of constant {:?}", to_print),
        };

        writeln!(out, "const {}: usize = {};", to_print, value).unwrap();
    }
}

fn write_type_def(mut out: impl Write, name: &str, type_def: &parse::VkTypeDef) {
    match type_def {
        parse::VkTypeDef::Enum | parse::VkTypeDef::Bitmask | parse::VkTypeDef::Handle => {
            writeln!(out, "type {} = u32;", name).unwrap();
        },
        parse::VkTypeDef::Struct { fields } => {
            writeln!(out, "#[repr(C)]").unwrap();
            writeln!(out, "#[allow(non_snake_case)]").unwrap();
            writeln!(out, "#[derive(Copy, Clone)]").unwrap();
            writeln!(out, "struct {} {{", name).unwrap();
            for (field_ty, field_name) in fields {
                writeln!(out, "    r#{}: {},", field_name, print_ty(&field_ty)).unwrap();
            }
            writeln!(out, "}}").unwrap();
        },
        parse::VkTypeDef::Union { fields } => {
            writeln!(out, "#[repr(C)]").unwrap();
            writeln!(out, "#[allow(non_snake_case)]").unwrap();
            writeln!(out, "#[derive(Copy, Clone)]").unwrap();
            writeln!(out, "union {} {{", name).unwrap();
            for (field_ty, field_name) in fields {
                writeln!(out, "    r#{}: {},", field_name, print_ty(&field_ty)).unwrap();
            }
            writeln!(out, "}}").unwrap();
        },
    }
}

fn write_commands_wrappers(mut out: impl Write, registry: &parse::VkRegistry) {
    for command in &registry.commands {
        if command.name == "vkGetDeviceProcAddr" || command.name == "vkGetInstanceProcAddr" {
            continue;
        }

        writeln!(out, "#[allow(non_snake_case)]").unwrap();
        writeln!(out, "extern \"system\" fn wrapper_{}(", command.name).unwrap();
        for (ty, n) in &command.params {
            writeln!(out, "    r#{}: {},", n, print_ty(ty)).unwrap();
        }
        writeln!(out, ") -> {} {{", print_ty(&command.ret_ty)).unwrap();
        writeln!(out, "    let mut msg_buf = Vec::<u8>::new();    // TODO: with_capacity").unwrap();

        /*writeln!(out, "    let msg = VulkanMessage::{} {{", &command.name[2..]).unwrap();
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
        writeln!(out, "    }};").unwrap();*/

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
            .enumerate()
            .map(|(off, (ty, _))| {
                if off == 0 {
                    format!("{}", print_ty(ty))
                } else {
                    format!(", {}", print_ty(ty))
                }
            })
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
        parse::VkType::Ident(ident) if ident == "int64_t" => "i64".to_string(),
        parse::VkType::Ident(ident) if ident == "uint8_t" => "u8".to_string(),
        parse::VkType::Ident(ident) if ident == "uint16_t" => "u16".to_string(),
        parse::VkType::Ident(ident) if ident == "uint32_t" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "uint64_t" => "u64".to_string(),
        parse::VkType::Ident(ident) if ident == "size_t" => "usize".to_string(),
        parse::VkType::Ident(ident) if ident == "float" => "f32".to_string(),
        parse::VkType::Ident(ident) if ident == "double" => "f64".to_string(),

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
        parse::VkType::Ident(ident) if ident == "LPCWSTR" => "*const u16".to_string(),
        parse::VkType::Ident(ident) if ident == "HANDLE" => "*mut c_void".to_string(),
        parse::VkType::Ident(ident) if ident == "HMONITOR" => "*mut c_void".to_string(),
        parse::VkType::Ident(ident) if ident == "HWND" => "*mut c_void".to_string(),
        parse::VkType::Ident(ident) if ident == "HINSTANCE" => "*mut c_void".to_string(),
        parse::VkType::Ident(ident) if ident == "DWORD" => "u32".to_string(),
        parse::VkType::ConstPointer(t, _) if **t == parse::VkType::Ident("SECURITY_ATTRIBUTES".into()) => "*const c_void".to_owned(),

        // FIXME: the definitions below are probably false, but we don't care because we probably won't use them
        parse::VkType::Ident(ident) if ident == "xcb_connection_t" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "xcb_window_t" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "xcb_visualid_t" => "u32".to_string(),      // TODO: definitely wrong
        parse::VkType::Ident(ident) if ident == "zx_handle_t" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "Window" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "VisualID" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "RROutput" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "GgpFrameToken" => "u32".to_string(),
        parse::VkType::Ident(ident) if ident == "GgpStreamDescriptor" => "u32".to_string(),

        parse::VkType::Ident(ty) => ty.to_string(),

        parse::VkType::Array(t, arr) => format!("[{}; {}]", print_ty(t), arr),
        parse::VkType::MutPointer(t, _) if **t == parse::VkType::Ident("void".into()) => "*mut c_void".to_owned(),
        parse::VkType::ConstPointer(t, _) if **t == parse::VkType::Ident("void".into()) => "*const c_void".to_owned(),
        parse::VkType::MutPointer(t, _) => format!("*mut {}", print_ty(t)),
        parse::VkType::ConstPointer(t, _) => format!("*const {}", print_ty(t)),
    }
}
