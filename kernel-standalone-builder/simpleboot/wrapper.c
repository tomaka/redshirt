#include "simpleboot/src/simpleboot.c"

// Note that unfortunately simpleboot calls `exit(1)` whenever something problematic happens
// instead of doing proper error management, but we just cope with this.
extern int simpleboot_wrapper(int argc, char **argv) {
    return simpleboot_main(argc, argv);
}
