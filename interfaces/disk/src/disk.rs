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

//! Registering disks.
//!
//! This module allows you to register your disk. Reading and writing commands can then be issued
//! towards this disk.
//!
//! Use this if you're writing for example a networking driver or a VPN.
//!
//! # Usage
//!
//! - Call [`register_disk`] in order to notify of the existence of a disk.
//! - You obtain a [`DiskRegistration`] that you can use to obtain the commands that need to be
//!   executed by the disk.
//! - Dropping the [`DiskRegistration`] unregisters the disk.
//!

use crate::ffi;
use core::fmt;
use futures::{lock::Mutex, prelude::*};
use redshirt_syscalls::{Decode as _, Encode as _, EncodedMessage};

/// Configuration of an interface to register.
#[derive(Debug)]
pub struct DiskConfig {
    /// `True` if the disk accepts write commands. `False` is the disk can only be read, for
    /// example for CD-ROMs.
    pub allow_write: bool,

    /// Size, in bytes, of a sector.
    pub sector_size: u32,

    /// Number of sectors on the disk.
    pub num_sectors: u32,
}

/// Registers a new disk.
pub async fn register_disk(config: DiskConfig) -> DiskRegistration {
    unsafe {
        let id = rand::random();

        redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &{
            ffi::DiskMessage::RegisterDisk {
                id,
                allow_write: config.allow_write,
                sector_size: config.sector_size,
                num_sectors: config.num_sectors,
            }
        })
        .unwrap();

        DiskRegistration {
            id,
            commands: Mutex::new((0..10).map(|_| build_commands_future(id)).collect()),
        }
    }
}

/// Registered disk.
///
/// Destroying this object will unregister the interface.
pub struct DiskRegistration {
    /// Identifier of the interface in the disks manager.
    id: u64,

    /// Futures that will resolve once we receive a command that the disk must execute.
    commands: Mutex<stream::FuturesOrdered<redshirt_syscalls::MessageResponseFuture<Vec<u8>>>>,
}

/// Build a `Future` resolving to the next command to execute by the disk.
fn build_commands_future(disk_id: u64) -> redshirt_syscalls::MessageResponseFuture<Vec<u8>> {
    unsafe {
        let message = ffi::DiskMessage::DiskNextCommand(disk_id).encode();
        let msg_id = redshirt_syscalls::MessageBuilder::new()
            .add_data(&message)
            .emit_with_response_raw(&ffi::INTERFACE)
            .unwrap();
        redshirt_syscalls::message_response(msg_id)
    }
}

impl DiskRegistration {
    /// Returns the next command that the disk must execute.
    ///
    /// > **Note**: It is possible to call this method multiple times on the same
    /// >           [`DiskRegistration`]. If that is done, no guarantee exists as to which
    /// >           `Future` finishes first.
    pub async fn next_command(&self) -> Command {
        let mut commands = self.commands.lock().await;
        let data = commands.next().await.unwrap();
        commands.push(build_commands_future(self.id));
        // TODO: extra copy when decoding :-/
        let decoded = ffi::DiskCommand::decode(EncodedMessage(data)).unwrap();
        match decoded {
            ffi::DiskCommand::StartRead {
                id,
                sector_lba,
                num_sectors,
            } => Command::Read(ReadCommand {
                id,
                sector_lba,
                num_sectors,
            }),
            ffi::DiskCommand::StartWrite {
                id,
                sector_lba,
                data,
            } => Command::Write(WriteCommand {
                id,
                sector_lba,
                data,
            }),
        }
    }
}

impl fmt::Debug for DiskRegistration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("DiskRegistration").field(&self.id).finish()
    }
}

impl Drop for DiskRegistration {
    fn drop(&mut self) {
        unsafe {
            let message = ffi::DiskMessage::UnregisterDisk(self.id);
            redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &message).unwrap();
        }
    }
}

/// Command received from the disks manager.
pub enum Command {
    /// See [`ReadCommand`].
    Read(ReadCommand),
    /// See [`WriteCommand`].
    Write(WriteCommand),
}

/// Read command received from the disks manager. The registerer must read data from the disk.
pub struct ReadCommand {
    id: ffi::ReadId,
    sector_lba: u64,
    num_sectors: u32,
}

impl ReadCommand {
    /// Returns the first sector to read from.
    pub fn sector_lba(&self) -> u64 {
        self.sector_lba
    }

    /// Returns the number of sectors to read.
    pub fn num_sectors(&self) -> u32 {
        self.num_sectors
    }

    /// Report that the read has finished. Contains the data read from the disk.
    pub fn report_finished(self, data: Vec<u8>) {
        unsafe {
            let message = ffi::DiskMessage::ReadFinished(self.id, data);
            redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &message).unwrap();
        }
    }
}

/// Write command received from the disks manager. The registerer must write data to the disk.
pub struct WriteCommand {
    id: ffi::WriteId,
    sector_lba: u64,
    data: Vec<u8>,
}

impl WriteCommand {
    /// Data to write to the disk.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Returns the first sector to write to.
    pub fn sector_lba(&self) -> u64 {
        self.sector_lba
    }

    /// Report that the write has finished.
    pub fn report_finished(self) {
        unsafe {
            let message = ffi::DiskMessage::WriteFinished(self.id);
            redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &message).unwrap();
        }
    }
}
