#!/usr/bin/env bash
# Build-pipeline stages for dm.yaml. Invoked as:
#   sh scripts/dm-build.sh esp        — assemble the FAT ESP image + publish EFI
#   sh scripts/dm-build.sh firmware   — stage the EDK2 firmware for QEMU pflash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

KERNEL_EFI="target/x86_64-unknown-uefi/release/orbita-kernel.efi"
QEMU_SHARE="${ORBITA_QEMU_SHARE:-/c/Program Files/qemu/share}"

stage="${1:-}"

case "$stage" in
  esp)
    rm -rf target/orbita-esp
    mkdir -p target/orbita-esp/EFI/BOOT dist
    cp "$KERNEL_EFI" target/orbita-esp/EFI/BOOT/BOOTX64.EFI
    printf 'fs0:\\EFI\\BOOT\\BOOTX64.EFI\r\n' > target/orbita-esp/startup.nsh
    cp "$KERNEL_EFI" dist/orbita-x86_64-uefi.efi
    ;;
  firmware)
    mkdir -p target/qemu-firmware
    cp -f "$QEMU_SHARE/edk2-x86_64-code.fd" target/qemu-firmware/edk2-x86_64-code.fd
    rm -f target/qemu-firmware/edk2-vars.fd
    cp -f "$QEMU_SHARE/edk2-i386-vars.fd" target/qemu-firmware/edk2-vars.fd
    ;;
  *)
    echo "usage: dm-build.sh {esp|firmware}" >&2
    exit 1
    ;;
esac
