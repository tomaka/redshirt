# for coreboot integration only
# normally you'll need src/Makefile and that's it.

ifeq ($(CONFIG_COREBOOT_BUILD),)
include ../../../.config
endif
libpayload_dir=../../libpayload

unexport KCONFIG_AUTOHEADER
unexport KCONFIG_AUTOCONFIG
unexport KCONFIG_DEPENDENCIES
unexport KCONFIG_SPLITCONFIG
unexport KCONFIG_TRISTATE
unexport KCONFIG_NEGATIVES

all: simpleboot

payloads/external/simpleboot/src/loader_cb.elf: simpleboot

simpleboot: $(libpayload_dir)/build/libpayload.a
	@echo "    MAKE       src/loader_cb.elf"
	@$(MAKE) -C src LIBPAYLOAD_PATH=../$(libpayload_dir) loader_cb.elf

$(libpayload_dir)/build/libpayload.a:
	@$(MAKE) -C $(libpayload_dir) defconfig
	@sed -i "s|.*CONFIG_LP_BASE_ADDRESS=.*|CONFIG_LP_BASE_ADDRESS=0x03000000|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_MULTIBOOT=y.*|# CONFIG_LP_MULTIBOOT is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_CURSES=y.*|# CONFIG_LP_CURSES is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_TINYCURSES=y.*|# CONFIG_LP_TINYCURSES is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_PDCURSES=y.*|# CONFIG_LP_PDCURSES is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*# CONFIG_LP_CBFS is not set.*|CONFIG_LP_CBFS=y|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_DEBUG_CBFS=y.*|# CONFIG_LP_DEBUG_CBFS is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_ENABLE_CBFS_FALLBACK=y.*|# CONFIG_LP_ENABLE_CBFS_FALLBACK is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_LZMA=y.*|# CONFIG_LP_LZMA is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_LZ4=y.*|# CONFIG_LP_LZ4 is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_VBOOT_LIB=y.*|# CONFIG_LP_VBOOT_LIB is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_CBMEM_CONSOLE=y.*|# CONFIG_LP_CBMEM_CONSOLE is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*# CONFIG_LP_SERIAL_CONSOLE is not set.*|CONFIG_LP_SERIAL_CONSOLE=y|" $(libpayload_dir)/.config
	@sed -i "s|.*# CONFIG_LP_VIDEO_CONSOLE is not set.*|CONFIG_LP_VIDEO_CONSOLE=y|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_VGA_VIDEO_CONSOLE=y.*|# CONFIG_LP_VGA_VIDEO_CONSOLE is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*# CONFIG_LP_COREBOOT_VIDEO_CONSOLE is not set.*|CONFIG_LP_COREBOOT_VIDEO_CONSOLE=y|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_MOUSE_CURSOR=y.*|# CONFIG_LP_MOUSE_CURSOR is not set|" $(libpayload_dir)/.config
	@sed -i "s|.*# CONFIG_LP_STORAGE is not set.*|CONFIG_LP_STORAGE=y|" $(libpayload_dir)/.config
	@sed -i "s|.*# CONFIG_LP_IGNORE_UNKNOWN_INTERRUPTS is not set.*|CONFIG_LP_IGNORE_UNKNOWN_INTERRUPTS=y|" $(libpayload_dir)/.config
	@sed -i "s|.*CONFIG_LP_DIE_ON_UNKNOWN_INTERRUPT=y.*|# CONFIG_LP_DIE_ON_UNKNOWN_INTERRUPT is not set|" $(libpayload_dir)/.config
	@# patch that extremely annoying cbfs (we can't use an empty macro either...)
	@sed -i "s|.*LOG.*;|{}|" $(libpayload_dir)/libcbfs/cbfs.c
	$(MAKE) -C $(libpayload_dir)

clean:
	$(MAKE) -C src clean

distclean: clean
	$(MAKE) -C $(libpayload_dir) clean

print-repo-info:
	echo "https://gitlab.com/bztsrc/simpleboot.git Simpleboot"

.PHONY: simpleboot libpayload clean distclean print-repo-info
