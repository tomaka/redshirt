The **redshirt** operating system is an experiment to build some kind of operating-system-like
environment where executables are all in WASM and are loaded from an IPFS-like decentralized
network.

I'm frequently telling people what my vision of an operating system would be. Now I've started
building it.

[![dependency status](https://deps.rs/repo/github/tomaka/os/status.svg)](https://deps.rs/repo/github/tomaka/os)

# How to test

## Pre-compiled binary

GitHub Actions automatically build a bootable ISO from the master branch.

GitHub Actions doesn't provide yet an easy way to access build artifacts, so follow these instructions:

- Go here: https://github.com/tomaka/redshirt/actions?query=branch%3Amaster+workflow%3A%22Continuous+integration%22
- Click on the first "Continuous integration" item of the list.
- Click on "Artifacts" on the right of the screen.
- Click on "bootable-x86_64". This downloads an archive containing the ISO `redshirt.iso`.

You can then try it using QEMU:

```
qemu-system-x86_64 -drive file=redshirt.iso -m 1024
```

Alternatively, you can write `redshirt.iso` on a USB stick and boot an actual machine with it.

## Compiling from source

There are two binaries available in this repository:

- The "hosted kernel" is a regular binary that executes WASM programs and uses the host operating
  system.
- The freestanding kernel is a multiboot2-compliant kernel that can be loaded with GRUB2 or any
  compliant bootloader.

For the hosted kernel:

```
# You need the WASI target installed:
rustup target add wasm32-wasi

# Then:
cargo run
```

For the freestanding kernel:

```
rustup target add wasm32-wasi

# From the root directory of this repository (where the `arm-freestanding.json` file is located):
RUST_TARGET_PATH=`pwd` cargo +nightly build -Z build-std=core,alloc --target arm-freestanding --package redshirt-standalone-kernel

# You now have a `target/arm-freestanding/debug/redshirt-standalone-kernel`.
# It can be loaded directly by QEMU:
qemu-system-arm -M raspi2 -m 2048 -serial stdio -kernel ./target/arm-freestanding/debug/redshirt-standalone-kernel
```

The freestanding kernel also supports x86_64:

```
RUST_TARGET_PATH=`pwd` cargo +nightly build -Z build-std=core,alloc --target x86_64-multiboot2 --package redshirt-standalone-kernel
```

Unfortunately, the `-kernel` CLI option of QEMU doesn't support the multiboot2 standard (which we use). See https://github.com/tomaka/os/issues/75.
You can however put the kernel on a USB disk or CD-ROM, and boot from it:

```
mkdir -p iso/boot/grub
cp .github/workflows/grub.cfg iso/boot/grub
cp target/x86_64-multiboot2/debug/redshirt-standalone-kernel iso/boot/kernel
# Note: grub-mkrescue is sometimes called grub2-mkrescue
grub-mkrescue -o redshirt.iso iso
qemu-system-x86_64 -drive file=redshirt.iso -m 1024
```

# Repository structure

Short overview of the structure of the repository:

- `core` is a crate containing all the core infrastructure of interpreting WASM and inter-process
  communication. It is meant to become `#![no_std]`-compatible.
- `interfaces` contains crates that provide definitions and helpers for WASM programs to use
  (examples: `tcp` for TCP/IP, `window` for windowing).
- `kernel` contains the kernel binaries, plus crates that implement interfaces using the host's
  environment (e.g.: implements the `tcp` interface using Linux's or Window's TCP/IP).
- `modules` contains WASM programs.

# Contributing

Please note that so far this is mostly a personal project. I reserve the right to break anything
at any time.

In general, I'd gladly accept PRs that fix bugs, do some minor API improvements, fix typos, etc.
However, since this is just a prototype, anything more involved is probably a bad idea. Feel free
to get in touch if you want to contribute anything non-trivial.

# General idea

- This is an operating-system-like environment, but it could be seen as similar to a web browser
  or something similar.

- If it ever becomes a real OS, everything would be run in ring 0. Isolation is guaranteed by the
  WASM interpreter, and no hardware capability is required.

- Programs are referred to by their hash, not by a file name. For example you don't tell the OS
  "execute /usr/bin/foo". Instead you say "execute A45d9a21c3a7". The WASM binary, if it doesn't
  exist locally, is fetched from a peer-to-peer network similar to IPFS.

- There exists 3 core syscalls (send a message, send an answer, wait for a message), and
  everything else is done by passing messages between processes or between a process and the
  "kernel". Programs don't know who they are sending the message to.

- A program can register itself as a handler of an interface. Example of what an interface is:
  TCP/IP, files manager, threads manager, etc. Interfaces are referred by hash as well. Only one
  process can be a handler of an interface at any given point in time. For example, the process
  that handles TCP/IP registers itself as the handler of the TCP/IP interface. The user decides
  which process handles which interface.

- Low-level interfaces are handled by the kernel itself. On desktop, the kernel handles for example
  TCP/IP, UDP, file system, etc. by asking the host OS. On bare metal, the provided interfaces would
  be for example "interrupt handler manager", "PCI", etc. and the handler for the TCP/IP interface
  would be a regular WASM process that communicates with the PCI, Ethernet, etc. interfaces.

- For security purposes, the user could choose to grant or deny to access to interface on a
  per-program basis, much like Android/browsers/etc. do. Don't want "sudoku" to access TCP/IP?
  Deny it.

- Very few things are built in. No built-in concepts such as networking or files. Almost
  everything is done through interfaces.

- Interfaces are referred to by a hash built in a determinstic way based on the name of the
  interface and its messages. If you make a breaking change to an interface, it automatically
  becomes incompatible. There's no version number.

- The programs loader is itself just an interface handler. In other words, when the kernel wants to
  start a program, it sends an IPC message to a process that then returns the WASM bytecode.

# Current state

- Futures are working.
- WASM programs can use TCP/IP, but the implementation is very hacky.
- Building the IPFS-like network is currently blocked due to the lack of Rust ECDH library that compiles for WASM.
  The plan is to bypass this problem by not using encryption.
- There is a freestanding version of the kernel for the bare metal for x86_64.
