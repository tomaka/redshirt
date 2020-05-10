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

//! Struct definitions and constants, as translated from the specs.

pub const HC_REVISION_OFFSET: u64 = 0x0;
pub const HC_CONTROL_OFFSET: u64 = 0x4;
pub const HC_COMMAND_STATUS_OFFSET: u64 = 0x8;
pub const HC_INTERRUPT_STATUS_OFFSET: u64 = 0xc;
pub const HC_INTERRUPT_ENABLE_OFFSET: u64 = 0x10;
pub const HC_INTERRUPT_DISABLE_OFFSET: u64 = 0x14;
pub const HC_HCCA_OFFSET: u64 = 0x18;
pub const HC_PERIOD_CURRENT_ED_OFFSET: u64 = 0x1c;
pub const HC_CONTROL_HEAD_ED_OFFSET: u64 = 0x20;
pub const HC_CONTROL_CURRENT_ED_OFFSET: u64 = 0x24;
pub const HC_BULK_HEAD_ED_OFFSET: u64 = 0x28;
pub const HC_BULK_CURRENT_ED_OFFSET: u64 = 0x2c;
pub const HC_DONE_HEAD_OFFSET: u64 = 0x30;
pub const HC_FM_INTERVAL_OFFSET: u64 = 0x34;
pub const HC_FM_REMAINING_OFFSET: u64 = 0x38;
pub const HC_FM_NUMBER_OFFSET: u64 = 0x3c;
pub const HC_PERIODIC_START_OFFSET: u64 = 0x40;
pub const HC_LS_THRESHOLD_OFFSET: u64 = 0x44;
pub const HC_RH_DESCRIPTOR_A_OFFSET: u64 = 0x48;
pub const HC_RH_DESCRIPTOR_B_OFFSET: u64 = 0x4c;
pub const HC_RH_STATUS_OFFSET: u64 = 0x50;
/// Register corresponding to the status of port 1. The status of port 2 (if it exists) is at 0x58,
/// the status of port 3 (if it exists) is at 0x5c, and so on.
pub const HC_RH_PORT_STATUS_1_OFFSET: u64 = 0x54;
