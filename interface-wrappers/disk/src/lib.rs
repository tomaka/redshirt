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

//! Disk registration and commands issuance.
//!
//! # Overview
//!
//! This interface allows indicating the presence of a disk on the machine by registering it. The
//! registered disk can then receive commands (such as reading and writing data) that it must
//! execute.
//!
//! The word "disk" should be understood as in a hard disk drive (HDD), solid-state drive (SSD),
//! CD-ROM drive, floppy disk drive, and anything similar.
//!
//! It is recommended to **not** register disks that aren't actual physical objects connected to
//! the machine (such as disks accessed over the network, or equivalents to UNIX's `losetup`).
//! Redshirt tries to be as explicit as possible and to keep its abstractions as close as possible
//! to the objects being abstracted.
//!
//! Users of this interface are expected to be ATA/ATAPI drivers, USB drivers, and similar. The
//! handler of this interface is expected to be a filesystem manager that determines the list of
//! partitions and handles the file systems.
//!
//! # About disks
//!
//! In the abstraction exposed by this interface, a disk is composed of a certain number of
//! sectors, each having a certain size. The size of all sectors is identical. These sectors are
//! indexed from `0` to `num_sectors - 1`.
//!
//! The sector index is typically referred to by the term "LBA sector". *LBA* designates the fact
//! that each sector is addressed through a index ranging linearly from `0` to `num_sectors - 1`,
//! as opposed to the older *CHS* addressing where sectors were referred to a by a head number,
//! cylinder number, and sectors offset. CHS addressing isn't used in this interface.
//!
//! It is not possible to partially read or write a sector. The sector has to be read entirely,
//! or written entirely.
//!
//! # Flushing
//!
//! The interface requires each disk write to be confirmed by sending a message on the interface
//! after it has been performed. This is important in order for the upper layer to be capable of
//! handling problematic situations such as a power outage.
//!
//! Consequently, while the disk driver is allowed to maintain a write cache, it must not report
//! a write success after the data has been put in cache, but after it has been written to disk.
//!
//! In the interval of time between the moment the writing is issued and the moment it is
//! confirmed, the upper layers should consider that the sector is physically in an undefined
//! state.

pub mod disk;
pub mod ffi;
