Experiment to build some kind of operating-system-like environment where executables are all in
WASM and are loaded from some IPFS-like decentralized network.

I'm frequently telling people what my vision of an operating system would be. Now I've started
building it.

[![dependency status](https://deps.rs/repo/github/tomaka/os/status.svg)](https://deps.rs/repo/github/tomaka/os)

# How to test

```
# You need the WASI target installed:
rustup target add wasm32-wasi

# Then:
cargo run
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
  exist locally, is fetched from IPFS or something similar.

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

- Threads and Futures are working.
- WASM programs can use TCP/IP, but the implementation is very hacky.
- Building IPFS is currently blocked due to the lack of Rust ECDH library that compiles for WASM.
  The plan is to bypass this problem by not using encryption.
