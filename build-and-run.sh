#!/bin/bash
set -e

cd "$(dirname "$0")"

echo "=== Building userspace ==="

cd user-init

rustc \
    --target aarch64-unknown-none \
    -C opt-level=2 \
    -C panic=abort \
    -C relocation-model=static \
    -C link-arg=-Tlinker.ld \
    -o user-init.elf \
    src/main.rs

objcopy \
    -O binary \
    user-init.elf \
    user-init.bin

cd ..

echo "=== Building kernel ==="

cargo build --release

echo "=== Running Ouroboros ==="

qemu-system-aarch64   -machine virt,gic-version=2   -cpu cortex-a76   -m 512M   -nographic  -kernel target/aarch64-unknown-none/release/ouroboros -d int -D qemu.log