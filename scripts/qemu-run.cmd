@echo off
rem QEMU launcher for dm.yaml (Windows, any shell).
rem Rebuilds the image first so every (re)start boots the fresh artifact,
rem then boots the UEFI kernel with the local EDK2 firmware.
setlocal
cd /d "%~dp0.." || exit /b 1

call dm build || exit /b 1

if not exist target\orbita-disk.img fsutil file createnew target\orbita-disk.img 16777216 >nul

qemu-system-x86_64 -machine q35 -m 512M -smp 4 ^
  -drive file=target/orbita-disk.img,format=raw,if=none,id=orbitadisk ^
  -device ich9-ahci,id=orbita_ahci ^
  -device ide-hd,drive=orbitadisk,bus=orbita_ahci.0 ^
  -drive format=raw,if=none,id=orbitapkg,file=target/orbita-pkg.img ^
  -device ide-hd,drive=orbitapkg,bus=orbita_ahci.1 ^
  -drive if=pflash,format=raw,readonly=on,file=target/qemu-firmware/edk2-x86_64-code.fd ^
  -drive if=pflash,format=raw,file=target/qemu-firmware/edk2-vars.fd ^
  -drive format=raw,file=fat:rw:target/orbita-esp ^
  -netdev user,id=orbitanet ^
  -device e1000,netdev=orbitanet ^
  -serial stdio %ORBITA_QEMU_EXTRA%
exit /b %ERRORLEVEL%
