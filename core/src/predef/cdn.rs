// Copyright(c) 2019 Pierre Krieger

//! Pre-defined interface for grabbing content off the network.

use crate::interface::Interface;

pub fn network_interface() -> Interface {
    unimplemented!()
}

pub fn call(instance: &wasmi::ModuleInstance) {
    let ret = instance.invoke_export("foo", &[wasmi::RuntimeValue::I32(5), wasmi::RuntimeValue::I32(3)], &mut wasmi::NopExternals).expect("failed to execute export");
}
