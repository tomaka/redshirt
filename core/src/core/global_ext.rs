
/// Externals that are always available, wherever we are.
pub struct GlobalExternals {

}

#[repr(u32)]
enum Indices {
    Yield = 0,
}

impl wasmi::ImportResolver for GlobalExternals {
    fn resolve_func(&self, module_name: &str, field_name: &str, signature: &wasmi::Signature)
        -> Result<wasmi::FuncRef, wasmi::Error>
    {
        let index = {

        };

        /*let key = (
            module_name.as_bytes().to_owned(),
            field_name.as_bytes().to_owned(),
        );
        let externval = self.map.get(&key).ok_or_else(|| {*/
            //Err(wasmi::Error::Instantiation(format!("Export {}:{} not found", module_name, field_name)))
        /*})?;
        let host_func_idx = match *externval {
            ExternVal::HostFunc(ref idx) => idx,
            _ => {
                return Err(wasmi::Error::Instantiation(format!(
                    "Export {}:{} is not a host func",
                    module_name, field_name
                )))
            }
        };
        Ok(FuncInstance::alloc_host(signature.clone(), host_func_idx.0))*/
        Ok(wasmi::FuncInstance::alloc_host(signature.clone(), 0))
    }

    fn resolve_global(&self, _module_name: &str, _field_name: &str, _global_type: &wasmi::GlobalDescriptor)
        -> Result<wasmi::GlobalRef, wasmi::Error>
    {
        Err(wasmi::Error::Instantiation(format!(
            "Importing globals is not supported yet"
        )))
    }

    fn resolve_memory(&self, module_name: &str, field_name: &str, _memory_type: &wasmi::MemoryDescriptor)
        -> Result<wasmi::MemoryRef, wasmi::Error>
    {
        Err(wasmi::Error::Instantiation(
            format!("Export {}:{} not found", module_name, field_name)
        ))
    }

    fn resolve_table(&self, _module_name: &str, _field_name: &str, _table_type: &wasmi::TableDescriptor)
        -> Result<wasmi::TableRef, wasmi::Error>
    {
        Err(wasmi::Error::Instantiation(format!(
            "Importing tables is not supported yet"
        )))
    }
}

impl wasmi::Externals for GlobalExternals {
    fn invoke_index(&mut self, index: usize, _args: wasmi::RuntimeArgs)
        -> Result<Option<wasmi::RuntimeValue>, wasmi::Trap>
    {
        // TODO: check that index is valid
        Err(wasmi::TrapKind::Host(Box::new(Interrupt { index })).into())
    }
}

/// Dummy value that we return whenever some WASM code tries to invoke one of the externals. This
/// makes it possible to yield back execution to the host.
#[derive(Debug)]
struct Interrupt {
    index: usize,
}

impl fmt::Display for Interrupt {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Interrupt({})", self.index)
    }
}

impl wasmi::HostError for Interrupt { }
