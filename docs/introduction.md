# Introduction

Redshirt is similar to an operating system. It allows executing programs that communicate with each other via messages.

# Programs

The concept of a **program** is the same as in an operating system. It is a list of instructions that are executed and that have access to some memory.

Contrary to most operating systems, the instruction set of programs in redshirt is [WebAssembly](https://webassembly.org/) (also referred to as "Wasm").

Using WebAssembly as the instructions set has two consequences:

- All programs are cross-platform.
- Programs are not executed directly by the CPU, but by an interpreter or must first be translated into native code.

Thanks to the second point, we can eliminate the need for mechanisms such as paging/virtual memory and privilege levels. In other words, we don't make use of any of the sandboxing measures normally provided by the CPU.

## How are programs actually executed?

At the time of the writing of this documentation, we use [the `wasmi` crate](https://docs.rs/wasmi) to execute programs.

It has been measured that compiling a program and executing it with `wasmi` is approximately ten times slower than compiling the same program for the native architecture and executing it.
This measurement has been performed within Linux and this doesn't account, however, for the overhead introduced by the CPU's sandboxing measures.

Crates such as [`wasmtime`](https://docs.rs/wasmtime) should make it possible to considerably boost the performance of programs. Comparing a well-optimized HTTP server compiled for the native architecture to a badly-optimized HTTP server executed by `wasmtime` showed that the latter was capable of serving half of the requests per second of the former.

## Limitations of sandboxing

One design issue that hasn't been solved at the time of this writing is  [preemption](https://en.wikipedia.org/wiki/Preemption_(computing)). In other words: how to run multiple CPU-intensive programs on the same CPU? In a typical operating system, the CPU receives periodic interrupts during which the operating system swaps the current thread for another.

Ideally, we would like to avoid relying on the CPU receiving interrupts and instead rely on [cooperating multitasking](https://en.wikipedia.org/wiki/Cooperative_multitasking). It is possible to make the WebAssembly interpreter or JIT insert periodic checks that interrupt the execution if a certain time has passed, but these checks are generally thought to be prohibitively expensive.

Additionally, at the time of this writing, redshirt doesn't enforce any limit on the memory that Wasm programs can use. This is however only a matter of implementation.

## About threads

WebAssembly at the moment doesn't specify any threading model or any memory model.

While redshirt has experimental support for creating multiple threads within a single process, WebAssembly-targeting compilers work under the assumption that only a single thread exists at any given point in time. As an example, LLVM stores the current stack pointer in a global variable.

Consequently, while experimental support exists, it isn't possible at the moment for programs to create secondary threads.

# Messages and interfaces

Programs are totally sandboxed, except for their capability to send messages to other processes and receive messages sent by other processes.

It is not possible, however, to send a message to a specific process (through a PID for instance). Instead, when you send a message, what you target is an **interface**.

Here is how it works:

- Program A registers itself towards the operating system to be the handler of a specific interface I.
- Program B sends a message whose destination is I.
- Program A receives the message that B has sent.
- Optionally, A can send a single answer to B's message.

In this example, B doesn't know about the existence of A. However, A knows about the existence of B so that it can properly track resources allocation.

Interfaces are referred to by a hash.

## What is the hash of an interface?

In the initial design, interfaces are defined by a list of messages and answers. The hash of an interface corresponds to the hash of the definition of these messages.

This is however quite difficult to implement in practice, and at the moment the hash is randomly generated manually (by visiting https://random.org) when a new interface is defined.

## Answers

Each message emitted accepts either zero or one answer. The sender indicates to the operating system whether it expects an answer.

If the emitter indicates that it expects an answer, a unique **message ID** gets assigned. This identifier is later passed to the interface handler for it to indicate which message it is answering.

## Message bodies and answer bodies

The actual message's body and the answer's body consist of an opaque buffer of data.

The way this body must be interpreted depends on the interface the message is targetting.
