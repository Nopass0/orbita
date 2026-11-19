@echo off
rem QEMU launcher for dm.yaml (Windows, any shell).
rem Rebuilds the image first so every (re)start boots the fresh artifact,
rem then boots the UEFI kernel with the local EDK2 firmware.
rem
rem Графика: virtio-vga-gl + virgl — интерфейс Orbita рисует РЕАЛЬНАЯ
rem видеокарта хоста (OpenGL через virglrenderer), окно QEMU показывает
rem результат через SDL+GL. Процессор: WHPX (аппаратная виртуализация),
rem при недоступности — TCG.
rem
rem Переменные окружения:
rem   ORBITA_DISPLAY  — дисплей QEMU (по умолчанию sdl,gl=on)
rem   ORBITA_ACCEL    — ускоритель (по умолчанию whpx,kernel-irqchip=off; tcg — без ускорения)
rem   ORBITA_RES      — разрешение, например 1600x900 (по умолчанию 1600x900)
rem   ORBITA_QEMU_EXTRA — дополнительные аргументы QEMU
setlocal
cd /d "%~dp0.." || exit /b 1

if not "%ORBITA_SKIP_BUILD%"=="1" (
  call dm build || exit /b 1
)

if not exist target\orbita-disk.img fsutil file createnew target\orbita-disk.img 16777216 >nul

set "QEMU=qemu-system-x86_64"
where %QEMU% >nul 2>nul || set "QEMU=C:\Program Files\qemu\qemu-system-x86_64.exe"
if "%ORBITA_DISPLAY%"=="" set "ORBITA_DISPLAY=sdl,gl=on"
if "%ORBITA_ACCEL%"=="" set "ORBITA_ACCEL=whpx,kernel-irqchip=off"
if "%ORBITA_RES%"=="" set "ORBITA_RES=1600x900"
for /f "tokens=1,2 delims=x" %%a in ("%ORBITA_RES%") do (
  set "RES_X=%%a"
  set "RES_Y=%%b"
)

"%QEMU%" -machine q35 -accel %ORBITA_ACCEL% -accel tcg -m 1G -smp 4 ^
  -rtc base=localtime ^
  -vga none -device virtio-vga-gl,xres=%RES_X%,yres=%RES_Y% -display %ORBITA_DISPLAY% ^
  -device virtio-tablet-pci ^
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
