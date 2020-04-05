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

use alloc::vec::Vec;
use core::convert::TryFrom;
use raw_table::RawPageTable;

mod cr3;
mod entry;
mod raw_table;

pub unsafe fn load_identity_mapping() -> Paging {
    let mut pds = Vec::with_capacity(32);

    let mut pdpt = RawPageTable::new();
    for n in 0..32 {
        let pd = RawPageTable::new();
        pdpt[n] = TryFrom::try_from(entry::DecodedPml4ePdptePde {
            present: true,
            read_write: true,
            user: false,
            write_through: false,
            cache_disable: false,
            accessed: false,
            physical_address: pd.address(),
            execute_disable: false,
        })
        .unwrap();
        pds.push(pd);
    }

    for (one_gb, pd) in pds.iter_mut().enumerate() {
        for n in 0..512 {
            pd[n] = TryFrom::try_from(entry::DecodedPde2M {
                present: true,
                read_write: true,
                user: false,
                write_through: false,
                cache_disable: false,
                accessed: false,
                dirty: false,
                global: false,
                attributes_table: false,
                physical_address: one_gb * 1024 * 1024 * 1024 + n * 2 * 1024 * 1024,
                execute_disable: false,
                protection_key: 0,
            })
            .unwrap();
        }
    }

    let mut pml4 = RawPageTable::new();
    pml4[0] = TryFrom::try_from(entry::DecodedPml4ePdptePde {
        present: true,
        read_write: true,
        user: false,
        write_through: false,
        cache_disable: false,
        accessed: false,
        physical_address: pdpt.address(),
        execute_disable: false,
    })
    .unwrap();

    cr3::load_cr3(&pml4, false, false);

    Paging { pml4, pdpt, pds }
}

pub struct Paging {
    pml4: raw_table::RawPageTable,
    pdpt: raw_table::RawPageTable,
    pds: Vec<raw_table::RawPageTable>,
}

impl Paging {}

impl Drop for Paging {
    fn drop(&mut self) {
        // TODO: explain
        panic!();
    }
}
