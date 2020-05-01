This document contains information about interfaces, plus a list of interfaces that exist.

# Interfaces designing

## Does this warrant an interface?

Interfaces are fundamentally related to code re-use and synchronizing components.

It is for example theoretically possible for an HTTP server to communicate directly with the networking card of the machine and to directly issue read commands to hard-disk drives. This can be done by statically linking the server to device drivers.

In practice, however, doing so has two big drawbacks:

- Even if we suppose that the HTTP server supports all hardware that has ever existed, new incompatible hardware gets constantly released. As such, the HTTP server would stop working on newer machines unless it gets updated.
- Multiple programs trying to access the same hardware will conflict with each other.

Because of these two drawbacks, the hardware should be abstracted behind an interface. The first drawback is solved because the interface user and the interface handler can both be updated separately, and the second drawback is solved by having multiple interface users communicate with the same interface handler.

If, however, we take the example of font rendering (turning font files into bitmaps), none of these two drawbacks apply. Consequently, there shouldn't be an interface dedicated to font rendering.

## Updating an interface

It sometimes happens that interfaces need a modification. Interfaces are considered immutable, and modifying an interface can in practice be done only by creating a different interface (that closely resembles the old one) with a different hash.

Simply switching everything to the new interface, however, would break all the existing softwares that use the former interface.

In order to remedy to this, there are two solutions:

- Interface handlers can register both the old and the new hash.
- There can be a program can acts as a conversion layer between the old and the new hash, accepting messages from the old interface, translating them, and re-emitting them.

## Avoiding cross-interface concerns

The list of interfaces will never be set in marble, and each version of an interface is in principle unrelated to the previous versions of that interface. As such, an interface must **never** depend on another interface.

For example, in the Linux world, creating an OpenGL context requires passing an X11 display. This means that a hypothetical equivalent redshirt OpenGL interface would depend on the equivalent redshirt X11 interface. This is forbidden.

If an equivalent of OpenGL+X11 had to be designed, one could create an interface that combines all of OpenGL and X11 together.

There is no limit to the size or the number of messages in an interface. "Elegance", "minimalism" or "code reuse" are **not** valid reasons to split an interface in multiple parts.

# Kernel-handled interfaces

Some interfaces, such as the `interface` interface, must be handled by the kernel. There is no other way that would lead to a correct implementation.

# Determining an interface hash

Undesigned at the moment.

# List of existing or planned interfaces

And now for a list. This list is most likely not up-to-date.

This list contains human-friendly names, but remember that interfaces are defined by their hash.

- `audio-playback`: Playing sounds.
- `device-tree`: Accessing hardware devices described by a DeviceTree (if any).
- `disks`: Registering disks potentially containing files.
- `ethernet`: Registering Ethernet interfaces.
- `files`: Opening/reading/writing files on a specific disk.
- `framebuffer`: Drawing a RGB buffer to an unspecified location.
- `hardware`: Accessing physical memory. Note: will most likely disappear to be superceded by `pci` and `device-tree`.
- `hid`: Accessing human-interface devices (keyboard, mouse, joysticks, etc.).
- `interface`: Registering interfaces.
- `kernel-debug`: Gathering information and statistics about the kernel. Supposed to be shown to users.
- `kernel-log`: Indicating to the kernel how to write its logs.
- `loader`: Loading content-addressed resources.
- `log`: Sending out logs destined to the user.
- `pci`: Accessing PCI devices (if any): reading/writing their memory-mapped memory/registers and waiting for interrupts.
- `random`: Generating random values.
- `system-time`: Managing the real time clock.
- `tcp`: TCP/IP sockets.
- `time`: Getting the value of the monotonic clock and waiting.
- `udp`: UDP packets.
- `usb`: Accessing USB devices (if any).
- `webgpu`: Issuing WebGPU draw calls to an unspecified location.
