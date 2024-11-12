#!/bin/sh

export AX_ARCH=riscv64
export AX_PLATFORM=riscv64-qemu-virt
export AX_SMP=1
export AX_MODE=release

TEST_NAME=rt_axlog2
BUILD_DIR=./target/riscv64gc-unknown-none-elf/release/
LD_SCRIPT=$BUILD_DIR/linker_riscv64-qemu-virt.lds
export RUSTFLAGS="-C link-arg=-T$LD_SCRIPT -C link-arg=-no-pie"

cargo build --target riscv64gc-unknown-none-elf --release --manifest-path $TEST_NAME/Cargo.toml

rust-objcopy --binary-architecture=riscv64 $BUILD_DIR/$TEST_NAME --strip-all -O binary $BUILD_DIR/$TEST_NAME.bin

qemu-system-riscv64 -m 128M -smp 1 -machine virt -bios default -kernel $BUILD_DIR/$TEST_NAME.bin -append "init=/sbin/init" -nographic \
    -D qemu.log -d in_asm,int,mmu,pcall,cpu_reset,guest_errors