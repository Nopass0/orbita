#!/bin/bash

echo "Building Orbita OS in Docker..."

# Сборка Docker образа
docker-compose build

# Запуск контейнера и сборка
docker-compose run --rm orbita-dev cargo build --release

echo "Docker build complete!"