fn main() {
    // Указываем компоновщику использовать наш скрипт
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rustc-link-arg=-Tlinker.ld");
    
    // Указываем путь к bootloader
    let bootloader_locator = bootloader_locator::locate_bootloader("bootloader").unwrap();
    println!("cargo:rustc-env=BOOTLOADER={}", bootloader_locator.display());
}