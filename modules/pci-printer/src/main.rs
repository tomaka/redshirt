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

//! Queries the PCI interface for all the devices on the system, and prints them out.
//!
//! This program is nothing more than a small debugging utility.

use fnv::FnvBuildHasher;
use std::borrow::Cow;

include!(concat!(env!("OUT_DIR"), "/build-pci.rs"));

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    let devices = redshirt_pci_interface::get_pci_devices().await;

    for device in devices {
        let (vendor_name, device_name) =
            match PCI_DEVICES.get(&(device.vendor_id, device.device_id)) {
                Some((v, d)) => (Cow::Borrowed(*v), Cow::Borrowed(*d)),
                None => (
                    Cow::Owned(format!("Unknown <0x{:x}>", device.vendor_id)),
                    Cow::Owned(format!("Unknown <0x{:x}>", device.device_id)),
                ),
            };

        log::info!(
            "PCI device: {} - {}\nDevice class: {} (prog. if. = 0x{:x}, rev. = 0x{:x})",
            vendor_name,
            device_name,
            show_class_code(device.class_code, device.subclass),
            device.prog_if,
            device.revision_id,
        );
    }
}

lazy_static::lazy_static! {
    static ref PCI_DEVICES: hashbrown::HashMap<(u16, u16), (&'static str, &'static str), FnvBuildHasher> = build_pci_info();
}

fn show_class_code(class_code: u8, subclass: u8) -> Cow<'static, str> {
    match (class_code, subclass) {
        (0x00, 0x00) => "Non-VGA-Compatible devices".into(),
        (0x00, 0x01) => "VGA-Compatible Device".into(),

        (0x01, 0x00) => "SCSI Bus Controller".into(),
        (0x01, 0x01) => "IDE Controller".into(),
        (0x01, 0x02) => "Floppy Disk Controller".into(),
        (0x01, 0x03) => "IPI Bus Controller".into(),
        (0x01, 0x04) => "RAID Controller".into(),
        (0x01, 0x05) => "ATA Controller".into(),
        (0x01, 0x06) => "Serial ATA".into(),
        (0x01, 0x07) => "Serial Attached SCSI".into(),
        (0x01, 0x08) => "Non-Volatile Memory Controller".into(),
        (0x01, 0x80) => "Other".into(),

        (0x02, 0x00) => "Ethernet Controller".into(),
        (0x02, 0x01) => "Token Ring Controller".into(),
        (0x02, 0x02) => "FDDI Controller".into(),
        (0x02, 0x03) => "ATM Controller".into(),
        (0x02, 0x04) => "ISDN Controller".into(),
        (0x02, 0x05) => "WorldFip Controller".into(),
        (0x02, 0x06) => "PICMG 2.14 Multi Computing".into(),
        (0x02, 0x07) => "Infiniband Controller".into(),
        (0x02, 0x08) => "Fabric Controller".into(),
        (0x02, 0x80) => "Other".into(),

        (0x03, 0x00) => "VGA Compatible Controller".into(),
        (0x03, 0x01) => "XGA Controller".into(),
        (0x03, 0x02) => "3D Controller (Not VGA-Compatible)".into(),
        (0x03, 0x80) => "Other".into(),

        (0x04, 0x00) => "Multimedia Video Controller".into(),
        (0x04, 0x01) => "Multimedia Audio Controller".into(),
        (0x04, 0x02) => "Computer Telephony Device".into(),
        (0x04, 0x03) => "Audio Device".into(),
        (0x04, 0x80) => "Other".into(),

        (0x05, 0x00) => "RAM Controller".into(),
        (0x05, 0x01) => "Flash Controller".into(),
        (0x05, 0x80) => "Other".into(),

        (0x06, 0x00) => "Host Bridge".into(),
        (0x06, 0x01) => "ISA Bridge".into(),
        (0x06, 0x02) => "EISA Bridge".into(),
        (0x06, 0x03) => "MCA Bridge".into(),
        (0x06, 0x04) => "PCI-to-PCI Bridge".into(),
        (0x06, 0x05) => "PCMCIA Bridge".into(),
        (0x06, 0x06) => "NuBus Bridge".into(),
        (0x06, 0x07) => "CardBus Bridge".into(),
        (0x06, 0x08) => "RACEway Bridge".into(),
        (0x06, 0x09) => "PCI-to-PCI Bridge".into(),
        (0x06, 0x0A) => "InfiniBand-to-PCI Host Bridge".into(),
        (0x06, 0x80) => "Other".into(),

        (0x07, 0x00) => "Serial Controller".into(),
        (0x07, 0x01) => "Parallel Controller".into(),
        (0x07, 0x02) => "Multiport Serial Controller".into(),
        (0x07, 0x03) => "Modem".into(),
        (0x07, 0x04) => "IEEE 488.1/2 (GPIB) Controller".into(),
        (0x07, 0x05) => "Smart Card".into(),
        (0x07, 0x80) => "Other".into(),

        (0x08, 0x00) => "PIC".into(),
        (0x08, 0x01) => "DMA Controller".into(),
        (0x08, 0x02) => "Timer".into(),
        (0x08, 0x03) => "RTC Controller".into(),
        (0x08, 0x04) => "PCI Hot-Plug Controller".into(),
        (0x08, 0x05) => "SD Host controller".into(),
        (0x08, 0x06) => "IOMMU".into(),
        (0x08, 0x80) => "Other".into(),

        (0x09, 0x00) => "Keyboard Controller".into(),
        (0x09, 0x01) => "Digitizer Pen".into(),
        (0x09, 0x02) => "Mouse Controller".into(),
        (0x09, 0x03) => "Scanner Controller".into(),
        (0x09, 0x04) => "Gameport Contro".into(),
        (0x09, 0x80) => "Other".into(),

        (0x0A, 0x00) => "Generic".into(),
        (0x0A, 0x80) => "Other".into(),

        (0x0B, 0x00) => "386".into(),
        (0x0B, 0x01) => "486".into(),
        (0x0B, 0x02) => "Pentium".into(),
        (0x0B, 0x03) => "Pentium Pro".into(),
        (0x0B, 0x10) => "Alpha".into(),
        (0x0B, 0x20) => "PowerPC".into(),
        (0x0B, 0x30) => "MIPS".into(),
        (0x0B, 0x40) => "Co-Processor".into(),
        (0x0B, 0x80) => "Other".into(),

        (0x0C, 0x00) => "FireWire (IEEE 1394) Controller".into(),
        (0x0C, 0x01) => "ACCESS Bus".into(),
        (0x0C, 0x02) => "SSA".into(),
        (0x0C, 0x03) => "USB Controller".into(),
        (0x0C, 0x04) => "Fibre Channel".into(),
        (0x0C, 0x05) => "SMBus".into(),
        (0x0C, 0x06) => "InfiniBand".into(),
        (0x0C, 0x07) => "IPMI Interface".into(),
        (0x0C, 0x08) => "SERCOS Interface (IEC 61491)".into(),
        (0x0C, 0x09) => "CANbus".into(),
        (0x0C, 0x80) => "Other".into(),

        (0x0D, 0x00) => "iRDA Compatible Controller".into(),
        (0x0D, 0x01) => "Consumer IR Controller".into(),
        (0x0D, 0x10) => "RF Controller".into(),
        (0x0D, 0x11) => "Bluetooth Controller".into(),
        (0x0D, 0x12) => "Broadband Controller".into(),
        (0x0D, 0x20) => "Ethernet Controller (802.1a)".into(),
        (0x0D, 0x21) => "Ethernet Controller (802.1b)".into(),
        (0x0D, 0x80) => "Other".into(),

        (0x0E, 0x00) => "I20".into(),

        (0x0F, 0x01) => "Satellite TV Controller".into(),
        (0x0F, 0x02) => "Satellite Audio Controller".into(),
        (0x0F, 0x03) => "Satellite Voice Controller".into(),
        (0x0F, 0x04) => "Satellite Data Controller".into(),

        (0x10, 0x00) => "Network and Computing Encrpytion/Decryption".into(),
        (0x10, 0x10) => "Entertainment Encryption/Decryption".into(),
        (0x10, 0x80) => "Other Encryption/Decryption".into(),

        (0x11, 0x00) => "DPIO Modules".into(),
        (0x11, 0x01) => "Performance Counters".into(),
        (0x11, 0x10) => "Communication Synchronizer".into(),
        (0x11, 0x20) => "Signal Processing Management".into(),
        (0x11, 0x80) => "Other".into(),

        (0x12, _) => "Processing Accelerator".into(),
        (0x13, _) => "Non-Essential Instrumentation".into(),
        (0x40, _) => "Co-Processor".into(),

        _ => format!("unknown ({}-{})", class_code, subclass).into(),
    }
}
