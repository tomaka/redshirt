// Copyright (C) 2019  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::{Cursor, Write};
use std::path::Path;

const VK_XML: &[u8] = include_bytes!("../vk.xml");

mod fpointers;
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

    write_redirect_handle(out.by_ref(), &registry);
    writeln!(out, "").unwrap();

    fpointers::write_pointers_structs(out.by_ref(), &registry).unwrap();
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
        parse::VkTypeDef::Enum | parse::VkTypeDef::Bitmask => {
            writeln!(out, "type {} = u32;", name).unwrap();
        },
        parse::VkTypeDef::NonDispatchableHandle => {
            writeln!(out, "type {} = u64;", name).unwrap();
        },
        parse::VkTypeDef::DispatchableHandle => {
            writeln!(out, "type {} = usize;", name).unwrap();
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
        writeln!(out, "unsafe extern \"system\" fn wrapper_{}(", command.name).unwrap();
        for (ty, n) in &command.params {
            writeln!(out, "    r#{}: {},", n, print_ty(ty)).unwrap();
        }
        writeln!(out, ") -> {} {{", print_ty(&command.ret_ty)).unwrap();
        writeln!(out, "    let mut msg_buf = Vec::<u8>::new();    // TODO: with_capacity").unwrap();

        // The first 2 bytes of the message is the command ID.
        writeln!(out, "    <u16 as Encode>::encode_to(&{}, &mut msg_buf);", command_id as u16).unwrap();

        // We then append every parameter one by one.
        for (param_ty, param_name) in &command.params {
            write_serialize(
                out.by_ref(),
                false,
                &format!("r#{}", param_name),
                param_ty,
                registry,
                &mut |handle| format!("{} as u32", handle),      // TODO: debug_assert! that we fit in u32?
                &mut |fields| {
                    parse::gen_path_subfield_in_list(&command.params, registry, fields).map(|(n, ty)| (n, ty.clone())).unwrap()
                }
            );
        }

        writeln!(out, "    let msg_id = syscalls::emit_message_raw(&INTERFACE, &msg_buf, true).unwrap().unwrap();").unwrap();
        writeln!(out, "    let response = syscalls::message_response_sync_raw(msg_id);").unwrap();
        writeln!(out, "    println!(\"got response: {{:?}}\", response);").unwrap();

        writeln!(out, "    let response_read = |mut msg_buf: &[u8]| -> Result<{}, parity_scale_codec::Error> {{", print_ty(&command.ret_ty)).unwrap();
        let ret_value_expr = write_deserialize(&command.ret_ty, registry, &mut |_, _| panic!());
        writeln!(out, "        let ret = {};", ret_value_expr).unwrap();
        for (param_ty, param_name) in &command.params {
            assert_ne!(param_name, "ret");      // TODO: rename parameters instead
            let mut var_name = 0;       // TODO: generalize to whole function
            let write_back = write_deserialize_response_into(param_ty, registry, &mut || { let n = var_name; var_name += 1; format!("n{}", n) }, param_name, false);
            writeln!(out, "        {}", write_back).unwrap();
        }
        writeln!(out, "            assert!(msg_buf.is_empty(), \"Remaining after response: {{:?}}\", msg_buf.len());").unwrap();     // TODO: return Error
        writeln!(out, "        Ok(ret)").unwrap();
        writeln!(out, "    }};").unwrap();
        writeln!(out, "    response_read(&response).unwrap()").unwrap();

        writeln!(out, "}}").unwrap();
        writeln!(out, "").unwrap();
    }
}

fn write_redirect_handle(mut out: impl Write, registry: &parse::VkRegistry) {
    writeln!(out, "unsafe fn redirect_handle_inner(state: &mut VulkanRedirect, emitter_pid: u64, mut msg_buf: &[u8]) -> Result<Option<Vec<u8>>, parity_scale_codec::Error> {{").unwrap();
    writeln!(out, "    #![allow(unused_parens)]").unwrap();
    writeln!(out, "    match <u16 as Decode>::decode(&mut msg_buf)? {{").unwrap();
    for (command_id, command) in registry.commands.iter().enumerate() {
        if command.name == "vkGetDeviceProcAddr" || command.name == "vkGetInstanceProcAddr" {
            continue;
        }
        writeln!(out, "        {} => handle_{}(state, emitter_pid, msg_buf),", command_id, command.name).unwrap();
    }
    writeln!(out, "        _ => panic!()").unwrap();        // TODO: don't panic
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out, "").unwrap();

    for command in &registry.commands {
        if command.name == "vkGetDeviceProcAddr" || command.name == "vkGetInstanceProcAddr" {
            continue;
        }

        writeln!(out, "unsafe fn handle_{}(state: &mut VulkanRedirect, emitter_pid: u64, mut msg_buf: &[u8]) -> Result<Option<Vec<u8>>, parity_scale_codec::Error> {{", command.name).unwrap();

        let mut var_name_gen = 0;
        let mut params_list = Vec::new();
        let mut params = String::new();
        for (param_num, (param_ty, param_name)) in command.params.iter().enumerate() {
            let expr = write_deserialize(param_ty, registry, &mut |interm, mutable| {
                let v_name = format!("n{}", var_name_gen);
                var_name_gen += 1;
                let mutable = if mutable { format!("mut ") } else { String::new() };
                writeln!(out, "    let {}{} = {};", mutable, v_name, interm).unwrap();
                v_name
            });

            let v_name = format!("n{}", var_name_gen);
            var_name_gen += 1;
            writeln!(out, "    let {} = {};", v_name, expr).unwrap();
            if !params.is_empty() { params.push_str(", "); }
            params_list.push(v_name.clone());
            params.push_str(&v_name);
        }

        // TODO: shouldn't panic but return an error instead
        let f_ptr = match fpointers::command_ty(&command) {
            fpointers::CommandTy::Static => format!("(state.static_pointers.r#{}.unwrap())", command.name),
            fpointers::CommandTy::Instance => format!("(state.instance_pointers.get(&{}).unwrap().r#{}.unwrap())", params_list[0], command.name),
            fpointers::CommandTy::PhysicalDevice => format!("{{ \
                let instance = state.instance_of_physical_devices.get(&{}).unwrap(); \
                let ptrs = state.instance_pointers.get(&instance).unwrap(); \
                ptrs.r#{}.unwrap() \
            }}", params_list[0], command.name),
            fpointers::CommandTy::Device => format!("{{ \
                let ptrs = match state.device_pointers.get(&{dev}) {{ \
                    Some(p) => p, \
                    None => panic!(\"No pointers for device {{:?}}\", {dev}), \
                }}; \
                match ptrs.r#{cmd} {{
                    Some(p) => p,
                    None => panic!(\"No pointer for {cmd}\")
                }} \
            }}", dev = params_list[0], cmd = command.name),
            fpointers::CommandTy::Queue => format!("{{ \
                let instance = state.device_of_queues.get(&{}).unwrap(); \
                let ptrs = state.device_pointers.get(&instance).unwrap(); \
                ptrs.r#{}.unwrap() \
            }}", params_list[0], command.name),
            fpointers::CommandTy::CommandBuffer => format!("{{ \
                let instance = state.device_of_command_buffers.get(&{}).unwrap(); \
                let ptrs = state.device_pointers.get(&instance).unwrap(); \
                ptrs.r#{}.unwrap() \
            }}", params_list[0], command.name),
        };

        writeln!(out, "    assert!(msg_buf.is_empty(), \"Remaining: {{:?}}\", msg_buf.len());").unwrap();     // TODO: return Error
        writeln!(out, "    println!(\"calling vk function\");").unwrap();
        writeln!(out, "    let ret = {}({});", f_ptr, params).unwrap();

        // As special additions, if this is `vkCreateInstance`, `vkDestroyInstance`,
        // `vkCreateDevice`, or `vkDestroyDevice`, we need to load or unload function pointers.
        // TODO: move that in lib.rs?
        if command.name == "vkCreateInstance" {
            writeln!(out, "    state.assign_handle_to_pid(*{}, emitter_pid);", params_list[2]).unwrap();
            writeln!(out, "    state.instance_pointers.insert(*{}, InstancePtrs::load_with(|name: &std::ffi::CStr| {{", params_list[2]).unwrap();
            writeln!(out, "        (state.get_instance_proc_addr)(*{}, name.as_ptr() as *const _)", params_list[2]).unwrap();
            writeln!(out, "    }}));").unwrap();
        } else if command.name == "vkDestroyInstance" {
            // TODO: also deassign all the devices, queues, command buffers, and physical devices
            writeln!(out, "    state.deassign_handle({});", params_list[0]).unwrap();
            writeln!(out, "    state.instance_pointers.remove(&{});", params_list[0]).unwrap();
        } else if command.name == "vkCreateDevice" {
            writeln!(out, "    state.assign_handle_to_pid(*{}, emitter_pid);", params_list[3]).unwrap();
            writeln!(out, "    state.device_pointers.insert(*{}, {{", params_list[3]).unwrap();
            writeln!(out, "        let instance = state.instance_of_physical_devices.get(&{}).unwrap();", params_list[0]).unwrap();
            writeln!(out, "        let instance_ptrs = state.instance_pointers.get(&instance).unwrap();").unwrap();
            writeln!(out, "        let dev_get = instance_ptrs.vkGetDeviceProcAddr.unwrap();").unwrap();
            writeln!(out, "        DevicePtrs::load_with(|name: &std::ffi::CStr| {{").unwrap();
            writeln!(out, "            dev_get(*{}, name.as_ptr() as *const _)", params_list[3]).unwrap();
            writeln!(out, "        }})").unwrap();
            writeln!(out, "    }});").unwrap();
        } else if command.name == "vkDestroyDevice" {
            // TODO: also deassign all the queues and command buffers
            writeln!(out, "    state.deassign_handle({});", params_list[0]).unwrap();
            writeln!(out, "    state.device_pointers.remove(&{});", params_list[0]).unwrap();
        } else if command.name == "vkEnumeratePhysicalDevices" {
            writeln!(out, "    debug_assert!(!{}.is_null());", params_list[1]).unwrap();
            writeln!(out, "    if !{}.is_null() {{", params_list[2]).unwrap();
            writeln!(out, "        for n in 0..(*{}) {{", params_list[1]).unwrap();
            writeln!(out, "            state.instance_of_physical_devices.insert(*({}.offset(n as isize)), {});", params_list[2], params_list[0]).unwrap();
            writeln!(out, "            state.assign_handle_to_pid(*({}.offset(n as isize)), emitter_pid);", params_list[2]).unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "    }}").unwrap();
        } else if command.name == "vkAllocateCommandBuffers" {
            writeln!(out, "    debug_assert!(!{}.is_null());", params_list[1]).unwrap();
            writeln!(out, "    for n in 0..(*{}).commandBufferCount {{", params_list[1]).unwrap();
            writeln!(out, "        state.device_of_command_buffers.insert(*({}.offset(n as isize)), {});", params_list[2], params_list[0]).unwrap();
            writeln!(out, "        state.assign_handle_to_pid(*({}.offset(n as isize)), emitter_pid);", params_list[2]).unwrap();
            writeln!(out, "    }}").unwrap();
        } else if command.name == "vkGetDeviceQueue" {
            writeln!(out, "    state.assign_handle_to_pid(*{}, emitter_pid);", params_list[3]).unwrap();
            writeln!(out, "    state.device_of_queues.insert(*{}, {});", params_list[3], params_list[0]).unwrap();
        } else if command.name == "vkGetDeviceQueue2" {
            writeln!(out, "    state.assign_handle_to_pid(*{}, emitter_pid);", params_list[2]).unwrap();
            writeln!(out, "    state.device_of_queues.insert(*{}, {});", params_list[2], params_list[0]).unwrap();
        }  // TODO: handle vkFreeComandBuffers

        writeln!(out, "    let mut msg_buf = Vec::new();").unwrap();     // TODO: with_capacity()?
        write_serialize(out.by_ref(), false, "ret", &command.ret_ty, registry, &mut |_| panic!(), &mut |_| panic!());
        for ((param_ty, _), param_var) in command.params.iter().zip(params_list.iter()) {
            write_serialize(out.by_ref(), true, param_var, param_ty, registry, &mut |handle| format!("state.handles_host_to_vm.get(&{}).unwrap().1", handle), &mut |fields| {
                if fields.len() >= 2 {
                    (format!("panic!()"), parse::VkType::Ident("int".to_string()))       // TODO:
                } else {
                    let field_num = command.params.iter().position(|p| p.1 == fields[0]).unwrap();
                    (params_list[field_num].clone(), command.params[field_num].0.clone())
                }
            });
        }

        // TODO: for now the caller always expects a response
        //writeln!(out, "    if !msg_buf.is_empty() {{").unwrap();
        writeln!(out, "        Ok(Some(msg_buf))").unwrap();
        //writeln!(out, "    }} else {{").unwrap();
        //writeln!(out, "        Ok(None)").unwrap();
        //writeln!(out, "    }}").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out, "").unwrap();
    }
}

/// Generates Rust code that serializes a Vulkan data structure into a buffer.
///
/// If `skip_const` is true, then we only serialize elements that are behind a mutable pointers,
/// i.e. elements that might have been modified by an earlier call to a Vulkan function.
fn write_serialize(
    out: &mut dyn Write,
    skip_const: bool,
    param_name: &str,
    param_ty: &parse::VkType,
    registry: &parse::VkRegistry,
    serialize_handles: &mut dyn FnMut(&str) -> String,
    other_field: &mut dyn FnMut(&[String]) -> (String, parse::VkType)
) {
    let type_def = if let parse::VkType::Ident(ty_name) = param_ty {
        registry.type_defs.get(ty_name)
    } else {
        None
    };

    match (param_ty, type_def, skip_const) {
        (parse::VkType::Ident(ty_name), _, _) if ty_name.starts_with("PFN_") => {
            // We skip serializing all function pointers.
        },
        (parse::VkType::Ident(ty_name), _, _) if ty_name == "void" => {
            // Nothing to do when serializing `void`.
        },
        (parse::VkType::Ident(ty_name), _, _) if ty_name == "size_t" => {
            // A `size_t` is platform-specific, but by using the `Compact` we can make it the same size everywhere.
            writeln!(out, "        <Compact<u128> as Encode>::encode_to(&Compact({} as u128), &mut msg_buf);", param_name).unwrap();
        },
        (parse::VkType::Ident(ty_name), _, _) if ty_name == "float" => {
            writeln!(out, "        <u32 as Encode>::encode_to(&mem::transmute::<f32, u32>({}), &mut msg_buf);", param_name).unwrap()
        },
        (parse::VkType::Ident(ty_name), _, _) if ty_name == "HANDLE" || ty_name == "HINSTANCE" || ty_name == "HWND" || ty_name == "LPCWSTR" || ty_name == "CAMetalLayer" || ty_name == "AHardwareBuffer" => {
            // TODO: what to do here?
        },
        (parse::VkType::Ident(ty_name), _, _) if print_ty(param_ty).starts_with("*mut ") || print_ty(param_ty).starts_with("*const ") || print_ty(param_ty).contains("c_void") => {
            // Platform-specific handles are serialized using the `Compact` encoding, so we can make it the same size everywhere.
            writeln!(out, "        <Compact<u128> as Encode>::encode_to(&Compact({} as usize as u128), &mut msg_buf);", param_name).unwrap();
        },
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Enum), false) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Bitmask), false) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::NonDispatchableHandle), false) |
        (parse::VkType::Ident(ty_name), None, false) => {
            writeln!(out, "        <{} as Encode>::encode_to(&{}, &mut msg_buf);", print_ty(param_ty), param_name).unwrap();
        },
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::DispatchableHandle), false) => {
            let serialize = serialize_handles(param_name);
            writeln!(out, "        <u32 as Encode>::encode_to(&({}), &mut msg_buf);", serialize).unwrap();
        }
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Enum), true) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Bitmask), true) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::NonDispatchableHandle), true) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::DispatchableHandle), true) |
        (parse::VkType::Ident(ty_name), None, true) => {
        },
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Struct { fields }), _) => {
            for (field_ty, field_name) in fields {
                write_serialize(
                    out,
                    skip_const,
                    &format!("{}.r#{}", param_name, field_name),
                    &field_ty,
                    registry,
                    serialize_handles,
                    &mut |to_find| {
                        let mut ret = param_name.to_owned();
                        for f in to_find { ret.push_str(".r#"); ret.push_str(f); }
                        let ty = parse::find_subfield_in_list(fields, registry, to_find).unwrap().clone();
                        (ret, ty)
                    }
                );
            }
        }
        (parse::VkType::MutPointer(ty_name, _), _, _) |
        (parse::VkType::ConstPointer(ty_name, _), _, _) if print_ty(&param_ty).contains("void") => {
            // TODO: these pNext parameters :-/
            writeln!(out, "        assert!({}.is_null());", param_name).unwrap();
        }
        (parse::VkType::MutPointer(ty_name, parse::VkTypePtrLen::One), _, _) |
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::One), _, false) => {
            // TODO: only do this if parameter is optional?
            writeln!(out, "        if !{}.is_null() {{", param_name).unwrap();
            writeln!(out, "            <u32 as Encode>::encode_to(&1, &mut msg_buf);").unwrap();
            write_serialize(out, false, &format!("(*{})", param_name), &ty_name, registry, serialize_handles, other_field);
            writeln!(out, "        }} else {{").unwrap();
            writeln!(out, "            <u32 as Encode>::encode_to(&0, &mut msg_buf);").unwrap();
            writeln!(out, "        }}").unwrap();
        }
        (parse::VkType::MutPointer(ty_name, parse::VkTypePtrLen::NullTerminated), _, _) |
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::NullTerminated), _, false) => {
            writeln!(out, "        if !{}.is_null() {{", param_name).unwrap();
            writeln!(out, "            let len = (0isize..).find(|n| *{}.offset(*n) == 0).unwrap() as u32;", param_name).unwrap();
            writeln!(out, "            <u32 as Encode>::encode_to(&len, &mut msg_buf);").unwrap();
            writeln!(out, "            for n in 0..len {{").unwrap();
            write_serialize(out, false, &format!("(*{}.offset(n as isize))", param_name), &ty_name, registry, serialize_handles, other_field);
            writeln!(out, "            }}").unwrap();
            writeln!(out, "        }} else {{").unwrap();
            writeln!(out, "            <u32 as Encode>::encode_to(&0, &mut msg_buf);").unwrap();
            writeln!(out, "        }}").unwrap();
        }
        (parse::VkType::MutPointer(ty_name, parse::VkTypePtrLen::OtherField { before_other_field, other_field: field, after_other_field }), _, _) |
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::OtherField { before_other_field, other_field: field, after_other_field }), _, _) => {
            let (other_field_name, other_field_ty) = other_field(field);
            let other_field_name = other_field_ty.gen_deref_expr(&other_field_name);

            writeln!(out, "        if !{}.is_null() {{", param_name).unwrap();      // TODO: remove?
            writeln!(out, "            let len = {}{}{};", before_other_field, other_field_name, after_other_field).unwrap();
            writeln!(out, "            <u32 as Encode>::encode_to(&(len as u32), &mut msg_buf);").unwrap();      // TODO: remove?
            writeln!(out, "            for m in 0..len {{").unwrap();       // TODO: might conflict with other variable name
            write_serialize(out, false, &format!("(*{}.offset(m as isize))", param_name), &ty_name, registry, serialize_handles, other_field);
            writeln!(out, "            }}").unwrap();
            writeln!(out, "        }} else {{").unwrap();
            writeln!(out, "            <u32 as Encode>::encode_to(&0, &mut msg_buf);").unwrap();
            writeln!(out, "        }}").unwrap();
        }
        (parse::VkType::Array(ty_name, len), _, _) => {
            writeln!(out, "        for val in (0..{}).map(|n| {}[n]) {{", len, param_name).unwrap();
            write_serialize(out, skip_const, "val", &ty_name, registry, serialize_handles, other_field);
            writeln!(out, "        }}").unwrap();
        },
        _ => {}     // TODO: remove default fallback so that we're explicit
    }
}

/// Generates Rust code that turns a serialized call into a Vulkan data structure.
///
/// Because of various difficulties, the API of this function is a bit tricky.
/// The function must return an expression that decodes a value of type `ty` from a local variable
/// named `msg_buf`.
///
/// Because `ty` might contain pointers, the `interm_step_gen` function can be used in order to
/// create a local variable and "pin" it. The closure takes as parameter an expression to put in
/// the variable, and returns the name of the local variable. The local variable must not move.
/// The second parameter to the closure indicates whether the variable must be mutable.
///
/// Keep in mind that the expression passed to `interm_step_gen` can decode data from `msg_buf`.
/// You must be careful to respect the order of operations. If `interm_step_gen` is called
/// multiple times, the expressions must be executed in the order in which the closure has been
/// called.
///
/// Also note that the code generated by the implementation is deterministic. This allows one
/// to call [`write_deserialize`] once and use the produced code in a loop.
///
// TODO: talk about environment: what is available to use, and the `?` operator
fn write_deserialize(
    ty: &parse::VkType,
    registry: &parse::VkRegistry,
    interm_step_gen: &mut dyn FnMut(String, bool) -> String,
) -> String {
    let type_def = if let parse::VkType::Ident(ty_name) = ty {
        registry.type_defs.get(ty_name)
    } else {
        None
    };

    match (ty, type_def) {
        (parse::VkType::Ident(ty_name), _) if ty_name.starts_with("PFN_") => {
            format!("mem::transmute::<_, {}>(0usize)", print_ty(ty))
        },
        (parse::VkType::Ident(ty_name), _) if ty_name == "size_t" => {
            format!("<Compact<u128> as Decode>::decode(&mut msg_buf)?.0 as {}", print_ty(ty))
        },
        (parse::VkType::Ident(ty_name), _) if ty_name == "float" => {
            format!("mem::transmute::<u32, f32>(Decode::decode(&mut msg_buf)?)")
        },
        (parse::VkType::Ident(ty_name), _) if ty_name == "LPCWSTR" || ty_name == "CAMetalLayer" || ty_name == "AHardwareBuffer" => {
            // TODO: what to do here?
            format!("{{ let v: {} = panic!(); v }}", print_ty(ty))
        },
        (parse::VkType::Ident(ty_name), _) if ty_name == "HANDLE" || ty_name == "HINSTANCE" || ty_name == "HWND" => {
            // TODO: what to do here?
            format!("{{ let v: {} = panic!(); v }}", print_ty(ty))
        },
        (parse::VkType::Ident(ty_name), _) if print_ty(ty).starts_with("*mut ") || print_ty(ty).starts_with("*const ") || print_ty(ty).starts_with("c_void") => {
            format!("<Compact<u128> as Decode>::decode(&mut msg_buf)?.0 as usize as {}", print_ty(ty))
        },
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Enum)) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Bitmask)) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::NonDispatchableHandle)) |
        (parse::VkType::Ident(ty_name), None) => {
            format!("<{} as Decode>::decode(&mut msg_buf)?", print_ty(ty))
        },
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::DispatchableHandle)) => {
            // We need to be tolerant on the lack on value in `handles_vm_to_host`, as the call
            // might happen before the handle is created.
            format!("if let Some(val) = state.handles_vm_to_host.get(&(emitter_pid, <u32 as Decode>::decode(&mut msg_buf)?)) {{ *val }} else {{ 0 }}")
        },

        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Struct { fields })) => {
            let mut field_names = String::new();
            for (field_ty, field_name) in fields {
                let field_expr = write_deserialize(field_ty, registry, &mut |e, mutable| interm_step_gen(e, mutable));
                let field_interm = interm_step_gen(format!("/* {}::{} */ {}", ty_name, field_name, field_expr), false);
                if !field_names.is_empty() { field_names.push_str(", "); }
                field_names.push_str(&format!("r#{}: {}", field_name, field_interm));
            }

            format!("{} {{ {} }}", ty_name, field_names)
        }

        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Union { fields })) => {
            // FIXME: implement properly
            format!("mem::zeroed::<{}>()", print_ty(ty))
        }

        (parse::VkType::MutPointer(ty_name, _), _) |
        (parse::VkType::ConstPointer(ty_name, _), _) if print_ty(ty).contains("void") => {
            // TODO: what to do here?
            format!("mem::zeroed::<{}>()", print_ty(ty))
        }

        (parse::VkType::MutPointer(ty_name, parse::VkTypePtrLen::One), _) |
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::One), _) => {
            let is_mut = if let parse::VkType::MutPointer(_, _) = ty { true } else { false };
            let is_present = interm_step_gen("<u32 as Decode>::decode(&mut msg_buf)? != 0".to_owned(), false);

            let interm = {
                let inner = write_deserialize(&ty_name, registry, &mut |f, mutable| {
                    let var = interm_step_gen(format!("if {} {{ Some({}) }} else {{ None }}", is_present, f), mutable);
                    if mutable {
                        format!("(*{}.as_mut().unwrap())", var)
                    } else {
                        format!("(*{}.as_ref().unwrap())", var)
                    }
                });

                format!("if {} {{ Some({}) }} else {{ None }}", is_present, inner)
            };

            let var = interm_step_gen(interm, is_mut);
            if is_mut {
                format!("{}.as_mut().map(|p| p as *mut _).unwrap_or(ptr::null_mut())", var)
            } else {
                format!("{}.as_ref().map(|p| p as *const _).unwrap_or(ptr::null())", var)
            }
        }

        (parse::VkType::MutPointer(ty_name, parse::VkTypePtrLen::NullTerminated), _) |
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::NullTerminated), _) => {
            // Pointers, when serialized, always start with the number of elements.
            let len_var = interm_step_gen(format!("/* len({}) */ <u32 as Decode>::decode(&mut msg_buf)? as usize", print_ty(ty_name)), false);

            let interm = {
                let inner = write_deserialize(&ty_name, registry, &mut |f, mutable| {
                    let var = interm_step_gen(format!("{{ \
                        let mut list = Vec::with_capacity({len}); \
                        for _n in 0..{len} {{ list.push({inner}); }} \
                        list \
                    }}", inner=f, len=len_var), mutable);
                    format!("{}[_n]", var)
                });

                format!("{{ \
                    let mut list = Vec::with_capacity({len}); \
                    for _n in 0..{len} {{ list.push({inner}); }} \
                    list.push(0); \
                    list \
                }}", inner=inner, len=len_var)
            };

            let var = interm_step_gen(interm, false);
            format!("if !{var}.is_empty() {{ {var}.as_ptr() }} else {{ ptr::null() }}", var=var)
        }

        (parse::VkType::MutPointer(ty_name, parse::VkTypePtrLen::OtherField { .. }), _) |
        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::OtherField { .. }), _) => {    
            // Pointers, when serialized, always start with the number of elements.
            let len_var = interm_step_gen(format!("/* len({}) */ <u32 as Decode>::decode(&mut msg_buf)? as usize", print_ty(ty_name)), false);

            let is_mut = if let parse::VkType::MutPointer(_, _) = ty { true } else { false };

            let interm = {
                let inner = write_deserialize(&ty_name, registry, &mut |f, mutable| {
                    let var = interm_step_gen(format!("{{ \
                        let mut list = Vec::with_capacity({len}); \
                        for _n in 0..{len} {{ list.push({inner}); }} \
                        list \
                    }}", inner=f, len=len_var), mutable);
                    format!("{}[_n]", var)
                });

                format!("{{ \
                    let mut list = Vec::with_capacity({len}); \
                    for _n in 0..{len} {{ list.push({inner}); }} \
                    list \
                }}", inner=inner, len=len_var)
            };

            let var = interm_step_gen(interm, is_mut);
            if is_mut {
                format!("if !{var}.is_empty() {{ {var}.as_mut_ptr() }} else {{ ptr::null_mut() }}", var=var)
            } else {
                format!("if !{var}.is_empty() {{ {var}.as_ptr() }} else {{ ptr::null() }}", var=var)
            }
        }

        (parse::VkType::Array(ty_name, len), _) => {
            // TODO: MaybeUninit isn't used correctly, but ¯\_(ツ)_/¯
            let inner = write_deserialize(&ty_name, registry, &mut |f, mutable| {
                let var = interm_step_gen(format!("{{ \
                    let mut array = mem::MaybeUninit::<[_; {len}]>::uninit(); \
                    for _n in 0..{len} {{ (&mut *array.as_mut_ptr())[_n as usize] = ({inner}); }} \
                    array.assume_init() \
                }}", inner=f, len=len), mutable);
                format!("{}[_n]", var)
            });

            format!("{{ \
                let mut array = mem::MaybeUninit::<[_; {len}]>::uninit(); \
                for _n in 0..{len} {{ (&mut *array.as_mut_ptr())[_n as usize] = ({inner}); }} \
                array.assume_init() \
            }}", inner=inner, len=len)
        }
    }
}

/// Generates Rust code that deserializes a response to a Vulkan function call and writes it
/// to `out`.
///
/// When you call a Vulkan function, some of the parameters include mutable pointers that the
/// Vulkan function writes to.
/// Since we're using a proxying mechanism, the mutable pointers themselves are not transmitted.
/// Instead, the data that Vulkan writes to the given pointer is serialized into the response, and
/// this function deserializes that and writes the data.
///
/// This function is called for each parameter made for a Vulkan function call, but most
/// parameters are ignored.
///
/// This function must generate statements (for example: "foo = 5;").
fn write_deserialize_response_into(
    ty: &parse::VkType,
    registry: &parse::VkRegistry,
    var_name_assign: &mut dyn FnMut() -> String,
    out_var_name: &str,
    force_write: bool,
) -> String {
    let type_def = if let parse::VkType::Ident(ty_name) = ty {
        registry.type_defs.get(ty_name)
    } else {
        None
    };

    match (ty, type_def, force_write) {
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Enum), false) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Bitmask), false) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::NonDispatchableHandle), false) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::DispatchableHandle), false) |
        (parse::VkType::Ident(ty_name), None, false) => String::new(),

        (parse::VkType::Ident(ty_name), _, true) if ty_name == "size_t" => {
            // A `size_t` is platform-specific, but by using the `Compact` we can make it the same size everywhere.
            format!("{} = <Compact<u128> as Decode>::decode(&mut msg_buf)?.0 as usize;", out_var_name)
        },
        (parse::VkType::Ident(ty_name), _, true) if ty_name == "float" => {
            format!("{} = mem::transmute::<u32, f32>(<u32 as Decode>::decode(&mut msg_buf)?);", out_var_name)
        },

        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Enum), true) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Bitmask), true) |
        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::NonDispatchableHandle), true) |
        (parse::VkType::Ident(ty_name), None, true) => {
            format!("{} = <{} as Decode>::decode(&mut msg_buf)?;", out_var_name, print_ty(ty))
        },

        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::DispatchableHandle), true) => {
            format!("{} = <u32 as Decode>::decode(&mut msg_buf)? as usize;", out_var_name)
        },

        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Struct { fields }), _) => {
            let mut out = String::new();
            for (field_ty, field_name) in fields {
                out.push_str(&write_deserialize_response_into(
                    field_ty,
                    registry,
                    var_name_assign,
                    &format!("{}.r#{}", out_var_name, field_name),
                    force_write
                ));
                out.push_str("\r");
            }
            out
        }

        (parse::VkType::Ident(ty_name), Some(parse::VkTypeDef::Union { fields }), _) => {
            // TODO: implement?
            String::new()
        }

        (parse::VkType::MutPointer(ty_name, _), _, _) |
        (parse::VkType::ConstPointer(ty_name, _), _, _) if print_ty(ty).contains("void") => {
            // TODO: what to do here?
            String::new()
        }

        (parse::VkType::ConstPointer(ty_name, parse::VkTypePtrLen::One), _, _) => {
            write_deserialize_response_into(ty_name, registry, var_name_assign, &format!("(*{})", out_var_name), force_write)
        }

        // TODO: is that implemented?
        /*(parse::VkType::ConstPointer(ty_name, len), _) => {
            // TODO: not implemented for non-null-terminated
            if let parse::VkTypePtrLen::NullTerminated = len {
            } else {
                return format!("mem::zeroed::<{}>()", print_ty(ty))
            };
    
            // Pointers, when serialized, always start with the number of elements.
            let len_var = interm_step_gen(format!("/* len({}) */ <u32 as Decode>::decode(&mut msg_buf)? as usize", print_ty(ty_name)), false);

            let interm = {
                let inner = write_deserialize(&ty_name, registry, &mut |f, mutable| {
                    let var = interm_step_gen(format!("{{ \
                        let mut list = Vec::with_capacity({len}); \
                        for _n in 0..{len} {{ list.push({inner}); }} \
                        list \
                    }}", inner=f, len=len_var), mutable);
                    format!("{}[_n]", var)
                });

                let opt_null_delim = if let parse::VkTypePtrLen::NullTerminated = len {
                    format!("list.push(0);")
                } else {
                    String::new()
                };

                format!("{{ \
                    let mut list = Vec::with_capacity({len}); \
                    for _n in 0..{len} {{ list.push({inner}); }} \
                    {opt_null_delim} \
                    list \
                }}", inner=inner, len=len_var, opt_null_delim=opt_null_delim)
            };

            let var = interm_step_gen(interm, false);
            format!("if !{var}.is_empty() {{ {var}.as_ptr() }} else {{ ptr::null() }}", var=var)
        }*/

        (parse::VkType::Array(ty_name, len), _, _) => {
            let iter_var = var_name_assign();
            let inner = write_deserialize_response_into(ty_name, registry, var_name_assign, &format!("{}[{}]", out_var_name, iter_var), force_write);
            if inner.is_empty() {
                String::new()
            } else {
                format!("for {} in 0..{} {{ {} }}", iter_var, len, inner)
            }
        }

        (parse::VkType::MutPointer(ty_name, parse::VkTypePtrLen::NullTerminated), _, _) => {
            // Passing a pointer to null-terminated buffer whose content is uninitialized doesn't
            // make sense. If this path is reached, there is either a mistake in the Vulkan API
            // definition, or a new way to call functions has been introduced.
            panic!()
        }

        (parse::VkType::MutPointer(ty_name, parse::VkTypePtrLen::One), _, _) |
        (parse::VkType::MutPointer(ty_name, parse::VkTypePtrLen::OtherField { .. }), _, _) => {
            let iter_var = var_name_assign();
            let inner = write_deserialize_response_into(ty_name, registry, var_name_assign, &format!("(*({}.offset({} as isize)))", out_var_name, iter_var), true);
            format!("{{ \
                let len = <u32 as Decode>::decode(&mut msg_buf)? as usize; \
                for {} in 0..len {{ \
                    {} \
                }} \
            }}", iter_var, inner)
        }

        _ => String::new()
    }
}

fn write_get_instance_proc_addr(mut out: impl Write, registry: &parse::VkRegistry) {
    writeln!(out, "unsafe extern \"system\" fn wrapper_vkGetInstanceProcAddr(_instance: usize, name: *const u8) -> PFN_vkVoidFunction {{").unwrap();
    writeln!(out, "    #![allow(non_snake_case)]").unwrap();
    writeln!(out, "    let name = match CStr::from_ptr(name as *const _).to_str() {{").unwrap();
    writeln!(out, "        Ok(n) => n,").unwrap();
    writeln!(out, "        Err(_) => return mem::transmute(ptr::null::<c_void>())").unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(out, "").unwrap();
    writeln!(out, "    match name {{").unwrap();

    for command in &registry.commands {
        if command.name == "vkGetInstanceProcAddr" {
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
        writeln!(out, "            let ptr = wrapper_{} as unsafe extern \"system\" fn({}) -> {};", command.name, params_tys, print_ty(&command.ret_ty)).unwrap();
        writeln!(out, "            mem::transmute::<_, PFN_vkVoidFunction>(ptr)").unwrap();
        writeln!(out, "        }}").unwrap();
    }

    writeln!(out, "        _ => mem::transmute(ptr::null::<c_void>())").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out, "").unwrap();
    writeln!(out, "unsafe extern \"system\" fn wrapper_vkGetDeviceProcAddr(_device: usize, name: *const u8) -> PFN_vkVoidFunction {{").unwrap();
    writeln!(out, "    wrapper_vkGetInstanceProcAddr(0, name)").unwrap();       // TODO: do more properly?
    writeln!(out, "}}").unwrap();
    writeln!(out, "").unwrap();
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
