# План разработки драйвера таймера (PIT/APIC)

## Общая информация

Системный таймер необходим для:
- Планировщика задач
- Измерения времени
- Создания задержек
- Генерации периодических прерываний

Будем реализовывать:
1. PIT (Programmable Interval Timer) - базовый таймер
2. APIC Timer - более современный таймер (опционально)

## Этапы разработки

### Этап 1: PIT драйвер

1. **Создать файл**: `drivers/timer/pit.rs`

2. **Константы PIT**:
   ```rust
   const PIT_CHANNEL0: u16 = 0x40;
   const PIT_CHANNEL1: u16 = 0x41;
   const PIT_CHANNEL2: u16 = 0x42;
   const PIT_COMMAND: u16 = 0x43;
   
   const PIT_FREQUENCY: u32 = 1193182; // Hz
   
   // Команды
   const PIT_CHANNEL0_SELECT: u8 = 0x00;
   const PIT_ACCESS_MODE_LOHI: u8 = 0x30;
   const PIT_OPERATING_MODE_RATE: u8 = 0x04;
   ```

3. **Структура драйвера**:
   ```rust
   pub struct Pit {
       frequency: u32,
       tick_count: AtomicUsize,
   }
   
   impl Pit {
       pub const fn new() -> Self {
           Self {
               frequency: 100, // 100 Hz по умолчанию
               tick_count: AtomicUsize::new(0),
           }
       }
   }
   ```

### Этап 2: Инициализация PIT

1. **Функция инициализации**:
   ```rust
   pub fn init(&self, frequency: u32) {
       let divisor = PIT_FREQUENCY / frequency;
       
       unsafe {
           // Отправляем команду
           let mut cmd_port = Port::<u8>::new(PIT_COMMAND);
           cmd_port.write(PIT_CHANNEL0_SELECT | PIT_ACCESS_MODE_LOHI | PIT_OPERATING_MODE_RATE);
           
           // Устанавливаем делитель частоты
           let mut channel0 = Port::<u8>::new(PIT_CHANNEL0);
           channel0.write((divisor & 0xFF) as u8);
           channel0.write((divisor >> 8) as u8);
       }
       
       self.frequency = frequency;
   }
   ```

### Этап 3: Обработчик прерываний

1. **Обработчик тиков**:
   ```rust
   pub extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
       // Увеличиваем счетчик тиков
       TIMER.tick_count.fetch_add(1, Ordering::Relaxed);
       
       // Вызываем обработчики таймера
       if let Some(handler) = TIMER_HANDLERS.lock().get_mut() {
           handler();
       }
       
       // Подтверждаем прерывание
       unsafe {
           PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
       }
   }
   ```

### Этап 4: API для работы с временем

1. **Получение времени**:
   ```rust
   pub fn ticks() -> usize {
       TIMER.tick_count.load(Ordering::Relaxed)
   }
   
   pub fn uptime_ms() -> u64 {
       let ticks = ticks();
       let frequency = TIMER.frequency as u64;
       (ticks as u64 * 1000) / frequency
   }
   ```

2. **Задержки**:
   ```rust
   pub fn sleep(milliseconds: u64) {
       let start = uptime_ms();
       let end = start + milliseconds;
       
       while uptime_ms() < end {
           x86_64::instructions::hlt();
       }
   }
   
   pub fn busy_wait_us(microseconds: u64) {
       let cycles = microseconds * (cpu_frequency_mhz() / 1000);
       let start = rdtsc();
       
       while rdtsc() - start < cycles {
           core::hint::spin_loop();
       }
   }
   ```

### Этап 5: Высокоточный таймер (TSC)

1. **Чтение TSC**:
   ```rust
   pub fn rdtsc() -> u64 {
       unsafe {
           let lo: u32;
           let hi: u32;
           asm!("rdtsc", out("eax") lo, out("edx") hi);
           ((hi as u64) << 32) | (lo as u64)
       }
   }
   ```

2. **Калибровка TSC**:
   ```rust
   pub fn calibrate_tsc() -> u64 {
       let pit_ticks = 10; // 100ms при 100Hz
       let start_tsc = rdtsc();
       let start_pit = ticks();
       
       while ticks() - start_pit < pit_ticks {
           core::hint::spin_loop();
       }
       
       let end_tsc = rdtsc();
       let elapsed_tsc = end_tsc - start_tsc;
       
       // TSC частота = (TSC тики / PIT тики) * PIT частота
       (elapsed_tsc * TIMER.frequency as u64) / pit_ticks as u64
   }
   ```

### Этап 6: APIC Timer (опционально)

1. **Структура APIC Timer**:
   ```rust
   pub struct ApicTimer {
       base_addr: VirtAddr,
       frequency: u32,
   }
   
   const APIC_TIMER_INIT_COUNT: u32 = 0x380;
   const APIC_TIMER_CURRENT_COUNT: u32 = 0x390;
   const APIC_TIMER_DIVIDE_CONFIG: u32 = 0x3E0;
   const APIC_LVT_TIMER: u32 = 0x320;
   ```

2. **Инициализация APIC Timer**:
   ```rust
   pub fn init_apic_timer() {
       unsafe {
           // Устанавливаем делитель
           write_apic_register(APIC_TIMER_DIVIDE_CONFIG, 0x3); // Делить на 16
           
           // Настраиваем LVT Timer
           write_apic_register(APIC_LVT_TIMER, 32 | (1 << 17)); // Периодический режим
           
           // Устанавливаем начальное значение счетчика
           write_apic_register(APIC_TIMER_INIT_COUNT, 0xFFFFFFFF);
       }
   }
   ```

### Этап 7: Система обратных вызовов

1. **Регистрация обработчиков**:
   ```rust
   pub struct TimerCallback {
       callback: fn(),
       interval_ms: u64,
       next_run: u64,
   }
   
   static TIMER_CALLBACKS: Mutex<Vec<TimerCallback>> = Mutex::new(Vec::new());
   
   pub fn register_timer(callback: fn(), interval_ms: u64) {
       let mut callbacks = TIMER_CALLBACKS.lock();
       callbacks.push(TimerCallback {
           callback,
           interval_ms,
           next_run: uptime_ms() + interval_ms,
       });
   }
   ```

2. **Обработка обратных вызовов**:
   ```rust
   fn process_timer_callbacks() {
       let current_time = uptime_ms();
       let mut callbacks = TIMER_CALLBACKS.lock();
       
       for callback in callbacks.iter_mut() {
           if current_time >= callback.next_run {
               (callback.callback)();
               callback.next_run = current_time + callback.interval_ms;
           }
       }
   }
   ```

### Этап 8: Интеграция с ядром

1. **Глобальная инициализация**:
   ```rust
   pub fn init_timers() {
       // Инициализируем PIT
       PIT.init(100); // 100 Hz
       
       // Калибруем TSC
       let tsc_freq = calibrate_tsc();
       serial_println!("TSC frequency: {} MHz", tsc_freq / 1_000_000);
       
       // Регистрируем обработчик прерываний
       interrupts::register_handler(InterruptIndex::Timer, timer_interrupt_handler);
       
       // Если доступен APIC, инициализируем его
       if cpu_has_apic() {
           init_apic_timer();
       }
   }
   ```

## Проверочный список для AI-агентов

Перед завершением проверьте:

1. ✓ Атомарные операции для счетчиков
2. ✓ Правильная обработка прерываний  
3. ✓ Защита от переполнения счетчиков
4. ✓ Корректная калибровка TSC
5. ✓ Обработка случая отсутствия APIC
6. ✓ Документация единиц измерения (мс, мкс, тики)
7. ✓ Потокобезопасность всех операций

## Тестирование

1. Проверка точности задержек
2. Тест стабильности частоты
3. Проверка обратных вызовов
4. Стресс-тест с множеством таймеров

## Зависимости

- Требует настроенных прерываний
- Использует PIC/APIC
- Необходим для планировщика задач