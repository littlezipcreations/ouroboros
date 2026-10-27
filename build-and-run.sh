
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

echo "=== Building userspace binaries ==="
(
    cd user-init

    cargo build --release --bins

    python3 ../ouro-pack.py \
        target/aarch64-unknown-none/release/user-init \
        user-init.run

    python3 ../ouro-pack.py \
        target/aarch64-unknown-none/release/hello \
        hello.run
)

echo "=== Installing binaries into OuroFS ==="

if [[ ! -f ouroboros.img ]]; then
    python3 ouro-image.py ouroboros.img \
        --create-mib 512 \
        --put /bin/user-init.run=user-init/user-init.run \
        --put /bin/hello.run=user-init/hello.run
else
    python3 ouro-image.py ouroboros.img \
        --put /bin/user-init.run=user-init/user-init.run \
        --put /bin/hello.run=user-init/hello.run
fi

echo "=== Building kernel ==="
cargo build --release

echo "=== Build complete ==="

if command -v qemu-system-aarch64 >/dev/null 2>&1; then
    echo "=== Launching QEMU ==="
    qemu-system-aarch64 \
    -M virt \
    -global virtio-mmio.force-legacy=false \
    -cpu cortex-a76 \
    -m 512M \
    -display sdl \
    -kernel target/aarch64-unknown-none/release/ouroboros \
    -drive if=none,file=ouroboros.img,format=raw,id=ouroDisk \
    -device virtio-blk-device,drive=ouroDisk \
    -device virtio-gpu-device \
    -device virtio-mouse-device \
    -nographic
else
    echo "qemu-system-aarch64 not found; skipping execution."
fi
