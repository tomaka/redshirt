// Copyright(c) 2019 Pierre Krieger

#![feature(never_type)]
#![warn(missing_docs)]
#![deny(unsafe_code)]

// TODO: futures don't work in #![no_std] :-/
// #![no_std]

extern crate alloc;     // TODO: is that needed?
extern crate core;      // TODO: is that needed?

pub mod interface;
pub mod module;
pub mod scheduler;
pub mod signature;
pub mod system;

mod predef;
