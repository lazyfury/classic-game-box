@echo off
rem Build the Rust library, then the C++ Win32 executable.
rem
rem   windows\scripts\build.bat
rem   set CGB_RUST_PROFILE=release & windows\scripts\build.bat
setlocal
set ROOT=%~dp0..\..
if "%CGB_RUST_PROFILE%"=="" set CGB_RUST_PROFILE=debug
if /I "%CGB_RUST_PROFILE%"=="release" (set CFG=Release) else (set CFG=Debug)

pushd "%ROOT%"
if /I "%CGB_RUST_PROFILE%"=="release" (
    cargo build --release
) else (
    cargo build
)
if errorlevel 1 goto :fail

cmake -S windows -B windows\build -A x64 -DCGB_RUST_PROFILE=%CGB_RUST_PROFILE%
if errorlevel 1 goto :fail

cmake --build windows\build --config %CFG%
if errorlevel 1 goto :fail

echo built: %ROOT%\windows\build\%CFG%\cgb-win.exe
popd
exit /b 0

:fail
popd
exit /b 1
