.PHONY: build run test clean docker-build docker-run

build:
	./scripts/build.sh

run:
	./scripts/run.sh

test:
	./scripts/test.sh

clean:
	cargo clean
	rm -rf target/

docker-build:
	./scripts/docker-build.sh

docker-run:
	docker-compose run --rm orbita-dev

install-deps:
	rustup override set nightly
	rustup component add rust-src llvm-tools-preview rustfmt clippy
	cargo install bootimage

format:
	cargo fmt

lint:
	cargo clippy

docs:
	cargo doc --open

all: format lint build test

help:
	@echo "Available commands:"
	@echo "  make build        - Build the OS"
	@echo "  make run          - Run in QEMU"
	@echo "  make test         - Run tests"
	@echo "  make clean        - Clean build artifacts"
	@echo "  make docker-build - Build using Docker"
	@echo "  make docker-run   - Run Docker container"
	@echo "  make install-deps - Install dependencies"
	@echo "  make format       - Format code"
	@echo "  make lint         - Run linter"
	@echo "  make docs         - Generate documentation"
	@echo "  make all          - Format, lint, build and test"