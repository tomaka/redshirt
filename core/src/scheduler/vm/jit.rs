// Copyright (C) 2019-2020  Pierre Krieger
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

// TODO: everything here is unsafe and prototipal

use super::{ExecOutcome, NewErr, RunErr, StartErr};
use crate::module::Module;

use alloc::{
    borrow::{Cow, ToOwned as _},
    boxed::Box,
    format,
    vec::Vec,
};
use core::{cell::RefCell, convert::TryInto, fmt, iter};
use smallvec::SmallVec;

mod coroutine;

pub struct Jit<T> {
    instance: wasmtime::Instance,

    /// Stack to use when invoking methods in the WASM VM.
    exec_stack: Box<[u8]>,

    memory: Option<wasmi::MemoryRef>,
    indirect_table: Option<wasmi::TableRef>,

    /// We only support one thread. That's its user data.
    thread_user_data: T,

    /// If true, the state machine is in a poisoned state and cannot run any code anymore.
    is_poisoned: bool,
}

/// Access to a thread within the virtual machine.
pub struct Thread<'a, T> {
    /// Reference to the parent object.
    vm: &'a mut Jit<T>,
}

impl<T> Jit<T> {
    /// Creates a new process state machine from the given module.
    ///
    /// The closure is called for each import that the module has. It must assign a number to each
    /// import, or return an error if the import can't be resolved. When the VM calls one of these
    /// functions, this number will be returned back in order for the user to know how to handle
    /// the call.
    ///
    /// A single main thread (whose user data is passed by parameter) is automatically created and
    /// is paused at the start of the "_start" function of the module.
    pub fn new(
        module: &Module,
        main_thread_user_data: T,
        mut symbols: impl FnMut(&str, &str, &wasmi::Signature) -> Result<usize, ()>,
    ) -> Result<Self, NewErr> {
        let engine = wasmtime::Engine::new(&Default::default());
        let store = wasmtime::Store::new(&engine);
        let module = wasmtime::Module::from_binary(&store, module.as_ref()).unwrap();

        /*for import in module.imports() {
            match import.ty() {
                wasmtime::ExternType::Func(f) => {
                    symbols(f.module(), f.name(), )
                }
                wasmtime::ExternType::Global(_) => unimplemented!(),
                wasmtime::ExternType::Table(_) => unimplemented!(),
                wasmtime::ExternType::Memory(_) => unimplemented!(),
            }
        }*/

        unimplemented!()
    }

    /// Returns true if the state machine is in a poisoned state and cannot run anymore.
    pub fn is_poisoned(&self) -> bool {
        self.is_poisoned
    }

    pub fn start_thread_by_id(
        &mut self,
        _: u32,
        _: impl Into<Cow<'static, [wasmi::RuntimeValue]>>,
        _: T,
    ) -> Result<Thread<T>, StartErr> {
        unimplemented!()
    }

    /// Returns the number of threads that are running.
    pub fn num_threads(&self) -> usize {
        1
    }

    pub fn thread(&mut self, index: usize) -> Option<Thread<T>> {
        if index == 0 {
            Some(Thread { vm: self })
        } else {
            None
        }
    }

    pub fn into_user_datas(self) -> impl ExactSizeIterator<Item = T> {
        iter::once(self.thread_user_data)
    }

    /// Copies the given memory range into a `Vec<u8>`.
    ///
    /// Returns an error if the range is invalid or out of range.
    pub fn read_memory(&self, offset: u32, size: u32) -> Result<Vec<u8>, ()> {
        let mem = match self.memory.as_ref() {
            Some(m) => m,
            None => unreachable!(),
        };

        mem.get(offset, size.try_into().map_err(|_| ())?)
            .map_err(|_| ())
    }

    /// Write the data at the given memory location.
    ///
    /// Returns an error if the range is invalid or out of range.
    pub fn write_memory(&mut self, offset: u32, value: &[u8]) -> Result<(), ()> {
        let mem = match self.memory.as_ref() {
            Some(m) => m,
            None => unreachable!(),
        };

        mem.set(offset, value).map_err(|_| ())
    }
}

unsafe impl<T: Send> Send for Jit<T> {}

impl<T> fmt::Debug for Jit<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Jit").finish()
    }
}

impl<'a, T> Thread<'a, T> {
    /// Starts or continues execution of this thread.
    ///
    /// If this is the first call you call [`run`](Thread::run) for this thread, then you must pass
    /// a value of `None`.
    /// If, however, you call this function after a previous call to [`run`](Thread::run) that was
    /// interrupted by an external function call, then you must pass back the outcome of that call.
    pub fn run(mut self, value: Option<wasmi::RuntimeValue>) -> Result<ExecOutcome<'a, T>, RunErr> {
        unimplemented!()
    }

    /// Returns the index of the thread, so that you can retreive the thread later by calling
    /// [`Jit::thread`].
    ///
    /// Keep in mind that when a thread finishes, all the indices above its index shift by one.
    pub fn index(&self) -> usize {
        0
    }

    /// Returns the user data associated to that thread.
    pub fn user_data(&mut self) -> &mut T {
        &mut self.vm.thread_user_data
    }

    /// Turns this thread into the user data associated to it.
    pub fn into_user_data(self) -> &'a mut T {
        &mut self.vm.thread_user_data
    }
}

impl<'a, T> fmt::Debug for Thread<'a, T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Thread")
            .field(&self.vm.thread_user_data)
            .finish()
    }
}
