Simpleboot
==========

[Simpleboot](https://gitlab.com/bztsrc/simpleboot) is an all-in-one OS loader and bootable disk image creator that can load Linux
kernels and Multiboot2 compliant kernels in ELF and PE formats.

NOTE: This is a boot *loader*, its job is to load a single kernel. If you're looking for a boot *manager*, which is capable to
boot multiple kernels and has an interactive menu as well, then take a look at **Simpleboot**'s big brother,
[Easyboot](https://gitlab.com/bztsrc/easyboot).

 *"Perfection is achieved, not when there is nothing more to add, but when there is nothing left to take away."*

 *Antoine de Saint-Exupéry*

This loader is a single file, as small as 58k, and yet it supports multiple firmware (like BIOS, UEFI), multiple file formats (ELF,
PE), multiple ABIs (SysV, fastcall, CDECL), multiple architectures (i386, x86_64; there's also an AArch64 variant for Raspberry Pi),
multiple boot protocols (Linux boot, Multiboot2, capable to fallback to FreeBSD or BIOS boot partitions), even transparently
uncompresses your payloads and generates ACPI tables on platforms that do not support it natively. It also supports SMP and able to
run the kernel on all CPU cores at once. It's probably the winner by far in "the most features per byte in a boot loader" category.
And yes, you can relax, it can also display a custom boot logo for you.

Rationale
---------

I was wondering what could be the bare minimum and simplest usable implementation of booting a kernel, because let's face it, GRUB
and the others sucks big time. They're very overcomplicated, bloated, hard to install and very easy to fuck up their config syntax.
And all the helpers (like grub-mkrescue or syslinux) have an outrageous number of dependencies, each with its own unnecessary
hazard of version incompatibility or possibly missing commands. To be honest, I never understood why creating a disk image file has
to be more complicated than creating a zip or tar archive for example.

There's definitely a need for a much simpler, much easier to use and much more user friendlier solution than GRUB. The typical
usecase is OS installer image creation, 99% of end-user computers with just one OS, and especially hobby OS development. In all
these cases there's absolutely no need for a complicated and interactive boot manager, because there's only one kernel to boot, and
with the last case it is very important that you should be able to rapidly and quickly create and test run new images without a
fuzz. So just because I can, I've created such a [suckless](https://suckless.org) tool, here's my solution:

1. create a directory and put your files in it, among other things your kernel binary
2. execute the dependency-free `simpleboot (source directory) (output image file)` command
3. and... that's about it... nothing else left to do! The image *Just Works (TM)*, it will get your kernel booted!

You can install the loader and make an existing device or image bootable; or you can create a bootable image anew. You can boot
that image in a VM, or you can write it with `dd` or [USBImager](https://bztsrc.gitlab.io/usbimager/) to a storage and boot that
on a real machine too.

Simplicity is the ultimate sophistication! The name **Simpleboot** is a pun on Multiboot because it uses a saner, simpler (yet
compatible) subset of the boot protocol and has a much simpler tool usage.

NOTE: The Multiboot2 protocol is just too dumb, so we violate the protocol a bit to support higher-half kernels, SMP (multicore)
and clean 64-bit long mode entry point if the kernel's format is 64-bit.

Installation
------------

Just download the binary for your OS. These are portable executables, they don't require installation and they don't need any
shared libraries / DLLs.

- [simpleboot](https://gitlab.com/bztsrc/simpleboot/-/raw/main/distrib/simpleboot) Linux, \*BSD (167k)
- [simpleboot.exe](https://gitlab.com/bztsrc/simpleboot/-/raw/main/distrib/simpleboot.exe) Windows (169k)

Furthermore you can find various packaging solutions in the [distrib](distrib) directory (for Debian, Ubuntu, RaspiOS, Gentoo,
Arch, qemu coreboot BIOS ROM).

Documentation
-------------

The detailed [documentation](docs) on how to use the bootable disk creator and how a kernel is booted can be found in the docs
directory. It includes all the relevant parts of the Multiboot2 specification too (a fixed version that matches what's actually
in GRUB's multiboot2.h header, plus Simpleboot's additions).

Example Kernels
---------------

In the [example](example) directory, you can find very simple "kernels", these just dump the received boot parameters to the serial
console. The [kernel.c](example/kernel.c) is more or less the same as the example kernel in the Multiboot2 specification (just uses
the `simpleboot.h` header with typedefs and has serial output instead of VGA text mode). Note that *NO ASSEMBLY* prologue nor any
special embedded data required with **Simpleboot**. The [linux.c](example/linux.c) is just a Linux kernel mock up for testing the
boot parameters passing.

Compilation
-----------

GNU/make needed for orchestration (although it's literally just `cc simpleboot.c -o simpleboot`). The toolchain doesn't matter,
any ANSI C compiler will do, works on POSIX and WIN32 MINGW too. Just go to the [src](src) directory and run `make`. That's all.
Despite of it's small size, it is self-contained and has exactly zero library dependencies. It's not called **Simpleboot** for
nothing :-)

To recompile the "boot_x86.bin" sector, you'll need the [flatassembler](https://flatassembler.net), and for the "loader_x86.efi"
and "loader_rpi.bin" you'll have to install LLVM Clang and lld (gcc and GNU ld won't work I'm afraid). But don't worry, I've
added all three to `src/data.h` as a byte array, so you don't have to compile these unless you really really want to (for that,
just delete data.h before you run make).

You can also compile **Simpleboot** as a [coreboot](https://coreboot.org/) payload. See the comments at the beginning of
[src/loader_cb.c](src/loader_cb.c) for build instructions and the [documentation](docs/coreboot.md) for more information.

License
-------

Licensed under the permissive terms of the MIT license, see [LICENSE](LICENSE) file for details. Do as you please. Not required,
but attribution appreciated.

Contributors
------------

I'd like to say thanks to Zahy for valuable feedback on FreeBSD. Special thanks to dzsolt for providing the ebuild.

Social
------

* [Hungarian UNIX Portal](https://hup.hu/node/182370) (original announcement)
* [Flatassembler forum](https://board.flatassembler.net/topic.php?t=22876) (about the legacy boot sector)
* [Raspberry Pi forum](https://forums.raspberrypi.com/viewtopic.php?p=2122434) (about the RPi port)
* [OSDEV forum](https://forum.osdev.org/viewtopic.php?f=2&t=56905) (about writing kernels)

Sorry, I don't use facepalm, twitees, masturbadon, vector, discoss and alike. I don't use anything that forces shady JS on users
(and that's why this repo isn't stored on github, btw).

Cheers,

bzt
