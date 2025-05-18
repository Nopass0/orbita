#!/bin/bash

echo "Starting Orbita OS in QEMU..."

# Проверка наличия QEMU
if ! command -v qemu-system-x86_64 &> /dev/null; then
    echo "Error: qemu-system-x86_64 not found. Please install QEMU."
    exit 1
fi

# Параметры QEMU
QEMU_ARGS="-drive format=raw,file=target/x86_64-unknown-none/release/bootimage-orbita.bin"
QEMU_ARGS="$QEMU_ARGS -m 512M"
QEMU_ARGS="$QEMU_ARGS -cpu qemu64,+sse,+sse2"
QEMU_ARGS="$QEMU_ARGS -serial stdio"
QEMU_ARGS="$QEMU_ARGS -vga std"
QEMU_ARGS="$QEMU_ARGS -display gtk"

# Отладочные параметры
if [ "$1" == "debug" ]; then
    QEMU_ARGS="$QEMU_ARGS -s -S"
    echo "Starting in debug mode. Connect GDB to localhost:1234"
fi

# Запуск
qemu-system-x86_64 $QEMU_ARGS