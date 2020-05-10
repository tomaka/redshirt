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


#[repr(C, align(16))]
pub struct EndpointDescriptor([u8; 16]);

#[derive(Debug)]
pub struct EndpointControlDecoded {
    /// Maximum number of bytes that can be sent or received in a single data packet. Only used
    /// when the direction is `OUT` or `SETUP`. Must be inferior or equal to 4095.
    pub maximum_packet_size: u16,
    /// If true, isochronous TD format. If false, general TD format.
    pub format: bool,
    /// When set, the HC continues on the next ED off the list without accessing this one.
    pub skip: bool,
    /// If false, full speed. If true, low speed.
    pub low_speed: bool,
    /// Direction of the data flow.
    pub direction: Direction,
    /// Value between 0 and 16. The USB address of the endpoint within the function.
    pub endpoint_number: u8,
    /// Value between 0 and 128. The USB address of the function containing the endpoint.
    pub function_address: u8,
}

impl EndpointControlDecoded {
    pub fn encode(&self) -> [u8; 4] {
        assert!(self.maximum_packet_size < (1 << 12));
        assert!(self.endpoint_number < (1 << 5));
        assert!(self.function_address < (1 << 8));

        let direction = match self.direction {
            Direction::In => 0b10,
            Direction::Out => 0b01,
            Direction::FromTd => 0b00,
        };

        let val = u32::from(self.maximum_packet_size) << 16 |
            if self.format { 1 } else { 0 } << 15 |
            if self.skip { 1 } else { 0 } << 14 |
            if self.low_speed { 1 } else { 0 } << 13 |
            direction << 11 |
            u32::from(self.endpoint_number) << 7 |
            u32::from(self.function_address);

        val.to_be_bytes()
    }
}

#[derive(Debug)]
pub enum Direction {
    In,
    Out,
    FromTd,
}
