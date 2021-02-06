// Copyright (C) 2019-2021  Pierre Krieger
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

use super::{ProcessesCollectionBuilder, RunFutureOut, RunOneOutcome};
use crate::sig;

use futures::prelude::*;
use hashbrown::HashSet;
use std::{
    sync::{Arc, Barrier, Mutex},
    thread,
};

#[test]
#[should_panic]
fn panic_duplicate_extrinsic() {
    ProcessesCollectionBuilder::<()>::with_seed([0; 32])
        .with_extrinsic("foo", "test", sig!(()), ())
        .with_extrinsic("foo", "test", sig!(()), ());
}

#[test]
fn basic() {
    let module = from_wat!(
        local,
        r#"(module
        (func $_start (result i32)
            i32.const 5)
        (export "_start" (func $_start)))
    "#
    );
    let processes = ProcessesCollectionBuilder::<()>::with_seed([0; 32]).build();
    processes.execute(&module, (), ()).unwrap();
    loop {
        let outcome = match futures::executor::block_on(processes.run()) {
            RunFutureOut::Direct(v) => v,
            RunFutureOut::ReadyToRun(rtr) => rtr.run(),
        };
        match outcome {
            RunOneOutcome::StartProcessAbort { .. } => {}
            RunOneOutcome::ProcessFinished { outcome, .. } => {
                assert!(matches!(outcome.unwrap(), Some(crate::WasmValue::I32(5))));
                break;
            }
            _ => panic!(),
        };
    }
}

#[test]
fn aborting_works() {
    let module = from_wat!(
        local,
        r#"(module
        (func $_start (result i32)
            i32.const 5)
        (export "_start" (func $_start)))
    "#
    );
    let processes = ProcessesCollectionBuilder::<()>::with_seed([0; 32]).build();
    processes.execute(&module, (), ()).unwrap().0.abort();
    let outcome = match futures::executor::block_on(processes.run()) {
        RunFutureOut::Direct(v) => v,
        RunFutureOut::ReadyToRun(rtr) => rtr.run(),
    };
    match outcome {
        RunOneOutcome::ProcessFinished {
            outcome: Err(_), ..
        } => {}
        _ => panic!(),
    };
}

#[test]
fn many_processes() {
    let module = from_wat!(
        local,
        r#"(module
        (import "" "test" (func $test (result i32)))
        (func $_start (result i32)
            call $test)
        (export "_start" (func $_start)))
    "#
    );
    let num_processes = 10000;
    let num_threads = 8;

    let processes = Arc::new(
        ProcessesCollectionBuilder::<i32>::with_seed([0; 32])
            .with_extrinsic("", "test", sig!(() -> I32), 98)
            .build(),
    );
    let mut spawned_pids = HashSet::<_, fnv::FnvBuildHasher>::default();
    for _ in 0..num_processes {
        let pid = processes.execute(&module, (), ()).unwrap().0.pid();
        assert!(spawned_pids.insert(pid));
    }

    let finished_pids = Arc::new(Mutex::new(HashSet::<_, fnv::FnvBuildHasher>::default()));
    let start_barrier = Arc::new(Barrier::new(num_threads));
    let end_barrier = Arc::new(Barrier::new(num_threads + 1));

    for _ in 0..num_threads {
        let processes = processes.clone();
        let finished_pids = finished_pids.clone();
        let start_barrier = start_barrier.clone();
        let end_barrier = end_barrier.clone();
        thread::spawn(move || {
            start_barrier.wait();

            let mut local_finished = Vec::with_capacity(num_processes);
            loop {
                let outcome = match processes.run().now_or_never() {
                    Some(RunFutureOut::Direct(v)) => v,
                    Some(RunFutureOut::ReadyToRun(rtr)) => rtr.run(),
                    None => break,
                };
                match outcome {
                    RunOneOutcome::ProcessFinished { pid, outcome, .. } => {
                        assert!(matches!(
                            outcome.unwrap(),
                            Some(crate::WasmValue::I32(1234))
                        ));
                        local_finished.push(pid);
                    }
                    RunOneOutcome::Interrupted {
                        thread, id: &98, ..
                    } => {
                        thread.resume(Some(crate::WasmValue::I32(1234)));
                    }
                    RunOneOutcome::StartProcessAbort { .. } => {}
                    _ => panic!(),
                };
            }

            {
                let mut finished_pids = finished_pids.lock().unwrap();
                for local in local_finished {
                    assert!(finished_pids.insert(local));
                }
            }

            end_barrier.wait();
        });
    }

    end_barrier.wait();
    for pid in finished_pids.lock().unwrap().drain() {
        assert!(spawned_pids.remove(&pid));
    }
    assert!(spawned_pids.is_empty());
}

// TODO: add fuzzing here
