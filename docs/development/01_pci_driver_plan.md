# План разработки PCI драйвера

## Общая информация

PCI (Peripheral Component Interconnect) - это шина для подключения периферийных устройств. Драйвер PCI необходим для обнаружения и настройки всех PCI устройств в системе.

## Этапы разработки

### Этап 1: Базовые структуры и константы

1. **Создать файл**: `drivers/pci/mod.rs`
2. **Определить структуры**:
   ```rust
   #[repr(C)]
   pub struct PciDeviceId {
       vendor_id: u16,
       device_id: u16,
   }
   
   pub struct PciDevice {
       bus: u8,
       device: u8,
       function: u8,
       vendor_id: u16,
       device_id: u16,
       class_code: u8,
       subclass: u8,
       header_type: u8,
   }
   ```

3. **Константы конфигурационного пространства**:
   ```rust
   const CONFIG_ADDRESS: u16 = 0xCF8;
   const CONFIG_DATA: u16 = 0xCFC;
   
   const PCI_VENDOR_ID: u8 = 0x00;
   const PCI_DEVICE_ID: u8 = 0x02;
   const PCI_COMMAND: u8 = 0x04;
   const PCI_STATUS: u8 = 0x06;
   const PCI_CLASS_REVISION: u8 = 0x08;
   const PCI_HEADER_TYPE: u8 = 0x0E;
   ```

### Этап 2: Чтение конфигурационного пространства

1. **Функция чтения**:
   ```rust
   pub fn read_config_word(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
       let address = ((bus as u32) << 16) | 
                    ((device as u32) << 11) | 
                    ((function as u32) << 8) | 
                    ((offset as u32) & 0xFC) | 
                    0x80000000;
       
       unsafe {
           let mut addr_port = Port::<u32>::new(CONFIG_ADDRESS);
           let mut data_port = Port::<u32>::new(CONFIG_DATA);
           
           addr_port.write(address);
           let data = data_port.read();
           
           ((data >> ((offset & 2) * 8)) & 0xFFFF) as u16
       }
   }
   ```

### Этап 3: Сканирование шины

1. **Проверка существования устройства**:
   ```rust
   fn device_exists(bus: u8, device: u8, function: u8) -> bool {
       let vendor_id = read_config_word(bus, device, function, PCI_VENDOR_ID);
       vendor_id != 0xFFFF
   }
   ```

2. **Сканирование всех устройств**:
   ```rust
   pub fn scan_bus() -> Vec<PciDevice> {
       let mut devices = Vec::new();
       
       for bus in 0..256 {
           for device in 0..32 {
               for function in 0..8 {
                   if device_exists(bus, device, function) {
                       let device_info = read_device_info(bus, device, function);
                       devices.push(device_info);
                   }
               }
           }
       }
       
       devices
   }
   ```

### Этап 4: Чтение информации об устройстве

1. **Полная информация об устройстве**:
   ```rust
   fn read_device_info(bus: u8, device: u8, function: u8) -> PciDevice {
       let vendor_id = read_config_word(bus, device, function, 0x00);
       let device_id = read_config_word(bus, device, function, 0x02);
       let class_info = read_config_word(bus, device, function, 0x0A);
       let header_type = read_config_byte(bus, device, function, 0x0E);
       
       PciDevice {
           bus,
           device, 
           function,
           vendor_id,
           device_id,
           class_code: (class_info >> 8) as u8,
           subclass: (class_info & 0xFF) as u8,
           header_type,
       }
   }
   ```

### Этап 5: База данных устройств

1. **Создать базу известных устройств**:
   ```rust
   const KNOWN_DEVICES: &[(u16, u16, &str)] = &[
       (0x8086, 0x1237, "Intel 82441FX Chipset"),
       (0x8086, 0x7000, "Intel PIIX3 ISA Bridge"),
       (0x8086, 0x7113, "Intel PIIX4 ACPI"),
       (0x10EC, 0x8139, "Realtek RTL8139"),
   ];
   ```

### Этап 6: Настройка устройств

1. **Включение Bus Mastering**:
   ```rust
   pub fn enable_bus_mastering(device: &PciDevice) {
       let command = read_config_word(device.bus, device.device, device.function, PCI_COMMAND);
       write_config_word(device.bus, device.device, device.function, PCI_COMMAND, command | 0x04);
   }
   ```

2. **Чтение BAR (Base Address Registers)**:
   ```rust
   pub fn read_bar(device: &PciDevice, bar_index: u8) -> u32 {
       let offset = 0x10 + (bar_index * 4);
       read_config_dword(device.bus, device.device, device.function, offset)
   }
   ```

### Этап 7: Поддержка прерываний

1. **Чтение IRQ**:
   ```rust
   pub fn get_interrupt_line(device: &PciDevice) -> u8 {
       read_config_byte(device.bus, device.device, device.function, 0x3C)
   }
   ```

### Этап 8: Интеграция с ядром

1. **Инициализация при старте системы**:
   ```rust
   pub fn init() {
       serial_println!("Initializing PCI bus...");
       
       let devices = scan_bus();
       serial_println!("Found {} PCI devices:", devices.len());
       
       for device in &devices {
           serial_println!("  {:04x}:{:04x} - Bus {}, Device {}, Function {}",
               device.vendor_id, device.device_id,
               device.bus, device.device, device.function);
       }
   }
   ```

## Проверочный список для AI-агентов

Перед завершением каждого этапа проверьте:

1. ✓ Все структуры имеют правильное выравнивание (`#[repr(C)]`)
2. ✓ Все unsafe блоки документированы
3. ✓ Проверка границ при работе с портами
4. ✓ Обработка ошибок (устройство может не существовать)
5. ✓ Нет паники в драйвере
6. ✓ Код соответствует no_std
7. ✓ Добавлены логи для отладки

## Тестирование

1. Создать модульные тесты для каждой функции
2. Проверить на эмуляторе QEMU
3. Логировать найденные устройства
4. Сравнить с выводом lspci в Linux

## Зависимости от других модулей

- Требует работающей системы портов ввода-вывода
- Использует serial для вывода отладки
- Будет использоваться драйверами устройств