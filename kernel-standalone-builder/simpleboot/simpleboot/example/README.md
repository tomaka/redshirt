Simpleboot Example Kernels
==========================

This directory contains the source for very minimal kernels, that can be booted with Simpleboot (and
[Easyboot](https://gitlab.com/bztsrc/easyboot) as well). All they do is dumping the received boot parameters to the serial console.

Compilation
-----------

`make all`

Will compile the example 64-bit [Multiboot2](https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html)-compatible ELF
kernel and create the bootable disk image `disk.img`.

`LINUX=1 make all`

Creates a disk image with an example kernel that speaks the
[Linux/x86 Boot Protocol](https://www.kernel.org/doc/html/latest/arch/x86/boot.html) v2.12+ (needs objcopy).

`MB32=1 make all`

Creates a disk image with a 32-bit Multiboot2-compatible ELF kernel.

`PE=1 make clean all`

Creates a disk image with a 64-bit Multiboot2-compatible COFF/PE kernel (requires Clang + lld).

`MB32=1 PE=1 make all`

Creates a disk image with a 32-bit Multiboot2-compatible COFF/PE kernel (requires Clang + lld).

`RPI=1 make clean all`

Creates a disk image with a 64-bit Multiboot2-compatible kernel for the Raspberry Pi (requires Clang + lld).

`RPI=1 PE=1 make clean all`

Creates a disk image with a 64-bit Multiboot2-compatible COFF/PE kernel for the Raspberry Pi (requires Clang + lld).

`make clean`

Cleans the repo from the compiled binaries.

Plus all cases can be prefixed by `HI=1` which will compile a higher half kernel, and `SMP=1` will test Symmetric
MultiProcessing (latter only with 64-bit Multiboot2-compatible ELF/COFF kernels, no Linux and no 32-bit).

Testing
-------

`make mnt`

Mounts the boot partition inside the disk image so that you can examine its contents.

`make qemu`

Boots the disk image in qemu using BIOS (or Raspberry Pi if `RPI` set).

`make efi`

Boots the disk image in qemu using UEFI (you have to provide your own OVMF).

`make cdrom`

Boots the disk image in qemu using BIOS and El Torito "no emulation" mode.
Use `CDROM=1 make disk.img` to generate a hybrid disk / cdrom image.

`make eficdrom`

Boots the disk image in qemu using UEFI and El Torito "no emulation" mode (you have to provide your own OVMF).
Use `CDROM=1 make disk.img` to generate a hybrid disk / cdrom image.

`make bochs`

Boots the disk image in bochs using BIOS.

`make rpi`

Boots the disk image in qemu on Raspberry Pi.

`make cb`

Boots the disk image using coreboot. Note: this toolchain generates `boot/mykernel`, and coreboot will complain about not finding
the kernel. To fix, either add `boot/simpleboot.cfg` too or use `mv boot/mykernel boot/kernel; make cb` (unlike with the other
loaders, you cannot change the default name with `-k` because coreboot is a read-only ROM image, so only config file works).

Debugging
---------

`make qemudbg`

Boots the disk image in qemu using BIOS, but does not start the VM.

`make efidbg`

Boots the disk image in qemu using UEFI, but does not start the VM.

`make gdb`

Run this in another terminal. It runs gdb, connects it to the stopped VM and starts execution.
Because there's no symbol file, it's a bit tricky to use. Place `1:jmp 1b` anywhere in the code, and press <kbd>Ctrl</kbd>+
<kbd>C</kbd> in the gdb's window. You'll see that the VM is running that jump. Use `set $pc += 2` to step over the jump
instruction, and after that you can do step by step execution with `si`, or continue with `c`.
