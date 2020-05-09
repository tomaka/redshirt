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

//! OHCI device handler.

use crate::HwAccessRef;
use core::convert::TryFrom as _;

mod definitions;

pub struct OhciDevice<TAcc> {
    hardware_access: TAcc,
}

/// Error that can happen during initialization.
#[derive(Debug, derive_more::Display)]
pub enum InitError {
    /// Unrecognized driver revision number.
    ///
    /// > **Note**: This probably indicates that the memory location doesn't correspond to an
    /// >           OHCI implementation, or that there is a bug in the physical memory access
    /// >           mechanism.
    BadRevision(u8),
}

/// Initializes an OHCI device whose registers are memory-mapped at the given location.
pub async unsafe fn init_ohci_device<TAcc>(
    access: TAcc,
    regs_loc: u64,
) -> Result<OhciDevice<TAcc>, InitError>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    // We start by checking whether the revision is one we know.
    let revision = {
        let mut out = [0];
        access
            .read_memory_u32(regs_loc + definitions::HC_REVISION_OFFSET, &mut out)
            .await;
        u8::try_from(out[0] & 0xff).unwrap()
    };
    if revision != 0x10 {
        return Err(InitError::BadRevision(revision));
    }

    // Determine the alignment requirement for the HCCA.
    let req_alignment = {
        // See section 7.2.1. We write all 1s to the HcHCCA register and read the valueback.
        access
            .write_memory_u32(regs_loc + definitions::HC_HCCA_OFFSET, &[0xffffffff])
            .await;
        let mut out = [0];
        access
            .read_memory_u32(regs_loc + definitions::HC_HCCA_OFFSET, &mut out)
            .await;
        // The value of HC_HCCA will be soemthing like `111..11110000`. We count the number of
        // trailing 0s.
        1u64 << out[0].trailing_zeros()
    };
    panic!("{:?}", req_alignment);

    Ok(OhciDevice {
        hardware_access: access,
    })
}

impl<TAcc> OhciDevice<TAcc> where for<'r> &'r TAcc: HwAccessRef<'r> {}
