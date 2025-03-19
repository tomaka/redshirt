ifneq ($(LIBPAYLOAD_PATH),)
include $(LIBPAYLOAD_PATH)/build/xcompile
ifneq ($(CONFIG_LP_ARCH_ARM64),)
STRIP=$(STRIP_arm64)
else
STRIP=$(STRIP_x86_32)
endif
ifneq ($(DEBUG),)
CFLAGS+=-DDEBUG=1
endif
CFLAGS+=-fshort-wchar -fno-strict-aliasing -fno-stack-protector -fno-stack-check -mno-red-zone -Wall -Wextra -Werror
else
PREFIX?=usr/
INSTDIR?=$(DESTDIR:/=)/$(PREFIX:/=)
ARCH=$(shell uname -m)
TMP=$(ARCH:x86_64=amd64)
TMP2=$(TMP:armv7l=armhf)
DEBARCH=$(TMP2:aarch64=arm64)
VERSION=$(shell grep -m 1 sbver simpleboot.c|cut -d '"' -f 2)
CFLAGS=-fshort-wchar -fno-strict-aliasing -ffreestanding -fno-stack-protector -fno-stack-check -mno-red-zone \
  -nostdlib -Wall -Wextra -Werror -Wno-long-long
endif

all: simpleboot

romfoss_x86.bin: romfoss_x86.asm
	fasm romfoss_x86.asm romfoss_x86.bin

rombios_x86.bin: rombios_x86.asm
	fasm rombios_x86.asm rombios_x86.bin

cdemu_x86.bin: cdemu_x86.asm
	fasm cdemu_x86.asm cdemu_x86.bin

boot_x86.bin: boot_x86.asm
	fasm boot_x86.asm boot_x86.bin

loader_x86.efi: loader_x86.c loader.h ../simpleboot.h
	clang --target=x86_64-pc-win32-coff $(CFLAGS) -Wframe-larger-than=512 -c loader_x86.c -o loader_x86.o
	lld -flavor link -subsystem:efi_application -Brepro -nodefaultlib -dll -dynamicbase -base:0x7000 -entry:_start loader_x86.o -out:loader_x86.efi
	@rm loader_x86.o loader_x86.lib 2>/dev/null

loader_rpi.bin: loader_rpi.c loader.h ../simpleboot.h
	clang --target=aarch64-elf $(CFLAGS) -Wl,-Ttext=0x80000 -Wl,--omagic loader_rpi.c -o loader_rpi.o

loader_cb.elf: loader_cb.c loader.h ../simpleboot.h
	$(LIBPAYLOAD_PATH)/bin/lpgcc $(CFLAGS) -o loader_cb.elf loader_cb.c
ifeq ($(DEBUG),)
	@$(STRIP) loader_cb.elf
endif

data.h: romfoss_x86.bin rombios_x86.bin cdemu_x86.bin boot_x86.bin loader_x86.efi loader_rpi.bin
	@$(CC) misc/bin2h.c -o bin2h
	@./bin2h romfoss_x86.bin rombios_x86.bin cdemu_x86.bin boot_x86.bin loader_x86.efi loader_rpi.o
	@rm bin2h
	@touch data.h

simpleboot: loader.h simpleboot.c
	@test -f data.h || make --no-print-directory data.h
	$(CC) -ansi -Wall -Wextra simpleboot.c -o simpleboot
	@strip simpleboot 2>/dev/null || true
	@strip simpleboot.exe 2>/dev/null || true

install: simpleboot
ifneq ("$(INSTDIR)","")
	install -D -m 755 -o root -g root simpleboot -t $(INSTDIR)/bin
	@mkdir -p $(INSTDIR)/share/man/man1 2>/dev/null || true
	cp misc/simpleboot.1.gz $(INSTDIR)/share/man/man1
	cp ../simpleboot.h $(INSTDIR)/include
else
	@echo "INSTDIR variable not set, not installing."
	@false
endif

deb:
	@rm -rf DEBIAN usr 2>/dev/null || true
	@mkdir -p DEBIAN usr/bin usr/include usr/share/man/man1
	@cp simpleboot usr/bin
	@cp ../simpleboot.h usr/include
	@cp misc/simpleboot.1.gz usr/share/man/man1
	@cat misc/deb_control | sed s/ARCH/$(DEBARCH)/g | sed s/VERSION/$(VERSION)/g | sed s/SIZE/`du -s usr|cut -f 1`/g >DEBIAN/control
	@md5sum `find usr -type f` >DEBIAN/md5sums
	@cp ../LICENSE DEBIAN/copyright
	@echo "2.0" >debian-binary
	@tar -czvf data.tar.gz usr
	@tar -C DEBIAN -czvf control.tar.gz control copyright md5sums
	ar r ../distrib/simpleboot_$(VERSION)-$(DEBARCH).deb debian-binary control.tar.gz data.tar.gz
	@rm -rf debian-binary control.tar.gz data.tar.gz DEBIAN usr

clean:
	rm *.bin loader_*.o loader_*.lib loader_*.efi loader_*.elf simpleboot 2>/dev/null || true

distclean: clean
	rm data.h 2>/dev/null || true
