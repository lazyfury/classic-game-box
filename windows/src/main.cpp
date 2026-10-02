#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif

#include <windows.h>

#include <objbase.h>

#include <utility>

#include "LaunchOptions.h"
#include "WinWindow.h"

// The C++ Win32 host entry point. It mirrors the Swift `main.swift` +
// `AppDelegate`: create the window, start the Rust app on its `HWND`, and pump
// the message loop.
int WINAPI wWinMain(HINSTANCE instance, HINSTANCE /*prev*/, PWSTR cmdLine, int /*show*/) {
    // Per-monitor DPI v2 before any window exists, so the client size and the
    // backing scale are correct from the first frame.
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    // wgpu's DX12 backend wants COM available on its thread; apartment
    // threaded matches the default for a GUI process.
    HRESULT com = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

    LaunchOptions options = ParseLaunchOptions(cmdLine ? cmdLine : L"");
    WinWindow window(instance, std::move(options));
    if (!window.Create()) {
        MessageBoxW(nullptr, L"创建窗口失败。", L"Classic Game Box", MB_ICONERROR | MB_OK);
        if (SUCCEEDED(com)) {
            CoUninitialize();
        }
        return 1;
    }

    int code = window.Run();
    if (SUCCEEDED(com)) {
        CoUninitialize();
    }
    return code;
}
