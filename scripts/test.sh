#!/bin/bash

echo "Running Orbita OS tests..."

# Запуск unit тестов
cargo test

# Запуск integration тестов
cargo test --test '*'

echo "Tests complete!"