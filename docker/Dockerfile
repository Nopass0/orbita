FROM rust:latest

# Установка необходимых инструментов
RUN apt-get update && apt-get install -y \
    qemu-system-x86 \
    qemu-utils \
    nasm \
    xorriso \
    grub-pc-bin \
    grub-efi-amd64-bin \
    mtools \
    dosfstools \
    gdb \
    git \
    curl \
    build-essential \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Установка rustup и необходимых компонентов
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly
ENV PATH="/root/.cargo/bin:${PATH}"

# Установка дополнительных компонентов
RUN rustup component add rust-src llvm-tools-preview rustfmt clippy

# Установка cargo-bootimage
RUN cargo install bootimage

# Создание рабочей директории
WORKDIR /orbita

# Копирование исходного кода
COPY . .

# Создание точки входа
ENTRYPOINT ["/bin/bash"]