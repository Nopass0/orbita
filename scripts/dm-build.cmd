@echo off
rem Build-pipeline stages for dm.yaml (Windows, any shell):
rem   dm-build.cmd apps       - build+pack orbita applications (.orbpkg)
rem   dm-build.cmd esp        - assemble the FAT ESP image + publish EFI + stage /pkg
rem   dm-build.cmd firmware   - stage the EDK2 firmware for QEMU pflash
setlocal
set "ROOT=%~dp0.."
pushd "%ROOT%" || exit /b 1

if "%~1"=="apps" goto :apps
if "%~1"=="esp" goto :esp
if "%~1"=="firmware" goto :firmware
echo usage: dm-build.cmd {apps^|esp^|firmware} 1>&2
popd
exit /b 1

:apps
cargo run --release -p orbita-build -- pack-all || goto :fail
popd
exit /b 0

:esp
if exist target\orbita-esp rd /s /q target\orbita-esp
mkdir target\orbita-esp\EFI\BOOT 2>nul
if not exist dist mkdir dist
copy /y target\x86_64-unknown-uefi\release\orbita-kernel.efi target\orbita-esp\EFI\BOOT\BOOTX64.EFI >nul || goto :fail
echo fs0:\EFI\BOOT\BOOTX64.EFI> target\orbita-esp\startup.nsh
rem Stage application packages into the ESP delivery channel (/pkg).
if exist target\orbita-pkg\*.orbpkg (
  mkdir target\orbita-esp\pkg 2>nul
  copy /y target\orbita-pkg\*.orbpkg target\orbita-esp\pkg\ >nul
)
copy /y target\x86_64-unknown-uefi\release\orbita-kernel.efi dist\orbita-x86_64-uefi.efi >nul || goto :fail
popd
exit /b 0

:firmware
if not exist target\qemu-firmware mkdir target\qemu-firmware
if "%ORBITA_QEMU_SHARE%"=="" set "ORBITA_QEMU_SHARE=C:\Program Files\qemu\share"
copy /y "%ORBITA_QEMU_SHARE%\edk2-x86_64-code.fd" target\qemu-firmware\edk2-x86_64-code.fd >nul || goto :fail
if exist target\qemu-firmware\edk2-vars.fd del /q target\qemu-firmware\edk2-vars.fd
copy /y "%ORBITA_QEMU_SHARE%\edk2-i386-vars.fd" target\qemu-firmware\edk2-vars.fd >nul || goto :fail
popd
exit /b 0

:fail
popd
exit /b 1
