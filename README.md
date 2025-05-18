# Orbita OS

## Русский

Orbita OS - это операционная система, написанная с нуля на языке Rust. Проект создан для обучения разработке операционных систем и исследования возможностей языка Rust в системном программировании.

### Особенности

- Написана полностью на Rust
- Поддержка графического режима
- Собственный менеджер памяти
- Драйверы устройств, написанные с нуля
- Модульная архитектура
- Поддержка многозадачности (в разработке)

### Требования

- Rust nightly
- QEMU для запуска
- Docker (опционально)
- 4GB RAM минимум
- x86_64 процессор

### Быстрый старт

```bash
# Клонирование репозитория
git clone https://github.com/yourusername/orbita
cd orbita

# Установка необходимых компонентов
rustup override set nightly
rustup component add rust-src llvm-tools-preview

# Сборка и запуск
cargo run
```

### Разработка

Для разработки рекомендуется использовать Docker:

```bash
# Запуск контейнера разработки
docker-compose up -d
docker exec -it orbita-dev bash

# Внутри контейнера
cargo build
cargo run
```

### Структура проекта

```
orbita/
├── src/           # Исходный код ядра
├── bootloader/    # Загрузчик системы
├── drivers/       # Драйверы устройств
├── docs/          # Документация
└── docker/        # Docker конфигурация
```

### Участие в разработке

1. Ознакомьтесь с файлом AGENTS.md
2. Выберите задачу из TODO.md
3. Создайте ветку feature/your-feature
4. Внесите изменения
5. Создайте Pull Request

### Документация

- [Руководство разработчика](AGENTS.md)
- [План разработки](TODO.md)
- [История изменений](docs/changelog/)

---

## English

Orbita OS is an operating system written from scratch in Rust. The project is created for learning OS development and exploring Rust capabilities in system programming.

### Features

- Written entirely in Rust
- Graphics mode support
- Custom memory manager
- Device drivers written from scratch
- Modular architecture
- Multitasking support (in development)

### Requirements

- Rust nightly
- QEMU for running
- Docker (optional)
- 4GB RAM minimum
- x86_64 processor

### Quick Start

```bash
# Clone repository
git clone https://github.com/yourusername/orbita
cd orbita

# Install required components
rustup override set nightly
rustup component add rust-src llvm-tools-preview

# Build and run
cargo run
```

### Development

Docker is recommended for development:

```bash
# Start development container
docker-compose up -d
docker exec -it orbita-dev bash

# Inside container
cargo build
cargo run
```

### Project Structure

```
orbita/
├── src/           # Kernel source code
├── bootloader/    # System bootloader
├── drivers/       # Device drivers
├── docs/          # Documentation
└── docker/        # Docker configuration
```

### Contributing

1. Read AGENTS.md file
2. Choose a task from TODO.md
3. Create feature/your-feature branch
4. Make changes
5. Create Pull Request

### Documentation

- [Developer Guide](AGENTS.md)
- [Development Plan](TODO.md)
- [Changelog](docs/changelog/)

### License

MIT License

### Authors

- Your Name

### Acknowledgments

- Rust community
- OS Dev wiki
- Phil Oppermann's blog