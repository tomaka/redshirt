The **redshirt** operating system is an experiment to build some kind of operating-system-like
environment where executables are all in Wasm and are loaded from an IPFS-like decentralized
network.

See the `docs/introduction.md` file for an introduction.

[![dependency status](https://deps.rs/repo/github/tomaka/os/status.svg)](https://deps.rs/repo/github/tomaka/os)

# How to test

**Important**: At the moment, most of the compilation requires a nightly version of Rust. See also https://github.com/tomaka/redshirt/issues/300.
Your C compiler must be recent enough to be capable of compiling to WebAssembly. This is for example the case for clang 9. See also https://github.com/tomaka/redshirt/issues/257.

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
# Loads the module whose hash is FWMwRMQCKdWVDdKyx6ogQ8sXuoeDLNzZxniRMyD5S71 and executes it.
# This should print "hello world".
cargo +nightly run -- --module-hash FWMwRMQCKdWVDdKyx6ogQ8sXuoeDLNzZxniRMyD5S71
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
- `docs` contains a description of what redshirt is and how it works. Start with `docs/introduction.md`.
- `interfaces` contains crates that provide definitions and helpers for Wasm programs to use
  (examples: `tcp` for TCP/IP, `window` for windowing).
- `kernel` contains the kernel binaries, plus crates that implement interfaces using the host's
  environment (e.g.: implements the `tcp` interface using Linux's or Window's TCP/IP).
- `modules` contains Wasm programs.

# Contributing

Please note that so far this is mostly a personal project. I reserve the right to change anything
at any time, including the license.
