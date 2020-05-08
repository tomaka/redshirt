Contains details about the messages passing system.

# Syscalls

There exists five syscalls at the moment:

- `next_notification`
- `emit_message`
- `emit_answer`
- `emit_message_error`
- `cancel_message`

Describing their exact API/ABI here would be redundant. Please read the source code of the `syscalls` crate.

In WebAssembly, imported functions always belong to a namespace. The namespace of these five functions is `redshirt`.

# Emitting a message

The `emit_message` syscall requires passing:

- A target interface.
- A message body.
- A flag indicating whether or not an answer is expected.
- A flag indicating whether or not the function is allowed to wait in case there is no interface handler or the interface handler is overwhelmed.

If an answer is expected, a `MessageId` gets assigned and is returned to the caller.

If no interface handler exists for the target interface, or if the queue of messages of the interface handler is full, the function will either wait or immediately return with an error, depending on the function parameters.

## System initialization

At system initialization, the kernel is tasked to start all the programs that the user has requested to start.

The kernel doesn't know in advance which program is going to use or register which interface, so it simply starts everything at the same time.

Consequently, it is possible (in particular shortly after the system's initialization) for a program to try emit a message on an interface that hasn't been registered yet but is going to be in the near future.

It is, consequently, recommended to leave the `allow_block` flag on.

## Limits

(Note: this is not yet implemented at the time of this writing)

There exists a limit to the number of simultaneous `MessageId`s held by a specific process. Messages continue to count towards the limit as long as they haven't been answered, even if they have been cancelled (see below).

This mechanism can be compared to the `ulimits` in the Linux world.

Note that this limit is not supposed to be normally reached under normal circumstances, and serves mostly as a protection against accidental infinite loops.

# Waiting for notifications

Each process is characterized by a queue of notifications that can be retrieved using this function. A notification consists of either an answer to a previously-emitted message, a message received on a registered interface, or a notification about a process being destroyed.

The `next_notification` syscall allows querying the operating system for notifications. It is similar to `epoll` in the Linux world.

The parameters of this syscall are:

- A list of `MessageId`s whose answer to query, or the special value `1` that represents "an interface message or a process destroyed notification".
- A flag indicating whether to wait or not.

If an notification in the queue matches one of the elements in the list, then this notification is retrieved. Otherwise, the function will block if the flag is set or return immediately if it is not.

Notifications about an answer contain the `MessageId`, whether the message has been successful or not, and the body of the answer.

Notifications about an interface message contain the interface hash, the emitter PID, the `MessageId`, and the body of the message.

Notifications about a destroyed process contain the PID of the process that has been destroyed. Destroyed process messages are used for interface handlers to free resources that might have been allocated for a specific process. They are emitted only for processes that have in the past sent a message on a registered interface.

## Limit to the queue size

(note: this isn't implemented at the time of writing)

If the number of notifications in a queue is higher than a certain limit, then emitting a message on the registered interfaces will block the emitter.

# Answering messages

Messages can be answer either by a "success" answer, containing a body, or by an "error" answer, via respectively the `emit_answer` and `emit_message_error` syscalls.

If an interface handler crashes, then all of the messages that it was supposed to answer are automatically answered with an error.

After a message has been answered, the corresponding `MessageId` is no longer valid.

# Cancelling messages

The `cancel_message` syscall allows one to notify the kernel that it is no longer interested in the answer to a previously-emitted message.

The message continues to be processed normally, and the interface handler isn't made aware of the cancellation.

The difference is that answers to cancelled messages are not pushed in the queue of notifications of the sending process.

Cancelling a message makes it possible to avoid the awkward situation where one needs to somehow call `next_notification` with that message only to discard the answer immediately after. Having to call `next_notification` even when we've free'd all the other resources associated with the message can be very annoying to deal with by itself.

# Backpressure

Under the design described above, an interface handler has no way to pro-actively notify an interface user of something happening.

For example the network manager has no way to tell the program that has opened a TCP socket that a message has arrived on that socket.

Instead, the interface user must emit a message asking the interface handler for the next event that happens. When the handler wants to notify the user of something, it can do so by answering that message.

This scheme permits proper backpressure to apply. A message sender is guaranteed to not receive more answers that it has emitted messages. It prevents a possible deadlock where a handler is waiting for more room in a process's queue, while the process is waiting for more room in the handler's queue.

There is, however, a possible deadlock if A tries to send to B, B sends to C, and C sends to A, while all three queues are full. This need to be solved.
