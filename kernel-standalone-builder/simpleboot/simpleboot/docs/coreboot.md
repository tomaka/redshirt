Simpleboot in ROM
=================

For backward compatiblity, you can create the disk image with the `-r` flag. This will also save `sb_bios.rom` (legacy BIOS
Expansion ROM) and `sb_uefi.rom` (legacy UEFI PCI Option ROM). You can test these in qemu with `-option-rom`, for example:

```
qemu-system-x86_64 -option-rom /path/to/your/sb_bios.rom -drive file=/path/to/your/disk.img,format=raw -serial stdio
```

But the real deal is that you can use **Simpleboot** in a Free and Open Source ROM. That is, neither BIOS nor UEFI (not a
proprietary blob that some company created and may or may not contain backdoors and telemetry). For maximum security and
privacy, you can compile this loader as a payload for the Free and Open Source [coreboot](https://coreboot.org/) firmware.
Another advantage, this boots blazing fast, about 50 times faster than with UEFI, truely within a blink of an eye. See
the comments at the beginning of [src/loader_cb.c](../src/loader_cb.c) for build instructions. Once compiled, you can
[flash that ROM](https://doc.coreboot.org/tutorial/flashing_firmware/index.html) to a real motherboard, or you can try it
out in a virtual machine, eg. in qemu (replace `/path/to/your/disk.img` with your actual disk image file):

```
qemu-system-x86_64 -bios build/coreboot.rom -drive file=/path/to/your/disk.img,format=raw -serial stdio
```

Note that the disk is specified as usual, but there's also a `-bios` parameter which replaces the factory default firmware ROM.

Configuration
-------------

This firmware ROM now contains the loader, but you still need a disk with an operating system to boot. On that disk, you must have
a *GUID Partitioning Table*, with an *EFI System Partition* formatted as FAT32 (no UEFI files needed, it's just the partition's
type is the same as with UEFI for inter-operability). More partitions might co-exists on the disk; you just need a well-recognizable
one (with a well-supported file system) to serve as a boot partition.

There's no boot menu, always the *first internal disk* will be booted, unless you plug in an USB storage before you power on the
machine (for this, you must compile coreboot with USB support). Other removable media (like eg. CDROM) and other obsolete tech not
supported. If by any chance needed, you can still use them with an USB adapter.

### Defaults

By default you should have only two files on the boot partition, a kernel and an initial ramdisk by these names:

- `vmlinuz-linux`
- `initramfs-linux.img`

These are what most (but sadly not all) distributions use and most likely work out-of-the-box. If you press any key (probably
<kbd>Space</kbd>) during boot, then **Simpleboot** will fallback to a backup operating system configuration, which is:

- `vmlinuz-fallback`
- `initramfs-linux-fallback.img`

If these are not found, then the **Simpleboot** default `kernel` is also tried. You cannot change the default kernel name in
coreboot with `-k` when you generate the disk because it's in a separate read-only ROM image file, but see below.

### Variable configuration on disk

To have more control over the boot process and what files to load, you can create [simpleboot.cfg](README.md) on the boot
partition. This works exactly like with the non-ROM loader, except the `framebuffer` directive does nothing. The coreboot
firmware has no means to change the resolution in run-time, that's configured at compile-time using coreboot's `make menuconfig`.

With a configuration file you can use whatever operating system files you'd like, and you can also load any non-Linux, custom
[Multiboot2](ABI.md) compliant kernel with as many modules as you'd like. The same applies to the backup configuration too.

### Permanent configuration in ROM

To make filenames fixed that users can't alter (just the files themselves), you can add the configuration file to the firmware
before you flash it to the ROM.

```
./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/simpleboot.cfg -n simpleboot.cfg
```

**Simpleboot** will only look for the on disk configuration file if not found in the ROM. Likewise you can add a bootsplash logo:

```
./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/logo.tga -n logo.tga
```

There's no need for a `bootsplash` directive in the configuration, it will be displayed automatically. However if `bootsplash` or
`backupbootsplash` is given, then the specified file will be loaded from disk and that logo will be displayed instead of the
embedded one (add the configuration file to the ROM to prevent this).

If you wish, you can add a [device tree blob](https://gitlab.com/bztsrc/gudt) to the ROM too (in GUDT, DSDT (ACPI .aml) or
FDT (.dtb) formats; depending which format your kernel understands):

```
./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/devices.gud -n devices
./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/devices.dtb -n devices
./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/DSDT.aml -n devices
```

Just like the logo, this will only serve as a reasonable default, which you can override in the configuration by loading a device
tree from disk with the `module` or `backupmodule` directive.

Finally, you can also add a default kernel and default initrd too (not just set their default filenames with `-k` and `-i`, but
their actual file content can be embedded in the ROM):

```
./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/kernel -n kernel
./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/initrd -n initrd
```

If exists, these are only used as a very last resort fallback option when absolutely all the other methods have failed.
