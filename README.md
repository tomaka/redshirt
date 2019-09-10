Experiment to build some kind of operating-system-like environment where executables are all in
WASM and are loaded from some IPFS-like decentralized network.

# General idea

- This is an operating-system-like environment, but it could be seen as similar to a web browser
  or something similar.

- If it ever becomes a real OS, everything would be run in ring 0. Isolation is guaranteed by the
  WASM interpreter, and no hardware capability is required.

- Programs are referred to by their hash, not by a file name. For example you don't tell the OS
  "execute /usr/bin/foo". Instead you say "execute A45d9a21c3a7". The WASM binary, if it doesn't
  exist locally, is fetched from IPFS or something similar.

- A program can register itself as a provider of an interface. Interfaces are referred by hash as
  well. Only one process can be a provider of an interface at any given point in time.

- The import table of a WASM module can contain functions from interfaces. The "kernel" will link
  them to the process that provides that interface.

- Very few things are built in. No built-in concepts such as networking or files. Almost
  everything is done through interfaces.

- The lowest-level interfaces are provided by the OS itself. On desktop, the provided interfaces
  would for example be TCP/IP, UDP, file system, etc. On bare metal, the provided interfaces would
  be for example "interrupt handler manager", "PCI", etc. and the provider for TCP/IP would be a
  regular WASM process built on top of the PCI, Ethernet, etc. interfaces.

- Interfaces are referred to by a hash built in a determinstic way based on the name of the
  interface and its functions' names and signatures. There's no versioning system.

- The programs loader is itself just an interface provider.
