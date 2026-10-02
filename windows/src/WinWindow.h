// The Win32 window + message loop, and the frame scheduling that mirrors the
// macOS host's display-link + event-monitor model.
//
// The window is the only thing the shell owns. Rust renders into its `HWND`
// and the UI reacts to the events this class forwards (see `Input.*` and
// `WndProc`).
#pragma once

#include <windows.h>

#include <cstdint>
#include <string>

#include "Gamepads.h"
#include "LaunchOptions.h"

struct CgbHostApp;

class WinWindow {
public:
    WinWindow(HINSTANCE instance, LaunchOptions options);
    ~WinWindow();

    WinWindow(const WinWindow&) = delete;
    WinWindow& operator=(const WinWindow&) = delete;

    // Register the window class, create the window and show it.
    bool Create();

    // Start the Rust app and pump messages until the window closes. Returns the
    // process exit code.
    int Run();

private:
    static LRESULT CALLBACK WndProcThunk(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam);
    LRESULT WndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam);

    bool StartApp();
    void RunFrame();
    void ApplyResizeIfNeeded();
    void SyncFullscreen();
    void SyncCursor();
    void SyncImeCaret();
    void HandleChar(wchar_t c);
    void HandleIme(UINT msg, WPARAM wParam, LPARAM lParam);
    void SetFullscreen(bool on);

    float Scale() const;
    static HCURSOR CursorFor(uint32_t code);

    HINSTANCE instance_ = nullptr;
    LaunchOptions options_;
    HWND hwnd_ = nullptr;
    CgbHostApp* app_ = nullptr;
    Gamepads gamepads_;

    UINT dpi_ = 96;
    bool continuous_ = false;
    bool resizePending_ = false;
    bool trackingLeave_ = false;
    bool composing_ = false;
    bool fullscreen_ = false;
    WINDOWPLACEMENT placement_ = {};
    uint32_t lastCursor_ = 0;
    wchar_t pendingHighSurrogate_ = 0;
};
