The **redshirt** operating system is an experiment to build some kind of operating-system-like
environment where executables are all in Wasm and are loaded from an IPFS-like decentralized
network.

See the `docs/introduction.md` file for an introduction.

# How to test

**Important**: At the moment, most of the compilation requires a nightly version of Rust. See also https://github.com/tomaka/redshirt/issues/300.
Your C compiler must be recent enough to be capable of compiling to WebAssembly. This is for example the case for clang 9. See also https://github.com/tomaka/redshirt/issues/257.

You also need to install the `wasm32-wasip1` target, as the Wasm programs are compiled for Wasi, and the `rust-src` component in order to build the standalone kernel.

```
rustup toolchain install --target=wasm32-wasip1 nightly
rustup component add --toolchain=nightly rust-src
```

Building the freestanding kernel is then done through the utility called `standalone-builder`:

```
cd kernel-standalone-builder
cargo +nightly run -- emulator-run --emulator qemu --target x86_64-multiboot2
```

# Repository structure

Short overview of the structure of the repository:

- `docs` contains a description of what redshirt is and how it works. Start with `docs/introduction.md`.
- `interfaces` contains crates that provide definitions and helpers for Wasm programs to use
  (examples: `tcp` for TCP/IP, `window` for windowing).
- `kernel` contains the code required to run the kernel.
- `kernel-standalone-kernel` contains a utility allowing to run and test the standalone kernel.
- `programs` contains Wasm programs.

# Contributing

Please note that so far this is mostly a personal project. I reserve the right to change anything
at any time, including the license.
