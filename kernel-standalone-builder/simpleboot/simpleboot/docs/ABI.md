Writing Simpleboot compatible kernels
=====================================

[Simpleboot](https://gitlab.com/bztsrc/simpleboot) is an all-in-one boot loader and bootable disk image creator that can load Linux
kernels and Multiboot2 compliant kernels in ELF and PE formats.

This is the very same protocol that [Easyboot](https://gitlab.com/bztsrc/easyboot) uses, all of the example kernels in this
repo must work with **Easyboot** too.

You can use the original Multiboot2 header in GRUB's repo, or the [simpleboot.h](../simpleboot.h) C/C++ header file to get easier to
use typedefs. The low-level binary format is the same, you can also use any existing Multiboot2 libraries, even with non-C
languages, like this [Rust](https://github.com/rust-osdev/multiboot2/tree/main/multiboot2/src) library for example (note: I'm not
affiliated with those devs in any way, I just searched for "Rust Multiboot2" and that was the first result).

Various kernel protocols only supported on x86, while 64-bit Multiboot2 kernel is supported on all platforms.

[[_TOC_]]

Boot Sequence
-------------

### Bootstrapping the loader

On *BIOS* machines, the very first sector of the disk is loaded to 0:0x7C00, and control passed to it. In this sector
**Simpleboot** has [boot_x86.asm](../src/boot_x86.asm), which is smart enough to locate and load the 2nd stage loader, and also
to set up long mode for it.

On *UEFI* machines the very same 2nd stage file, called `EFI/BOOT/BOOTX64.EFI` is loaded directly by the firmware. The source for
this loader can be found in [loader_x86.c](../src/loader_x86.c). That's it, **Simpleboot** isn't GRUB nor syslinux, both of which
requiring dozens and dozens of system files on the disk. Here no more files needed, just this one.

On *Raspberry Pi* the loader is called `KERNEL8.IMG`, compiled from [loader_rpi.c](../src/loader_rpi.c). This will work in qemu,
but to boot on a real machine, you'll need further firmware files `bootcode.bin`, `fixup.dat` and `start.elf` too, which can be
downloaded from the [official repository](https://github.com/raspberrypi/firmware/tree/master/boot). Place these files in the
directory that you use to generate the boot partition.

On *coreboot* machines, there's no loader file, it is part of the ROM and flashed with the rest of firmware to the motherboard.
This version is compiled from [loader_cb.c](../src/loader_cb.c) which is built on top of coreboot's libpayload library.

### The loader

This loader is very carefully written to work on multiple platforms and configurations. It loads the GUID Partitioning Table from
the disk, and looks for an "EFI System Partition". When found, it looks for the `simpleboot.cfg` configuration file on that boot
partition. If not found, then it defaults to what was given during disk image creation. Now that your kernel's filename is known,
the loader locates and loads the first 4096 bytes of it.

Using that buffer it autodetects the kernel's format, and it is smart enough to interpret the section and segment information about
where to load what (it does on-demand memory mapping whenever necessary). Then it sets up a proper environment depending on the
detected boot protocol (Multiboot2 / Linux / etc. protected or long mode, ABI arguments etc.). After the machine state is solid and
well-defined, as a very last act, the loader jumps to your kernel's entry point.

Machine State
-------------

Everything what's written in the [Multiboot2 specification](https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html)
about the machine state stands, except for the general purpose registers. **Simpleboot** passes two arguments to your kernel's
entry point according to the SysV ABI and Microsoft fastcall ABI as well. First parameter is the magic, the second one is a
physical memory address, pointing to a Multiboot Information taglist (abbreviated as MBI hereafter, see below).

We also violate the Multiboot2 protocol a bit to handle higher-half kernels. Multiboot2 mandates that memory must be identity
mapped. Well, under **Simpleboot** this is just partially true: we only guarantee that all of the physical RAM is surely identity
mapped as expected; however some regions above that (depending on the kernel's program headers) might be yet available. This does
not break normal Multiboot2 compliant kernels, which are not supposed to access memory outside of the available physical RAM.

Your kernel is loaded exactly the same way on both BIOS and UEFI systems as well as on RPi, firmware differences are just "Somebody
Else's Problem". The only thing your kernel will see is whether the MBI contains the EFI system table tag or not. To simplify your
life, **Simpleboot** does not generate EFI memory map (type 17) tag either, it provides only the [Memory map](#memory_map) (type 6)
tag indiscriminatly on all platforms (on UEFI systems too, there the memory map is simply converted for you, so your kernel has to
deal with only one kind of tags). Old, obsolete tags are also ommited and never generated by this boot loader.

The kernel is running at supervisor level (ring 0 on x86, EL1 on ARM), possibly on all CPU cores in parallel.

GDT unspecified, but valid. Stack is set up in the first 640k, and growing downwards (but you should change this as soon as
possible to whatever stack you deem worthy). When SMP is enabled, all cores have their own stacks, and the core id is on the
top of the stack (but you can also get the core id the usual platform specific way, using cpuid / mpidr / etc.).

You should consider IDT as unspecified; IRQs, NMI and software interrupts disabled. Dummy exception handlers are set up to display
some minimal dump and halt the machine. These should only be relied on to report if your kernel goes havoc before you were able
to set up your own IDT and handlers, preferably as soon as possible. On ARM vbar_el1 is set up to call the same dummy exception
handlers (although they dump different registers of course).

Framebuffer is also set by default. You can alter the resolution in config, but if not given, framebuffer is still configured.

It is important to never return from your kernel. You're free to overwrite any part of the loader in memory (as soon as you've
finished with the MBI tags), so there's simply nowhere to return to. "Der Mohr hat seine Schuldigkeit getan, der Mohr kann gehen."

### ELF32 / PE32

If your kernel is a 32-bit binary (or 64-bit but `-g` force GRUB compatiblity mode flag was used), then the kernel will run in
protected mode, without any paging and with flat segmentation (meaning segments will have the base of 0 and limit of 0xFFFFFFFF).
These 32-bit kernels are loaded with a 100% GRUB-compatible Multiboot2 protocol.

Note that 32-bit is *just for kernel* backward compatibility, you'll still have to have a 64-bit machine (EFI32 and legacy
computers without long mode not supported).

### ELF64 / PE32+

Otherwise if your kernel is a 64-bit binary, then it will run in long mode, with identity mapping (meaning virtual addresses are
the same as physical addresses). No 64-bit trampoline code needed and kernel could be higher-half mapped, so this is *not*
GRUB-compatible (which mandates 32-bit protected mode entry point). On ARM only 64-bit mode supported.

These 64-bit kernels might be booted on all CPU cores at once too using the `multicore` directive (or the `-m` flag, SMP).

### Linux kernels

Linux (if kernel has 0xAA55 at offset 0x1FE and `HdrS` at offset 0x202) is a special case, with such kernels *Multiboot2 not used*
at all, instead the [The Linux/x86 Boot Protocol](https://www.kernel.org/doc/html/latest/arch/x86/boot.html) applies. The minimum
boot protocol version supported is v2.12 (the value at offset 0x206 must be larger or equal to 0x20C). Has no program headers, so a
block starting at file offset `((byte at offset 0x1F1 + 1) * 512)` and `(uint32_t at offset 0x260)` bytes long is loaded to address
`(uint32_t at offset 0x258)`. The control is then transferred to address `(uint32_t at offset 0x258) + 512` in pure 64-bit long
mode. No MBI tags passed, instead `rsi` points to a [zero page](https://www.kernel.org/doc/html/latest/arch/x86/zero-page.html),
which is a [struct boot_params](https://github.com/torvalds/linux/blob/master/arch/x86/include/uapi/asm/bootparam.h#L185)
structure, with very similar information.

On ARM, the Linux kernel has the magic `ARM` plus a byte 64 at offset 0x38. This image is loaded at 0x80000, which is also the
top of the stack and the entry point. The FDT (Flattened Device Tree, .dtb) pointer is passed in the `x0` register.

Boot information passed to your kernel
--------------------------------------

It's not obvious at first, but Multiboot2 actually specifies two, totally independent set of tags:

- The first set supposed to be inlined in a Multiboot2 compliant kernel, called OS image's Multiboot2 header (section 3.1.2), hence
  *provided by the kernel*. **Simpleboot** does not care about these tags, and it does not parse your kernel for these either.
  You simply don't need any special magical data embedded in your kernel file with **Simpleboot**, ELF and PE headers suffice.

- The second set is *passed to the kernel* dynamically on boot, **Simpleboot** uses only these tags. However it does not generate
  all that Multiboot2 specifies (it simply omits the old, obsoleted or legacy ones). These tags are called the MBI tags, see
  [Boot information](https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html#Boot-information-format) (section 3.6).

NOTE: the Multiboot2 specification on MBI tags is buggy as hell. You can find a fixed up version below, which aligns with the
multiboot2.h header file that you can find in GRUB's source repository. This spec also contains the **Simpleboot** additions.

The first parameter to your kernel is the magic 0x36d76289 (in `rax`, `rcx` and `rdi`). You can locate the MBI tags using the
second parameter (in `rbx`, `rdx` and `rsi`). On ARM platform magic is in `x0` and address is in `x1`. On RISC-V and MIPS `a0` and
`a1` used, respectively. If and when this loader is ported to another architecture, then always the registers specified by SysV ABI
for function arguments must be used. If there are other common ABIs on the platform which do not interfere with SysV ABI, then the
values should be duplicated in those ABI's registers (or on the top of the stack) too.

### Headers

The passed address is always 8-bytes aligned, and starts with an MBI header:

```
        +-------------------+
u32     | total_size        |
u32     | reserved          |
        +-------------------+
```

This is followed by a series of also 8-bytes aligned tags. Every tag begins with the following tag header fields:

```
        +-------------------+
u32     | type              |
u32     | size              |
        +-------------------+
```

`type` contains an identifier of contents of the rest of the tag. `size` contains the size of tag including header fields but not
including padding. Tags follow one another padded when necessary in order for each tag to start at 8-bytes aligned address. Tags
are terminated by a tag of type `0` and size `8`.

### Boot command line

```
        +-------------------+
u32     | type = 1          |
u32     | size              |
u8[n]   | string            |
        +-------------------+
```

`string` contains the command line specified in *simpleboot.cfg*'s `kernel` line (without the kernel's path and filename). The
command line is a normal C-style zero-terminated UTF-8 string.

### Boot loader name

```
        +----------------------+
u32     | type = 2             |
u32     | size = 19            |
u8[n]   | string "Simpleboot"  |
        +----------------------+
```

`string` contains the name of a boot loader booting the kernel. The name is a normal C-style UTF-8 zero-terminated string. If
you're booting an emergency backup, then the string will be `Simpleboot (backup)`.

### Modules

```
        +-------------------+
u32     | type = 3          |
u32     | size              |
u32     | mod_start         |
u32     | mod_end           |
u8[n]   | string            |
        +-------------------+
```

This tag indicates to the kernel what boot module was loaded along with the kernel image, and where it can be found. The
`mod_start` and `mod_end` contain the start and end physical addresses of the boot module itself. You'll never get a gzip
compressed buffer, because **Simpleboot** transparently uncompresses those for you. The `string` field provides an arbitrary
string to be associated with that particular boot module; it is a normal C-style zero-terminated UTF-8 string. Specified in
*simpleboot.cfg*'s `module` line and its exact use is specific to the operating system. Unlike the boot command line tag, the
module tags *also include* the module's path and filename.

One tag appears per module. This tag type may appear multiple times. If an initial ramdisk was loaded along with your kernel,
then that will appear as the first module.

There's a special case, if the file is a DSDT ACPI table, an FDT (dtb) or GUDT blob, then it won't appear as a module, rather
ACPI old RSDP (type 14) or ACPI new RSDP (type 15) will be patched and their DSDT replaced with the contents of this file.

### Memory map

This tag provides memory map.

```
        +-------------------+
u32     | type = 6          |
u32     | size              |
u32     | entry_size = 24   |
u32     | entry_version = 0 |
varies  | entries           |
        +-------------------+
```

`size` contains the size of all the entries including this field itself. `entry_size` is always 24. `entry_version` is set at `0`.
Each entry has the following structure:

```
        +-------------------+
u64     | base_addr         |
u64     | length            |
u32     | type              |
u32     | reserved          |
        +-------------------+
```

`base_addr` is the starting physical address. `length` is the size of the memory region in bytes. `type` is the variety of address
range represented, where a value of `1` indicates available RAM, value of `3` indicates usable memory holding ACPI information,
value of `4` indicates reserved memory which needs to be preserved on hibernation, value of `5` indicates a memory which is
occupied by defective RAM modules and all other values currently indicated a reserved area. `reserved` is set to `0` on BIOS boots.

When the MBI generated on a UEFI machine, then various EFI Memory Map entries are stored as type `1` (available RAM) or `2`
(reserved RAM), and should you need it, the original EFI Memory Type is placed in the `reserved` field.

The map provided is guaranteed to list all standard RAM that should be available for normal use, and it is always ordered by
ascending `base_addr`. This available RAM type however includes the regions occupied by kernel, mbi, segments and modules. Kernel
must take care not to overwrite these regions (**Simpleboot** could easily exclude those regions, but that would break Multiboot2
compatibility).

### Framebuffer info

```
        +----------------------------------+
u32     | type = 8                         |
u32     | size = 38                        |
u64     | framebuffer_addr                 |
u32     | framebuffer_pitch                |
u32     | framebuffer_width                |
u32     | framebuffer_height               |
u8      | framebuffer_bpp                  |
u8      | framebuffer_type = 1             |
u16     | reserved                         |
u8      | framebuffer_red_field_position   |
u8      | framebuffer_red_mask_size        |
u8      | framebuffer_green_field_position |
u8      | framebuffer_green_mask_size      |
u8      | framebuffer_blue_field_position  |
u8      | framebuffer_blue_mask_size       |
        +----------------------------------+
```

The field `framebuffer_addr` contains framebuffer physical address. The field `framebuffer_pitch` contains the length of one row in
bytes. The fields `framebuffer_width`, `framebuffer_height` contain framebuffer dimensions in pixels. The field `framebuffer_bpp`
contains number of bits per pixel. `framebuffer_type` is always set to 1, and `reserved` always contains 0 in current version of
specification and must be ignored by OS image. The remaining field describe a packed pixel format, the channels' position and size
in bits. You can use the expression `((~(0xffffffff << size)) << position) & 0xffffffff` to get an UEFI GOP like channel mask.

### EFI 64-bit system table pointer

This tag only exists if **Simpleboot** is running on a UEFI machine. On a BIOS machine this tag never generated.

```
        +-------------------+
u32     | type = 12         |
u32     | size = 16         |
u64     | pointer           |
        +-------------------+
```

This tag contains pointer to EFI system table.

### EFI 64-bit image handle pointer

This tag only exists if **Simpleboot** is running on a UEFI machine. On a BIOS machine this tag never generated.

```
        +-------------------+
u32     | type = 20         |
u32     | size = 16         |
u64     | pointer           |
        +-------------------+
```

This tag contains pointer to EFI image handle. Usually it is boot loader image handle.

### SMBIOS tables

```
        +-------------------+
u32     | type = 13         |
u32     | size              |
u8      | major             |
u8      | minor             |
u8[6]   | reserved          |
        | smbios tables     |
        +-------------------+
```

This tag contains a copy of SMBIOS tables as well as their version.

### ACPI old RSDP

```
        +-------------------+
u32     | type = 14         |
u32     | size              |
        | copy of RSDPv1    |
        +-------------------+
```

This tag contains a copy of RSDP as defined per ACPI 1.0 specification. (With a 32-bit address.)

### ACPI new RSDP

```
        +-------------------+
u32     | type = 15         |
u32     | size              |
        | copy of RSDPv2    |
        +-------------------+
```

This tag contains a copy of RSDP as defined per ACPI 2.0 or later specification. (With a 64-bit address probably.)

These (type 14 and 15) point to an `RSDT` or `XSDT` table with a pointer to a `FACP` table, which in turn contains two pointers to
a `DSDT` table, which describes the machine. **Simpleboot** fakes these tables on machines that do not support ACPI otherwise. Also
if you provide a DSDT table, an FDT (dtb) or GUDT blob as a module, then **Simpleboot** will patch the pointers to point to that
user provided table. To parse these tables, you can use my dependency-free, single header [hwdet](https://gitlab.com/bztsrc/hwdet)
library (or the bloated [apcica](https://github.com/acpica/acpica) and [libfdt](https://github.com/dgibson/dtc)).

### EDID

Tags with `type` greater than or equal to 256 are not part of the Multiboot2 specification, nonetheless provided by **Simpleboot**.

```
        +-------------------+
u32     | type = 256        |
u32     | size              |
        | copy of EDID      |
        +-------------------+
```

This tag contains a copy of the supported monitor resolution list according to the EDID specification.

### SMP

Symmetric MultiProcessing is only supported for 64-bit Multiboot2 kernels.

```
        +-------------------+
u32     | type = 257        |
u32     | size              |
u32     | numcores          |
u32     | running           |
u32     | bspid             |
        +-------------------+
```

This tag exists if `multicore` directive was given. `numcores` contains the number of CPU cores in the system, `running` is
the number of cores that have successfully initialized and running the same kernel in parallel. The `bspid` contains the BSP
core's identifier (on x86 lAPIC id), so that kernels can distinguish APs and run a different code on those. All APs have their
own stack, and on top of the stack there'll be the id of the current core (but you can also get that using cpuid / mpidr / etc.).

### Boot Partition

```
        +-------------------+
u32     | type = 258        |
u32     | size = 24         |
u128    | bootuuid          |
u128    | rootuuid          |
        +-------------------+
```

This tag contains the unique identifier field in the GPT of the boot partition. If the booting does not use a GUID Partitioning
Table, then generated as `54524150-(device code)-(partition number)-616F6F7400000000`. The root partition's unique identifier
is only stored if `size` is 40 (Easyboot only, **Simpleboot** always generates 24 bytes long tag with just the boot partition).

Memory Layout
-------------

### BIOS machines

| Start    | End     | Description                                                  |
|---------:|--------:|--------------------------------------------------------------|
|      0x0 |   0x400 | Interrupt Vector Table (usable, real mode IDT)               |
|    0x400 |   0x4FF | BIOS Data Area (usable)                                      |
|    0x4FF |   0x500 | BIOS boot drive code (most likely 0x80, usable)              |
|    0x500 |   0x5A0 | syncronization data for SMP (usable)                         |
|    0x5A0 |  0x1000 | exception handler stack (usable after you set up your IDT)   |
|   0x1000 |  0x8000 | paging tables (usable after you set up your paging tables)   |
|   0x8000 | 0x20000 | loader code and data (usable after you set up your IDT)      |
|  0x20000 | 0x90000 | config + logo + tags; from the top to bottom: kernel's stack |
|  0x90000 | 0x9A000 | Linux kernel only: zero page + cmdline                       |
|  0x9A000 | 0xA0000 | Extended BIOS Data Area (better not to touch)                |
|  0xA0000 | 0xFFFFF | VRAM and BIOS ROM (not usable)                               |
| 0x100000 |       x | kernel segments, followed by the modules, each page aligned  |

### UEFI machines

Nobody knows. UEFI allocates memory as it pleases. Expect anything and everything, but most likely placed below 256M. All area
will be surely listed in the memory map as type = 1 (`MULTIBOOT_MEMORY_AVAILABLE`) and reserved = 2 (`EfiLoaderData`), however
this isn't exclusive, other kinds of memory too might be listed like that (boot loader's bss section for example).

### Raspberry Pi

| Start    | End     | Description                                                  |
|---------:|--------:|--------------------------------------------------------------|
|      0x0 |   0x500 | reserved by firmware (better not to touch)                   |
|    0x500 |   0x5A0 | syncronization data for SMP (usable)                         |
|    0x5A0 |  0x1000 | exception handler stack (usable after you set up your VBAR)  |
|   0x1000 | 0x20000 | paging tables (usable after you set up your paging tables)   |
|  0x20000 | 0x80000 | config + logo + tags; from the top to bottom: kernel's stack |
|  0x80000 | 0x8F000 | loader code and data (usable after you set up your VBAR)     |
|  0x8F000 |       x | kernel segments, followed by the modules, each page aligned  |

The first few bytes are reserved for [armstub](https://github.com/raspberrypi/tools/blob/master/armstubs/armstub8.S). It only
starts core 0, so to start Application Processors, write a function's address to 0xE0 (core 1), 0xE8 (core 2), 0xF0 (core 3),
which addresses are located in this area. This is irrelevant when the `multicore` directive is used, then all cores will
execute the kernel.

Although natively not supported on the RPi, you still get an ACPI old RSDP (type 14) tag, with fake tables. The `APIC` table
is used to communicate the number of available CPU cores to the kernel. The startup function address is stored in the RSD PTR
-> RSDT -> APIC -> cpu\[x].apic_id field (and core id in cpu\[x].acpi_id, where BSP is always cpu\[0].acpi_id = 0 and
cpu\[0].apic_id = 0xD8. Watch out, "acpi" and "apic" looks pretty similar).

If a valid FDT blob is passed by the firmware, or if one of the modules is a .dtb, .aml or .gudt file, then a FADT (with magic
`FACP`) table is also added. In this table, the DSDT pointer (32-bit, at offset 40) is pointing to the provided flattened device
tree blob. For Linux kernels this pointer is passed to the entry point in `x0`.

Although no memory map feature provided by the firmware, you'll still get a Memory Map (type 6) tag too, listing detected RAM
and the MMIO region. You can use this to detect the MMIO's base address, which is different on RPi3 and RPi4.
