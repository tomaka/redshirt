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

    writeln!(out, "unsafe fn redirect_handle_inner(state: &mut VulkanRedirect, mut msg_buf: &[u8]) -> Result<Option<Vec<u8>>, parity_scale_codec::Error> {{").unwrap();
    write_redirect_handle(out.by_ref(), &registry);
    writeln!(out, "}}").unwrap();
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
    for (command_id, command) in registry.commands.iter().enumerate() {
        if command.name == "vkGetDeviceProcAddr" || command.name == "vkGetInstanceProcAddr" {
            continue;
        }

        writeln!(out, "#[allow(non_snake_case)]").unwrap();
        writeln!(out, "extern \"system\" fn wrapper_{}(", command.name).unwrap();
        for (ty, n) in &command.params {
            writeln!(out, "    r#{}: {},", n, print_ty(ty)).unwrap();
        }
        writeln!(out, ") -> {} {{", print_ty(&command.ret_ty)).unwrap();
        writeln!(out, "    #![allow(unused_unsafe)]").unwrap();
        writeln!(out, "    unsafe {{").unwrap();
        writeln!(out, "        let mut msg_buf = Vec::<u8>::new();    // TODO: with_capacity").unwrap();

        // The first 2 bytes of the message is the command ID.
        writeln!(out, "        <u16 as Encode>::encode_to(&{}, &mut msg_buf);", command_id as u16).unwrap();

        // We then append every parameter one by one.
        for (param_ty, param_name) in &command.params {
            write_param_serialize(out.by_ref(), &format!("r#{}", param_name), param_ty, registry);
        }

        writeln!(out, "        let msg_id = syscalls::emit_message_raw(&INTERFACE, &msg_buf, true).unwrap().unwrap();").unwrap();
        writeln!(out, "        let response = syscalls::message_response_sync_raw(msg_id);").unwrap();

        writeln!(out, "        panic!()").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out, "").unwrap();
    }
}

fn write_param_serialize(out: &mut dyn Write, param_name: &str, param_ty: &parse::VkType, registry: &parse::VkRegistry) {
    let type_def = if let parse::VkType::Ident(ty_name) = param_ty {
        registry.type_defs.get(ty_name)
    } else {
        None
    };

    match (param_ty, type_def) {
        (parse::VkType::Ident(ty_name), _) if ty_name.starts_with("PFN_") => {
            // We skip serializing all function pointers.
        },
        (parse::VkType::Ident(ty_name), _) if ty_name == "size_t" => {
            // TODO: a size_t indicates some memory length, and it is this memory that must be serialized
        },
        (parse::VkType::Ident(ty_name), _) if ty_name == "float" => {
            writeln!(out, "        <u32 as Encode>::encode_to(&mem::transmute::<f32, u32>({}), &mut msg_buf);", param_name).unwrap()
        },
        (parse::VkType::Ident(ty_name), _) if ty_name == "HANDLE" || ty_name == "HINSTANCE" || ty_name == "HWND" || ty_name == "LPCWSTR" || ty_name == "CAMetalLayer" || ty_name == "AHardwareBuffer" => {
            // TODO: what to do here?
        },
        (parse::VkType::Ident(ty_name), _) if print_ty(param_ty).starts_with("*mut ") || print_ty(param_ty).starts_with("*const ") || print_ty(param_ty).starts_with("c_void") => {
            // TODO:
            panic!("{:?}", ty_name);
        },
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Enum)) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Bitmask)) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Handle)) |
        (parse::VkType::Ident(ty_name), None) => {
            writeln!(out, "        <{} as Encode>::encode_to(&{}, &mut msg_buf);", print_ty(param_ty), param_name).unwrap();
        },
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Struct { fields })) => {
            for (field_ty, field_name) in fields {
                write_param_serialize(out, &format!("{}.r#{}", param_name, field_name), &field_ty, registry);
            }
        }
        (parse::VkType::ConstPointer(ty_name, _), _) if **ty_name == parse::VkType::Ident("void".to_string()) => {
            // TODO: these pNext parameters :-/
        }
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::One), _) => {
            // TODO: only do this if parameter is optional?
            writeln!(out, "        if !{}.is_null() {{", param_name).unwrap();
            writeln!(out, "            <u32 as Encode>::encode_to(&1, &mut msg_buf);").unwrap();
            write_param_serialize(out, &format!("(*{})", param_name), &ty_name, registry);
            writeln!(out, "        }} else {{").unwrap();
            writeln!(out, "            <u32 as Encode>::encode_to(&0, &mut msg_buf);").unwrap();
            writeln!(out, "        }}").unwrap();
        }
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::NullTerminated), _) => {
            writeln!(out, "        if !{}.is_null() {{", param_name).unwrap();
            writeln!(out, "            let len = (0isize..).find(|n| *{}.offset(*n) == 0).unwrap() as u32;", param_name).unwrap();
            writeln!(out, "            <u32 as Encode>::encode_to(&len, &mut msg_buf);").unwrap();
            writeln!(out, "            for n in 0..len {{").unwrap();
            write_param_serialize(out, &format!("(*{}.offset(n as isize))", param_name), &ty_name, registry);
            writeln!(out, "            }}").unwrap();
            writeln!(out, "        }} else {{").unwrap();
            writeln!(out, "            <u32 as Encode>::encode_to(&0, &mut msg_buf);").unwrap();
            writeln!(out, "        }}").unwrap();
        }
        (parse::VkType::Array(ty_name, len), _) => {
            writeln!(out, "        for val in (0..{}).map(|n| {}[n]) {{", len, param_name).unwrap();
            write_param_serialize(out, "val", &ty_name, registry);
            writeln!(out, "        }}").unwrap();
        },
        _ => {}     // TODO: remove default fallback so that we're explicit
    }
}

fn write_redirect_handle(mut out: impl Write, registry: &parse::VkRegistry) {
    writeln!(out, "match <u16 as Decode>::decode(&mut msg_buf)? {{").unwrap();

    for (command_id, command) in registry.commands.iter().enumerate() {
        if command.name == "vkGetDeviceProcAddr" || command.name == "vkGetInstanceProcAddr" {
            continue;
        }

        writeln!(out, "    {} => {{ // {}", command_id, command.name).unwrap();
        for (param_ty, param_name) in &command.params {
            let mut interm = Vec::new();
            if let Ok(()) = write_param_deserialize_step1(&mut interm, param_ty, registry) {
                write!(out, "let r#{} = ", param_name).unwrap();
                out.write_all(&interm).unwrap();
                writeln!(out, ";").unwrap();
            }
        }
        writeln!(out, "        Ok(None)").unwrap();
        writeln!(out, "    }},").unwrap();
    }

    writeln!(out, "    _ => panic!()").unwrap();        // TODO: don't panic
    writeln!(out, "}}").unwrap();
}

fn write_param_deserialize_step1(out: &mut dyn Write, ty: &parse::VkType, registry: &parse::VkRegistry) -> Result<(), ()> {
    let type_def = if let parse::VkType::Ident(ty_name) = ty {
        registry.type_defs.get(ty_name)
    } else {
        None
    };

    match (ty, type_def) {
        (parse::VkType::Ident(ty_name), _) if ty_name.starts_with("PFN_") => {
            write!(out, "ptr::null::<{}>()", print_ty(ty)).unwrap()
        },
        (parse::VkType::Ident(ty_name), _) if ty_name == "size_t" => {
            // TODO: ???
            write!(out, "0").unwrap()
        },
        (parse::VkType::Ident(ty_name), _) if ty_name == "float" => {
            write!(out, "mem::transmute::<u32, f32>(Decode::decode(&mut msg_buf)?)").unwrap()
        },
        (parse::VkType::Ident(ty_name), _) if ty_name == "HANDLE" || ty_name == "HINSTANCE" || ty_name == "HWND" || ty_name == "LPCWSTR" || ty_name == "CAMetalLayer" || ty_name == "AHardwareBuffer" => {
            // TODO: what to do here?
            write!(out, "ptr::null::<{}>()", print_ty(ty)).unwrap()
        },
        (parse::VkType::Ident(ty_name), _) if print_ty(ty).starts_with("*mut ") || print_ty(ty).starts_with("*const ") || print_ty(ty).starts_with("c_void") => {
            // TODO:
            panic!("{:?}", ty_name);
        },
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Enum)) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Bitmask)) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Handle)) |
        (parse::VkType::Ident(ty_name), None) => {
            write!(out, "<{} as Decode>::decode(&mut msg_buf)?", print_ty(ty)).unwrap();
        },
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Struct { fields })) => {
            writeln!(out, "{{").unwrap();
            let mut fields_out = Vec::new();
            for (field_ty, field_name) in fields {
                let mut interm = Vec::new();
                if let Ok(()) = write_param_deserialize_step1(&mut interm, &field_ty, registry) {
                    write!(out, "let r#{} = ", field_name).unwrap();
                    out.write_all(&interm).unwrap();
                    writeln!(out, ";").unwrap();
                    fields_out.push(field_name);
                }
            }
            write!(out, "(").unwrap();
            for field_name in fields_out {
                write!(out, "r#{}, ", field_name).unwrap();
            }
            writeln!(out, ") }}").unwrap();
        }
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Union { fields })) => {
            writeln!(out, "mem::zeroed::<{}>()", print_ty(ty)).unwrap();    // FIXME:
        }
        (parse::VkType::ConstPointer(ty_name, _), _) if **ty_name == parse::VkType::Ident("void".to_string()) => {
            // TODO: these pNext parameters :-/
            write!(out, "ptr::null::<{}>()", print_ty(ty)).unwrap()
        }
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::One), _) => {
            // TODO: only do this if parameter is optional?
            write!(out, "if <u32 as Decode>::decode(&mut msg_buf)? != 0 {{ Some(").unwrap();
            write_param_deserialize_step1(out, &ty_name, registry).unwrap();
            write!(out, ") }} else {{ None }}").unwrap();
        }
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::NullTerminated), _) => {
            // TODO: can be optimized by grabbing a pointer to `msg_buf` itself if the type is
            //       [u8] (which is always the case in practice for null-terminated)
            writeln!(out, "{{ let len = <u32 as Decode>::decode(&mut msg_buf)? as usize;").unwrap();
            writeln!(out, "let mut out = Vec::with_capacity(len);").unwrap();
            writeln!(out, "for n in 0..len {{ out.push(").unwrap();
            write_param_deserialize_step1(out, &ty_name, registry).unwrap();
            writeln!(out, "); }}").unwrap();
            writeln!(out, "out }}").unwrap();
        }
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::OtherField(rust_expr)), _) |
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::RustExpr(rust_expr)), _) => {
            writeln!(out, "{{").unwrap();
            writeln!(out, "let mut out = Vec::with_capacity(({}) as usize);", rust_expr).unwrap();
            writeln!(out, "for n in 0..{} {{ out.push(", rust_expr).unwrap();
            write_param_deserialize_step1(out, &ty_name, registry).unwrap();
            writeln!(out, "); }}").unwrap();
            writeln!(out, "out }}").unwrap();
        }
        (parse::VkType::MutPointer(_, _), _) => {
            return Err(())
        }
        (parse::VkType::Array(ty_name, len), _) => {
            // TODO: no
            /*writeln!(out, "        for val in (0..{}).map(|n| {}[n]) {{", len, param_name).unwrap();
            write_param_serialize(out, "val", &ty_name, registry);
            writeln!(out, "        }}").unwrap();*/
            writeln!(out, "mem::zeroed::<[{}; {}]>()", print_ty(ty_name), len).unwrap();
        }
    }

    Ok(())
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
        parse::VkType::Ident(ident) if ident == "char" => "u8".to_string(),
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
