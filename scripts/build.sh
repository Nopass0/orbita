#!/bin/bash

echo "Building Orbita OS..."

# Проверка наличия необходимых компонентов
if ! command -v cargo &> /dev/null; then
    echo "Error: cargo not found. Please install Rust."
    exit 1
fi

# Сборка проекта
cargo build --release

# Создание загрузочного образа
if command -v bootimage &> /dev/null; then
    bootimage build --target x86_64-unknown-none
else
    echo "Warning: bootimage not found. Installing..."
    cargo install bootimage
    bootimage build --target x86_64-unknown-none
fi

echo "Build complete!"