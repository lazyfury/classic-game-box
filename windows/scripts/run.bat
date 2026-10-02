@echo off
rem Build (if needed) and run the C++ Win32 host.
rem
rem   windows\scripts\run.bat
rem   windows\scripts\run.bat mario.nes
setlocal
set ROOT=%~dp0..\..
if "%CGB_RUST_PROFILE%"=="" set CGB_RUST_PROFILE=debug
if /I "%CGB_RUST_PROFILE%"=="release" (set CFG=Release) else (set CFG=Debug)

if not exist "%ROOT%\windows\build\%CFG%\cgb-win.exe" (
    call "%~dp0build.bat"
    if errorlevel 1 exit /b 1
)

"%ROOT%\windows\build\%CFG%\cgb-win.exe" %*
