// Copyright(c) 2019 Pierre Krieger

//! Utilities concerning the Vulkan function pointers loading.

use crate::{parse, print_ty};
use std::io::{self, Write};

/// Writes to `out` the code of Rust structs that contain Vulkan pointers.
pub fn write_pointers_structs(out: &mut dyn Write, registry: &parse::VkRegistry) -> Result<(), io::Error> {
    write_pointers(out, registry, "StaticPtrs", |cmd| command_ty(cmd) == CommandTy::Static)?;
    writeln!(out, "")?;
    write_pointers(out, registry, "InstancePtrs", |cmd| command_ty(cmd) == CommandTy::Instance)?;
    writeln!(out, "")?;
    write_pointers(out, registry, "DevicePtrs", |cmd| command_ty(cmd) == CommandTy::Device)?;
    Ok(())
}

fn write_pointers(out: &mut dyn Write, registry: &parse::VkRegistry, struct_name: &str, mut filter: impl FnMut(&parse::VkCommand) -> bool) -> Result<(), io::Error> {
    writeln!(out, "struct {} {{", struct_name)?;

    for command in &registry.commands {
        if !filter(command) { continue; }
        write!(out, "    r#{}: Option<extern \"system\" fn(", command.name)?;
        for (param_off, (param_ty, _)) in command.params.iter().enumerate() {
            // TODO: skip device pointers
            if param_off != 0 { write!(out, ", ")?; }
            write!(out, "{}", print_ty(&param_ty))?;
        }
        writeln!(out, ") -> {}>,", print_ty(&command.ret_ty))?;
    }

    writeln!(out, "}}")?;
    writeln!(out, "")?;
    writeln!(out, "impl {} {{", struct_name)?;
    writeln!(out, "    unsafe fn load_with(mut loader: impl FnMut(&std::ffi::CStr) -> PFN_vkVoidFunction) -> Self {{")?;
    for command in &registry.commands {
        if !filter(command) { continue; }
        writeln!(out, "        let r#{n} = loader(std::ffi::CStr::from_bytes_with_nul_unchecked(b\"{n}\\0\"));", n = command.name)?;
    }
    writeln!(out, "        {} {{", struct_name)?;
    for command in &registry.commands {
        if !filter(command) { continue; }
        writeln!(out, "            r#{n}: mem::transmute(r#{n}),", n = command.name)?;
    }
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(())
}

/// Type of a command.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum CommandTy {
    Static,
    /// The command operates on an `Instance` and must be loaded using `vkGetInstanceProcAddr`.
    Instance,
    /// The command operates on a `Device` and must be loaded using `vkGetDeviceProcAddr`.
    Device,
}

/// Determines the type of a command.
pub fn command_ty(command: &parse::VkCommand) -> CommandTy {
    let (first_param_ty, _) = command.params.first().unwrap();
    match first_param_ty {
        parse::VkType::Ident(ident) if ident == "VkInstance" => CommandTy::Instance,
        parse::VkType::Ident(ident) if ident == "VkPhysicalDevice" => CommandTy::Instance,
        parse::VkType::Ident(ident) if ident == "VkDevice" => CommandTy::Device,
        parse::VkType::Ident(ident) if ident == "VkQueue" => CommandTy::Device,
        parse::VkType::Ident(ident) if ident == "VkCommandBuffer" => CommandTy::Device,
        _ => {
            // In order to make sure that this function doesn't silently break if a new type
            // of function is introduced, we hardcode the list of static functions here and panic
            // if there's an unknown one.
            match command.name.as_str() {
                "vkCreateInstance" => {},
                "vkEnumerateInstanceVersion" => {},
                "vkEnumerateInstanceLayerProperties" => {},
                "vkEnumerateInstanceExtensionProperties" => {},
                _ => panic!()
            }

            CommandTy::Static
        },
    }
}
