# 2025-05-19 Network Drivers

## Изменения
- Добавлены начальные реализации сетевых драйверов RTL8139 и Intel E1000
- Реализованы функции инициализации, отправки и приёма пакетов
- Добавлена обработка прерываний для обоих драйверов
- Создан модуль `drivers/net` для сетевых устройств

## Технические детали
- RTL8139 использует буферы передачи и приёма в памяти и работу с портами ввода-вывода
- Драйвер E1000 настраивает кольцевые буферы дескрипторов и использует DMA через MMIO регистры
- Оба драйвера написаны в стиле no_std и не требуют внешних зависимостей

## Тестирование
- Код отформатирован и проверен на компиляцию
- Запущены модульные тесты для конструкций драйверов
