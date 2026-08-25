use orbita_arch_x86_64::io::Port;

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const COMMAND_PORT: u16 = 0x64;

const STATUS_OUTPUT_FULL: u8 = 1 << 0;
const STATUS_INPUT_FULL: u8 = 1 << 1;
const CONFIG_FIRST_PORT_INTERRUPT: u8 = 1 << 0;
const CONFIG_FIRST_PORT_CLOCK_DISABLED: u8 = 1 << 4;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Ps2Status {
    Present { config_byte: u8 },
    NotResponding,
}

pub fn probe_controller() -> Ps2Status {
    let mut status = Port::<u8>::new(STATUS_PORT);
    let mut command = Port::<u8>::new(COMMAND_PORT);
    let mut data = Port::<u8>::new(DATA_PORT);

    unsafe {
        wait_for_input_clear(&mut status);
        command.write(0xAD);
        wait_for_input_clear(&mut status);
        command.write(0xA7);

        flush_output(&mut status, &mut data);

        wait_for_input_clear(&mut status);
        command.write(0x20);
        if let Some(config_byte) = wait_and_read(&mut status, &mut data) {
            restore_first_port(&mut status, &mut command, &mut data, config_byte);
            Ps2Status::Present { config_byte }
        } else {
            Ps2Status::NotResponding
        }
    }
}

pub fn initialize_keyboard() -> bool {
    let mut status = Port::<u8>::new(STATUS_PORT);
    let mut command = Port::<u8>::new(COMMAND_PORT);
    let mut data = Port::<u8>::new(DATA_PORT);

    unsafe {
        flush_output(&mut status, &mut data);
        wait_for_input_clear(&mut status);
        command.write(0x20);
        let Some(config_byte) = wait_and_read(&mut status, &mut data) else {
            return false;
        };

        restore_first_port(&mut status, &mut command, &mut data, config_byte);
        wait_for_input_clear(&mut status);
        command.write(0xAE);
        true
    }
}

pub fn poll_data() -> Option<u8> {
    let mut status = Port::<u8>::new(STATUS_PORT);
    let mut data = Port::<u8>::new(DATA_PORT);

    unsafe {
        if status.read() & STATUS_OUTPUT_FULL != 0 {
            Some(data.read())
        } else {
            None
        }
    }
}

unsafe fn restore_first_port(
    status: &mut Port<u8>,
    command: &mut Port<u8>,
    data: &mut Port<u8>,
    config_byte: u8,
) {
    let restored = (config_byte | CONFIG_FIRST_PORT_INTERRUPT) & !CONFIG_FIRST_PORT_CLOCK_DISABLED;
    unsafe {
        wait_for_input_clear(status);
        command.write(0x60);
        wait_for_input_clear(status);
        data.write(restored);
        wait_for_input_clear(status);
        command.write(0xAE);
    }
}

unsafe fn flush_output(status: &mut Port<u8>, data: &mut Port<u8>) {
    for _ in 0..32 {
        if unsafe { status.read() } & STATUS_OUTPUT_FULL == 0 {
            break;
        }
        let _ = unsafe { data.read() };
    }
}

unsafe fn wait_for_input_clear(status: &mut Port<u8>) {
    for _ in 0..10_000 {
        if unsafe { status.read() } & STATUS_INPUT_FULL == 0 {
            break;
        }
        core::hint::spin_loop();
    }
}

unsafe fn wait_and_read(status: &mut Port<u8>, data: &mut Port<u8>) -> Option<u8> {
    for _ in 0..10_000 {
        if unsafe { status.read() } & STATUS_OUTPUT_FULL != 0 {
            return Some(unsafe { data.read() });
        }
        core::hint::spin_loop();
    }
    None
}
