Booting a kernel with Simpleboot
================================

[Simpleboot](https://gitlab.com/bztsrc/simpleboot) is an all-in-one boot loader and bootable disk image creator that can load Linux
kernels and Multiboot2 compliant kernels in ELF and PE formats.

[[_TOC_]]

Configuration
-------------

```
 simpleboot [-v|-vv] [-k <name>] [-i <name>] [-m|-g] [-s <mb>] [-b <mb>] [-u <guid>] [-p <t> <u> <i>]
   [-r|-e] [-c] <indir> <outfile|device>

  -v, -vv         increase verbosity / validation
  -k <name>       set the default kernel filename (defaults to 'kernel')
  -i <name>       set the default initrd filename (by default none)
  -m              set multicore to enabled (by default disabled)
  -g              set 32-bit GRUB compatibility mode (by default disabled)
  -s <mb>         set the disk image size in Megabytes (defaults to 35M)
  -b <mb>         set the boot partition size in Megabytes (defaults to 33M)
  -u <guid>       set the boot partition's unique identifier (defaults to random)
  -p <t> <u> <i>  add a root partition (type guid, unique guid, imagefile)
  -r              place loaders in ROM (by default save them into the image)
  -e              add El Torito Boot Catalog (BIOS / EFI CDROM boot support)
  -c              always create a new image file even if it exists
  indir           use the contents of this directory for the boot partition
  outfile         output image file or device name
```

The **Simpleboot** tool creates a bootable disk image named `(outfile)` using GUID Partitioning Table with a single partition
formatted as FAT32 and named as "EFI System Partition". The contents of that partition are taken from the `(indir)` you provide.
Unless specified otherwise, by default the file named `kernel` in the root of that partition will be booted. If you place a file
named `simpleboot.cfg` in the same root directory, then it will be parsed during boot, and will overrule the defaults. Instead of
that overcomplicated and brain-dead OS image Multiboot2 header, **Simpleboot** uses a simple plain text file for configuration.
With either NL or CRLF line endings, you can easily edit it with any text editor. No need for re-compilation or messing with a
hexeditor just to change the screen resolution for example.

The `-k (name)` and `-i (name)` arguments change the default kernel filename and default initrd filename, respectively. Note that
specifying these do absolutely nothing with any files, they just set the default names in the loader. Likewise `-m` turns multicore
(Symmetric MultiProcessing) on by default. These are just defaults, all of these can be overridden from the config file during boot.

Normally 32-bit kernels started in protected mode (just like with GRUB), and 64-bit kernels in long mode (GRUB does not support
these). With `-g` you can enforce a 32-bit GRUB compatible protected mode entry point even on 64-bit kernels. Mutually exclusive
with the `-m` flag (GRUB does not support SMP at all).

The tool also has some optional command line flags: `-s (mb)` sets the overall size of the generated disk image in Megabytes, while
`-b (mb)` sets the size of the boot partition in Megabytes. Obviously the former must be bigger than the latter. If not specified,
then partition size is calculated from the size of the given directory (33 Mb at a minimum, the smallest FAT32 there can be) and
disk size defaults to 2 Mb bigger than the partition (due to alignment and space needed for the partitioning table). If there's a
more than 2 Mb gap between these two size values, then you can use 3rd party tools like `fdisk` to add more partitions to the
image to your liking (or see `-p` below). If you want a predictable layout, you can also specify the boot partition's unique
identifier (UniquePartitionGUID) with the `-u <guid>` flag.

Optionally you can also add extra partition(s) with the `-p` flag. This requires 3 arguments: (PartitionTypeGUID),
(UniquePartitionGUID) and the name of the image file that contains the contents of the partition. This flag can be repeated
multiple times.

The `-r` flag does not save the loader into the image or on the disk, it rather creates `sb_bios.rom` (a legacy BIOS Expansion
ROM) and `sb_uefi.rom` (an UEFI PCI Option ROM) files next to the output file, which you can flash to a motherboard's ROM. For
backward compatibility only, if you really want ROM booting, then you should use the more advanced [coreboot](coreboot.md) loader
instead. Mutually exclusive with the `-e` flag.

The `-e` flag adds El Torito Boot Catalog to the generated image, so that it can be booted not just as an USB stick but as a
BIOS / EFI CDROM too. Mutually exclusive with the `-r` flag.

If `(outfile)` is a device file (eg. `/dev/sda` on Linux, `/dev/disk0` on BSDs, and `\\.\PhysicalDrive0` on Windows), then it does
not create GPT nor ESP, instead it locates the already existing ones on the device. It still copies all files in `(indir)` to the
boot partition, and installs loaders. This also works if `(outfile)` is an image file that already exists (in this case use `-c` to
always create a new, empty image file first).

The `simpleboot.cfg` file may contain the following lines (very similar to grub.cfg's syntax, you can find an example configuration
file [here](../example/simpleboot.cfg)):

### Comments

All lines starting with a `#` are considered comments and skipped till the end of the line.

### Verbosity level

You can set the verbosity level using a line starting with `verbose`.

```
verbose (0-3)
```

This tells the loader how much information to print to the boot console. Verbose 0 means totally quiet (default) and verbose 3
will dump the loaded kernel segments and the machine code at the entry point.

### Framebuffer

You can request a specific screen resolution with the line starting with `framebuffer`. The format is as follows:

```
framebuffer (width) (height) (bits per pixel)
```

**Simpleboot** will set up a framebuffer for you, even if this line doesn't exists (800 x 600 x 32bpp by default). But if this
line does exist, then it will try to set the specified resolution. You should check the [Framebuffer info](ABI.md#framebuffer-info)
(type 8) tag to see what resolution you actually have. Paletted modes not supported, so bits per pixel has to be 15 at least.

### Boot splash logo

You can also display a logo at the center of the screen using a line starting with `bootsplash`.

```
bootsplash [#(bgcolor)] (path to a tga file)
```

The background color is optional, and has to be in HTML notation starting with a hashmark followed by 6 hexadecimal digits, RRGGBB.
For example `#ff0000` is full bright red and `#007f7f` is a darker cyan. If the first argument does not start with `#`, then a path
argument is assumed.

The path must point to an existing file, and it has to be an absolute path on the boot partition. The image must be in a run length
encoded, color-mapped [Targa format](https://www.gamers.org/dEngine/quake3/TGA.txt), because that's the most compressed variant
(first three bytes of the file must be `0`, `1` and `9` in this order, see Data Type 9 in the specification).

To save in this format from GIMP, first select "Image > Mode > Indexed...", in the pop-up window set "Maximum number of colors"
to 256. Then select "File > Export As...", enter a filename which ends in `.tga`, and in the pop-up window check "RLE compression".
For a command line conversion tool, you can use ImageMagick, `convert (any image file) -colors 256 -compress RLE bootsplash.tga`.
(Note: I was first considering [qoi](https://qoiformat.org/), but in all my test cases tga compressed much better. That website
is cheating big time, it presents only special, carefully hand-picked test images. No boot logos among them.)

### Loading a kernel

The line starting with `kernel` tells what file should be booted, and with what parameters. If this line is missing, then the
filename given with `-k` on the **Simpleboot** command line will be used, and if even that's missing then simply the name `kernel`.

```
kernel (path to your kernel file) (optional boot command line arguments)
```

The path must point to an existing file, a kernel binary, and it has to be an absolute path on the boot partition. If the kernel
isn't in the root directory of the partition, then the directory separator is always `/`, even on UEFI systems. If the name contains
a space, then that must be escaped with `\`. The path might be followed by command line arguments, separated by a space. These
command line arguments will be passed to your kernel in the [Boot command line](ABI.md#boot-command-line) (type 1) tag.

NOTE: unlike GRUB, where you have to use special commands like "linux" or "multiboot" to select the boot protocol, here there's
just one command and the protocol is autodetected from your kernel in run-time.

NOTE: normally the Multiboot2 protocol does not allow higher-half kernels, but **Simpleboot** violates the protocol a little bit
for those kernels in a way that does not break normal, non-higher-half Multiboot2 compatible kernels.

### Loading further modules

You can load arbitrary files (initial ramdisks, kernel drivers, etc.) along with the kernel using lines starting with `module`.
Note that this line can be repeated multiple times. If initrd was given on the **Simpleboot** command line, then that counts as
the first module, therefore the very first `module` line in the configuration file will override it.

```
module (path to a file) (optional module command line arguments)
```

The path must point to an existing file, and it has to be an absolute path on the boot partition. It might be followed by command
line arguments, separated by a space. If the file is gzip compressed, then it will be transparently uncompressed. Information
about these loaded (and uncompressed) modules will be passed to your kernel in [Modules](ABI.md#modules) (type 3) tags.

The special case is if the module starts with the bytes `DSDT`, `GUDT` or `0xD00DFEED`. In these case the file won't be added to
the Modules tags, rather the ACPI table will be patched so that its DSDT pointers will point to the contents of this file. With
this you can easily replace a buggy BIOS' ACPI table with a user provided one.

To parse these tables, you can use my dependency-free, single header [hwdet](https://gitlab.com/bztsrc/hwdet) library.

### Multicore Support

To start the kernel on all processor cores at once, specify the `multicore` directive (64-bit Multiboot2 kernels only). This will
add [SMP](ABI.md#smp) (type 257) tag to the MBI.

```
multicore
```

Emergency Backup
----------------

If during loading you press <kbd>any</kbd> key, then the boot process will restart, only this time `simpleboot.cfg` will be
parsed for commands prefixed by `backup`, for example `backupkernel`, `backupmodule`, etc.

With his feature you can quickly revert to a known to work configuration should you experience any issues with your latest kernel
or modules.

Troubleshooting
---------------

If you encounter any problems, just run with `simpleboot -vv` flag. This will perform validation and will output the results
verbosely at image creation time. Otherwise add `verbose 3` to `simpleboot.cfg` to get detailed boot time messages.

If you see `PMBR-ERR` string on the top left corner with red background, then that means your CPU is very old and does not
support 64-bit long mode or the boot sector was unable to bootstrap the loader. Might occur only on BIOS machines, this can
never happen with UEFI or on RaspberryPi.

Otherwise please feel free to open an [issue](https://gitlab.com/bztsrc/simpleboot/-/issues) on gitlab.

