// Copyright(c) 2019 Pierre Krieger

#![feature(never_type)]
#![warn(missing_docs)]
#![deny(unsafe_code)]

// TODO: futures don't work in #![no_std] :-/
// #![no_std]

extern crate alloc;

pub mod core;
pub mod interface;
pub mod module;
pub mod system;

mod predef;
