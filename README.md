The **redshirt** operating system is an experiment to build some kind of operating-system-like
environment where executables are all in Wasm and are loaded from an IPFS-like decentralized
network.

I'm frequently telling people what my vision of an operating system would be. Now I've started
building it.

[![dependency status](https://deps.rs/repo/github/tomaka/os/status.svg)](https://deps.rs/repo/github/tomaka/os)

# How to test

**Important**: At the moment, most of the compilation requires a nightly version of Rust. See also https://github.com/tomaka/redshirt/issues/300.

You also need to install the `wasm32-wasi` target, as the Wasm modules are compiled for Wasi, and the `rust-src` component in order to build the standalone kernel.

```
rustup toolchain install --target=wasm32-wasi nightly
rustup component add --toolchain=nightly rust-src
```

There are two binaries available in this repository:

- The "CLI kernel" is a regular binary that executes Wasi programs and leverages functionalities from the host operating system.
- The freestanding kernel is a bare-metal kernel.

For the CLI kernel:

```
# TODO: `--module-hash` must be passed the hash of the module to load,
# but there is no modules-hosting platform at t the moment
# See https://github.com/tomaka/redshirt/issues/333
cargo +nightly run -- --module-hash=A
```

For the freestanding kernel:

```
cd kernel/standalone-builder
cargo +nightly run -- emulator-run --emulator qemu --target x86_64-multiboot2
```

# Repository structure

Short overview of the structure of the repository:

- `core` is a crate containing all the core infrastructure of interpreting Wasm and inter-process
  communication. It is meant to become `#![no_std]`-compatible.
- `interfaces` contains crates that provide definitions and helpers for Wasm programs to use
  (examples: `tcp` for TCP/IP, `window` for windowing).
- `kernel` contains the kernel binaries, plus crates that implement interfaces using the host's
  environment (e.g.: implements the `tcp` interface using Linux's or Window's TCP/IP).
- `modules` contains Wasm programs.

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
  Wasm interpreter, and no hardware capability is required.

- Programs are referred to by their hash, not by a file name. For example you don't tell the OS
  "execute /usr/bin/foo". Instead you say "execute A45d9a21c3a7". The Wasm binary, if it doesn't
  exist locally, is fetched from a peer-to-peer network similar to IPFS.

- There exists 3 core syscalls (send a message, send an answer, wait for a notification), and
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
  would be a regular Wasm process that communicates with the PCI, Ethernet, etc. interfaces.

- For security purposes, the user could choose to grant or deny to access to interface on a
  per-program basis, much like Android/browsers/etc. do. Don't want "sudoku" to access TCP/IP?
  Deny it.

- Very few things are built in. No built-in concepts such as networking or files. Almost
  everything is done through interfaces.

- Interfaces are referred to by a hash built in a determinstic way based on the name of the
  interface and its messages. If you make a breaking change to an interface, it automatically
  becomes incompatible. There's no version number.

- The programs loader is itself just an interface handler. In other words, when the kernel wants to
  start a program, it sends an IPC message to a process that then returns the Wasm bytecode.
