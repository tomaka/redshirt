// Copyright(c) 2019 Pierre Krieger

use core::ffi::c_void;
use parity_scale_codec::{Encode, Decode};

#[link(wasm_import_module = "")]
extern "C" {
    /// Asks for the next message.
    ///
    /// The `to_poll` parameter must be a list (whose length is `to_poll_len`) of messages to poll.
    /// Entries in this list equal to `0` are ignored. Entries equal to `1` are special and mean
    /// "a message received on an interface". If a message is successfully pulled, the
    /// corresponding entry in `to_poll` is set to `0`.
    ///
    /// If `block` is true, then this function puts the thread to sleep until a message is
    /// available. If `block` is false, then this function returns as soon as possible.
    ///
    /// If the function returns 0, then there is no message available and nothing has been written.
    /// This function never returns 0 if `block` is `true`.
    /// If the function returns a value larger than `out_len`, then a message is available whose
    /// length is the value that has been returned, but nothing has been written in `out`.
    /// If the function returns value inferior or equal to `out_len` (and different from 0), then
    /// a message has been written in `out`.
    ///
    /// Messages written in `out` can be decoded into a [`Message`].
    pub(crate) fn next_message(
        to_poll: *mut [u64],
        to_poll_len: u32,
        out: *mut [u8],
        out_len: u32,
        block: bool
    ) -> u32;

    /// Sends a message to the process that has registered the given interface.
    ///
    /// The message body is what will go into the [`actual_data`](Message::actual_data) field of
    /// the [`Message`] that the target will receive.
    pub(crate) fn emit_message(
        interface_hash: *const [u8; 32],
        msg: *const [u8],
        msg_len: u32,
        needs_answer: bool,
    ) -> u64;

    /// Sends an answer back to the emitter of given `message_id`.
    ///
    /// Returns `0` on success, or `1` if there is no message with that id.
    pub(crate) fn emit_answer(
        message_id: u64,
        msg: *const [u8],
        msg_len: u32,
    ) -> u32;

    /// Registers the current process as the provider for the interface passed as parameter.
    ///
    /// Only one process can ever be registered for any given interface. If `register_interface`
    /// is ever called twice (even amongst different processes), the second call will fail.
    ///
    /// Return `0` on success, or `1` on failure.
    ///
    /// If this succeeds, when a process calls `emit_message` with that interface hash, the
    /// message will arrive to the current process and can be retrieved with [`next_message`].
    pub(crate) fn register_interface(interface_hash: *const [u8; 32]) -> u32;
}

#[derive(Debug, Encode, Decode)]
pub enum Message {
    Interface(InterfaceMessage),
    Response(ResponseMessage),
}

#[derive(Debug, Encode, Decode)]
pub struct InterfaceMessage {
    pub message_id: u64,
    pub emitter_pid: u64,
    pub actual_data: Vec<u8>,
}

#[derive(Debug, Encode, Decode)]
pub struct ResponseMessage {
    pub message_id: u64,
    pub actual_data: Vec<u8>,
}
