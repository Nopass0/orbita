@echo off
rem Запуск Orbita OS в QEMU (dm start / dm alias run / вручную).
rem
rem Сначала пересобирает образ (если не задано ORBITA_SKIP_BUILD=1), затем
rem загружает ядро в QEMU:
rem   * CPU: 4 ядра, WHPX (аппаратная виртуализация Windows), запасной — TCG;
rem   * память: 4 ГБ;
rem   * GPU: virtio-vga-gl + virgl — интерфейс рисует РЕАЛЬНАЯ видеокарта
rem     хоста; окно GTK+OpenGL, размер окна = разрешение ОС (меняется на лету);
rem   * мышь: virtio-tablet (без захвата курсора);
rem   * диск: target\orbita-disk.img, 10 ГБ (OrbitaFS, изменения сохраняются).
rem
rem Переменные окружения:
rem   ORBITA_DISPLAY  — дисплей QEMU (по умолчанию gtk,gl=on,show-menubar=off)
rem   ORBITA_ACCEL    — ускоритель (по умолчанию whpx,kernel-irqchip=off)
rem   ORBITA_RES      — начальное разрешение (по умолчанию 1600x900)
rem   ORBITA_MEM      — память (по умолчанию 4G)
rem   ORBITA_CPUS     — ядра (по умолчанию 4)
rem   ORBITA_QEMU_EXTRA — дополнительные аргументы QEMU
setlocal
cd /d "%~dp0.." || exit /b 1

if not "%ORBITA_SKIP_BUILD%"=="1" (
  call dm build || exit /b 1
)

rem Диск 10 ГБ (разреженный файл). Метка .10g — образ уже нужного размера;
rem старый маленький образ сохраняется рядом как orbita-disk.img.old.
if not exist target\orbita-disk.10g (
  if exist target\orbita-disk.img move /y target\orbita-disk.img target\orbita-disk.img.old >nul
  fsutil file createnew target\orbita-disk.img 10737418240 >nul || exit /b 1
  fsutil sparse setflag target\orbita-disk.img >nul 2>nul
  echo 10 GiB> target\orbita-disk.10g
)

set "QEMU=qemu-system-x86_64"
where %QEMU% >nul 2>nul || set "QEMU=C:\Program Files\qemu\qemu-system-x86_64.exe"
if "%ORBITA_DISPLAY%"=="" set "ORBITA_DISPLAY=gtk,gl=on,show-menubar=off"
if "%ORBITA_ACCEL%"=="" set "ORBITA_ACCEL=whpx,kernel-irqchip=off"
if "%ORBITA_RES%"=="" set "ORBITA_RES=1600x900"
if "%ORBITA_MEM%"=="" set "ORBITA_MEM=4G"
if "%ORBITA_CPUS%"=="" set "ORBITA_CPUS=4"
for /f "tokens=1,2 delims=x" %%a in ("%ORBITA_RES%") do (
  set "RES_X=%%a"
  set "RES_Y=%%b"
)

"%QEMU%" -name "Orbita OS" -machine q35 -accel %ORBITA_ACCEL% -accel tcg -m %ORBITA_MEM% -smp %ORBITA_CPUS% ^
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
