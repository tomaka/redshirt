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

use criterion::{criterion_group, criterion_main, Criterion};
use redshirt_core::{extrinsics::wasi::WasiExtrinsics, Module, SystemBuilder, SystemRunOutcome};

fn bench(c: &mut Criterion) {
    /* Original code:
    #![feature(alloc_error_handler)]
    #![no_std]
    #![no_main]

    #[global_allocator]
    static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

    #[cfg(not(any(test, doc, doctest)))]
    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo) -> ! {
        unsafe { core::hint::unreachable_unchecked() }
    }

    #[cfg(not(any(test, doc, doctest)))]
    #[alloc_error_handler]
    fn alloc_error_handler(_: core::alloc::Layout) -> ! {
        panic!()
    }

    extern crate alloc;
    use alloc::vec;
    use futures::prelude::*;
    use tiny_keccak::*;

    #[no_mangle]
    fn _start() {
        let data = [254u8; 4096];

        let mut res: [u8; 32] = [0; 32];
        let mut keccak = tiny_keccak::Keccak::v256();
        keccak.update(&data);
        keccak.finalize(&mut res);

        assert_ne!(res[0] as isize, 0);
    }
    */
    let module = Module::from_bytes(&include_bytes!("keccak.wasm")[..]).unwrap();

    c.bench_function("keccak-4096-bytes", |b| {
        let system = SystemBuilder::new(WasiExtrinsics::default())
            .build()
            .unwrap();
        b.iter(|| {
            system.execute(&module).unwrap();
            futures::executor::block_on(async {
                loop {
                    match system.run().await {
                        SystemRunOutcome::ProgramFinished { outcome, .. } => break outcome,
                        _ => panic!(),
                    }
                }
            })
            .unwrap();
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
