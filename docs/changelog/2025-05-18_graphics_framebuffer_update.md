# 2025-05-18 Graphics Framebuffer Update

## Изменения
- Используется реальный тип `bootloader::framebuffer::Framebuffer`
- `Framebuffer::new` принимает ссылку на буфер загрузчика
- `init` сохраняет framebuffer из `BootInfo`
- Обновлена документация модуля graphics

## Технические детали
- Информация о размере и формате копируется из `FrameBufferInfo`
- Глобальная переменная `FRAMEBUFFER` инициализируется при старте

## Тестирование
- Код проверен на компиляцию и соответствие стилю
