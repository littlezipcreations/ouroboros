#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

echo "=== Building userspace ELF ==="
(
    cd user-init
    cargo build --release
    python3 ../ouro-pack.py \
        target/aarch64-unknown-none/release/user-init \
        user-init.run
)

echo "=== Building kernel ==="
cargo build --release

echo "=== Build complete ==="
echo "Generated user-init.run and rebuilt the kernel."
# rm -f ouroboros.img # Leave off if you don't want to delete the image
if [[ ! -f ouroboros.img ]]; then
    echo "=== Creating 512 MiB disk image ==="
    truncate -s 512M ouroboros.img
fi
if command -v qemu-system-aarch64 >/dev/null 2>&1; then
    echo "=== Launching QEMU ==="
    qemu-system-aarch64 \
        -M virt \
        -cpu cortex-a53 \
        -m 512 \
        -nographic \
        -kernel target/aarch64-unknown-none/release/ouroboros \
        -global virtio-mmio.force-legacy=false \
        -drive if=none,file=ouroboros.img,format=raw,id=ouroDisk \
        -device virtio-blk-device,drive=ouroDisk 
else
    echo "qemu-system-aarch64 not found; skipping execution."
fi