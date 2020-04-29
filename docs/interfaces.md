# Interfaces

This document contains a list of interfaces that exist.

## Does this warrant an interface?

Interfaces are fundamentally related to code re-use and synchronizing components.

It is for example theoretically possible for an HTTP server to communicate directly with the networking card of the machine and to directly issue read commands to hard-disk drives. This can be done by statically linking the server to device drivers.

In practice, however, doing so has two big drawbacks:

- Even if we suppose that the HTTP server supports all hardware that has ever existed, new incompatible hardware gets constantly released. As such, the HTTP server would stop working on newer machines unless it gets updated.
- Multiple programs trying to access the same hardware will conflict with each other.

Because of these two drawbacks, the hardware should be abstracted behind an interface. The first drawback is solved because the interface user and the interface handler can both be updated separately, and the second drawback is solved by having multiple interface users communicate with the same interface handler.

If, however, we take the example of font rendering (turning font files into bitmaps), none of these two drawbacks apply. Consequently, there shouldn't be an interface dedicated to font rendering.

## Updates

It sometimes happens that interfaces need an update. Since interfaces are referred to by a hash and not by a number, this can be done by modifying the hash of the updated interface.

Modifying the hash, however, would break all the existing software that uses this interface.

In order to solve this, there are two solutions:

- Interface handlers can register both the old and the new hash.
- There can be a program can acts as a conversion layer between the old and the new hash.

## List of existing interfaces

And now for a list.

- `framebuffer`: Drawing a RGB buffer.
- `hardware`: Accessing physical memory.
- `interface`: Registering interfaces.
- `kernel-log`: Indicating to the kernel how to write its logs.
- `loader`: Loading resources from the DHT.
- `log`: Sending out logs for the user.
- `pci`: Accessing PCI devices (if any).
- `random`: Generating random values.
- `system-time`: Managing the real time clock.
- `tcp`: TCP/IP sockets.
- `time`: Getting the value of the monotonic clock and waiting.
