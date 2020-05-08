It is generally desirable to limit as much as possible the rights that a program has.

In redshirt, there is no system-wide rights management. Instead, each interface holds a list of which program can have access to the capabilities that they provide.

For example, it is the network manager program that holds a list of the programs that are allowed to open TCP connections.

By default, all programs should be banned from using anything, and must instead be whitelisted. For example, when you create a new process, it is by default prevented from using the TCP connection. One must then send a message to the TCP interface to inform the network manager that the newly-created process is allowed to open TCP connections.
