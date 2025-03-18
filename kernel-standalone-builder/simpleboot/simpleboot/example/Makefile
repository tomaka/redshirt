SIMPLEBOOT=../src/simpleboot
KERNEL?=mykernel
OVMF?=/usr/share/qemu/bios-TianoCoreEFI.bin
# compile a "Linux" kernel
#LINUX=1
# compile a Multiboot COFF/PE kernel (otherwise ELF)
#PE=1
# compile a Multiboot 32-bit kernel
#MB32=1
# compile a Multiboot 64-bit kernel for Raspberry Pi
#RPI=1
# test Symmetric MultiProcessor support
#SMP=1
# create El Torito hybrid disk image
#CDROM=1

CFLAGS?=-Wall -ffreestanding -fno-stack-protector -nostdlib -static -I..
ifeq ($(MB32),)
GCCFLG=-m64
CLANGFLG=--target=x86_64-pc-win32-coff
else
GCCFLG=-m32
CLANGFLG=--target=i386-pc-win32-coff
endif
ifneq ($(RPI),)
SBFLAGS=-s 64
endif
ifeq ($(HI),)
BASE=0x10
else
BASE=0xffffffffffe0
endif
ifneq ($(ROM),)
SBFLAGS+=-r
QEMUFLG=-option-rom sb_bios.rom
endif
ifneq ($(CDROM),)
SBFLAGS+=-e
endif

all: disk.img

# compile the kernel
boot/$(KERNEL): kernel.c linux.c ../simpleboot.h
	@mkdir -p boot
ifneq ($(RPI),)
ifeq ($(PE),)
	clang --target=aarch64-elf -fno-strict-aliasing $(CFLAGS) -Wl,-Ttext=$(BASE)0000 -Wl,--omagic kernel.c -o boot/$(KERNEL)
else
	clang --target=aarch64-win32-coff -fno-strict-aliasing $(CFLAGS) -c kernel.c -o kernel.o
	lld -flavor link -subsystem:console -nodefaultlib -base:$(BASE)0000 -entry:_start kernel.o -out:boot/$(KERNEL)
	@rm kernel.o
endif
else
ifeq ($(LINUX),)
ifeq ($(PE),)
	gcc $(GCCFLG) $(CFLAGS) -Wl,-Ttext=$(BASE)1000 kernel.c -o boot/$(KERNEL)
else
	clang $(CLANGFLG) -fno-strict-aliasing $(CFLAGS) -c kernel.c -o kernel.o
	lld -flavor link -subsystem:console -nodefaultlib -base:$(BASE)0000 -entry:_start kernel.o -out:boot/$(KERNEL)
	@rm kernel.o
endif
else
	gcc $(CFLAGS) -Wl,-Ttext=$(BASE)0000 linux.c -o kernel.o
	strip --remove-section=.note.gnu.property --remove-section=.note.gnu.build-id --remove-section=.comment kernel.o
	objcopy -O binary kernel.o boot/$(KERNEL)
	@rm kernel.o
endif
endif

# compile the simpleboot image generator
$(SIMPLEBOOT):
	@make --no-print-directory -C ../src simpleboot

# generate a bootable disk image with our kernel in it
disk.img: boot/$(KERNEL) $(SIMPLEBOOT)
ifneq ($(SMP),)
	@echo "multicore" > boot/simpleboot.cfg
endif
	$(SIMPLEBOOT) $(SBFLAGS) -vv -k "$(KERNEL)" boot disk.img

# mount the generated disk (requires root priviledge)
mnt: disk.img
	@mkdir -p mnt || true
	sudo mount -o loop,offset=1048576,user,umask=000 disk.img mnt

# test in a UEFI machine (as EFI CDROM)
eficdrom: disk.img
	qemu-system-x86_64 -bios $(OVMF) -m 256 -cdrom disk.img -serial stdio
	@printf '\033[0m'

# test in a UEFI machine
efi: disk.img
	qemu-system-x86_64 -bios $(OVMF) -m 256 -drive file=disk.img,format=raw -serial stdio -smp cpus=2
	@printf '\033[0m'

# test in a BIOS machine (as BIOS CDROM)
cdrom: disk.img
	qemu-system-x86_64 -m 256 -cdrom disk.img -serial stdio

# test in a BIOS machine
qemu: disk.img
ifeq ($(RPI),)
	qemu-system-x86_64 $(QEMUFLG) -m 256 -drive file=disk.img,format=raw -serial stdio -smp cpus=2
else
	@make --no-print-directory rpi
endif

# test with another, non-qemu VM too
bochs: disk.img
	bochs -f bochs.rc -q

# test in a Raspberry Pi 3B
rpi: disk.img
	@mkdir mnt && sudo mount -o loop,offset=1048576,user,umask=000 disk.img mnt
	qemu-system-aarch64 -M raspi3b -kernel mnt/KERNEL8.IMG -drive file=disk.img,if=sd,format=raw -serial stdio || true
	@sudo umount mnt && rmdir mnt

# test with coreboot
cb: disk.img
	qemu-system-x86_64 -bios ../distrib/coreboot.rom -drive file=disk.img,format=raw -serial stdio -smp cpus=2

# debug in a UEFI machine (stop before executing)
efidbg: disk.img
	qemu-system-x86_64 -s -S -d int -bios $(OVMF) -m 256 -drive file=disk.img,format=raw -serial stdio -smp cpus=2 || true
	@printf '\033[0m'

# debug in a BIOS machine (stop before executing)
qemudbg: disk.img
	qemu-system-x86_64 -s -S -d int -m 256 -drive file=disk.img,format=raw -serial stdio -smp cpus=2

# start the debugger and attach it to the stopped VM
gdb:
	gdb -w -x gdb.rc || true
	pkill qemu

clean:
	@(test -d mnt && (sudo umount mnt || true) && rmdir mnt) || true
	rm -rf boot disk.img kernel.o *.rom 2>/dev/null || true

distclean: clean
