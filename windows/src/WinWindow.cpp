#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif

#include "WinWindow.h"

#include <imm.h>
#include <shellapi.h>
#include <windowsx.h>

#include <string>
#include <utility>
#include <vector>

#include "Input.h"
#include "cgb_host.h"

namespace {

constexpr wchar_t kClassName[] = L"ClassicGameBoxWin";
constexpr int kInitialWidth = 1100;
constexpr int kInitialHeight = 760;
constexpr int kMinWidthDip = 960;
constexpr int kMinHeightDip = 600;

// A wheel notch scrolls about this many logical pixels, matching the macOS
// host's line→point factor.
constexpr float kWheelStep = 30.0f;

}  // namespace

WinWindow::WinWindow(HINSTANCE instance, LaunchOptions options)
    : instance_(instance), options_(std::move(options)) {}

WinWindow::~WinWindow() {
    if (app_) {
        // The wgpu surface borrows the HWND, so the Rust app must be torn down
        // while the window still exists. `DestroyWindow` runs in `WndProc`'s
        // WM_CLOSE, before `hwnd_` is released.
        cgb_host_destroy(app_);
        app_ = nullptr;
    }
    if (hwnd_) {
        DestroyWindow(hwnd_);
        hwnd_ = nullptr;
    }
}

bool WinWindow::Create() {
    WNDCLASSEXW wc = {};
    wc.cbSize = sizeof(wc);
    wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
    wc.lpfnWndProc = &WinWindow::WndProcThunk;
    wc.hInstance = instance_;
    wc.hCursor = nullptr;  // set per-frame by SyncCursor / WM_SETCURSOR
    wc.hbrBackground = nullptr;
    wc.lpszClassName = kClassName;
    wc.hIcon = LoadIconW(nullptr, IDI_APPLICATION);
    if (RegisterClassExW(&wc) == 0) {
        return false;
    }

    RECT rect = {0, 0, kInitialWidth, kInitialHeight};
    AdjustWindowRectEx(&rect, WS_OVERLAPPEDWINDOW, FALSE, 0);
    hwnd_ = CreateWindowExW(
        0, kClassName, L"Classic Game Box", WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT, CW_USEDEFAULT, rect.right - rect.left, rect.bottom - rect.top,
        nullptr, nullptr, instance_, this);
    if (!hwnd_) {
        return false;
    }

    dpi_ = GetDpiForWindow(hwnd_);
    if (dpi_ == 0) {
        dpi_ = 96;
    }

    ShowWindow(hwnd_, SW_SHOW);
    UpdateWindow(hwnd_);
    return true;
}

int WinWindow::Run() {
    if (!StartApp()) {
        return 1;
    }
    DragAcceptFiles(hwnd_, TRUE);

    RunFrame();

    MSG msg = {};
    for (;;) {
        bool needFrame = false;
        if (continuous_) {
            // A running game / animation: present as fast as the surface
            // allows (Fifo throttles to the display), while staying responsive
            // to messages.
            RunFrame();
            MsgWaitForMultipleObjectsEx(0, nullptr, 1, QS_ALLINPUT, 0);
        } else {
            // Idle: the gamepad's own loop. Wake on a message or a short
            // timeout, then sample input; a pad connecting or an assignment
            // change needs a frame even though no window message arrived.
            DWORD wait = MsgWaitForMultipleObjectsEx(0, nullptr, 16, QS_ALLINPUT, 0);
            if (wait == WAIT_TIMEOUT && app_ && cgb_host_poll(app_)) {
                needFrame = true;
            }
        }

        while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
            needFrame = true;
            if (msg.message == WM_QUIT) {
                return static_cast<int>(msg.wParam);
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if (needFrame && !continuous_) {
            RunFrame();
        }
    }
}

bool WinWindow::StartApp() {
    RECT client = {};
    GetClientRect(hwnd_, &client);
    LONG width = client.right - client.left;
    LONG height = client.bottom - client.top;
    uint32_t physicalWidth = static_cast<uint32_t>(width > 0 ? width : 1);
    uint32_t physicalHeight = static_cast<uint32_t>(height > 0 ? height : 1);

    std::string library = cgbinput::ToUtf8(options_.libraryDir);
    std::string rom = cgbinput::ToUtf8(options_.rom);
    app_ = cgb_host_start(reinterpret_cast<void*>(hwnd_), physicalWidth, physicalHeight, Scale(),
                         library.empty() ? nullptr : library.c_str(),
                         rom.empty() ? nullptr : rom.c_str());
    if (!app_) {
        MessageBoxW(hwnd_, L"无法创建 wgpu 渲染后端（HWND surface / device）。",
                    L"Classic Game Box", MB_ICONERROR | MB_OK);
        return false;
    }
    return true;
}

void WinWindow::RunFrame() {
    if (!app_) {
        return;
    }
    ApplyResizeIfNeeded();
    cgb_host_frame(app_);
    SyncCursor();
    SyncFullscreen();
    SyncImeCaret();
    continuous_ = cgb_host_needs_frame(app_);
}

void WinWindow::ApplyResizeIfNeeded() {
    if (!resizePending_ || !app_) {
        return;
    }
    resizePending_ = false;
    RECT client = {};
    GetClientRect(hwnd_, &client);
    LONG width = client.right - client.left;
    LONG height = client.bottom - client.top;
    uint32_t physicalWidth = static_cast<uint32_t>(width > 0 ? width : 1);
    uint32_t physicalHeight = static_cast<uint32_t>(height > 0 ? height : 1);
    cgb_host_resize(app_, physicalWidth, physicalHeight, Scale());
}

void WinWindow::SyncFullscreen() {
    if (!app_) {
        return;
    }
    int request = cgb_host_take_fullscreen(app_);
    if (request == 1 && !fullscreen_) {
        SetFullscreen(true);
    } else if (request == 0 && fullscreen_) {
        SetFullscreen(false);
    }
}

void WinWindow::SetFullscreen(bool on) {
    if (on) {
        placement_.length = sizeof(placement_);
        GetWindowPlacement(hwnd_, &placement_);
        MONITORINFO monitor = {};
        monitor.cbSize = sizeof(monitor);
        GetMonitorInfoW(MonitorFromWindow(hwnd_, MONITOR_DEFAULTTONEAREST), &monitor);
        LONG_PTR style = GetWindowLongPtrW(hwnd_, GWL_STYLE);
        SetWindowLongPtrW(hwnd_, GWL_STYLE, style & ~WS_OVERLAPPEDWINDOW);
        SetWindowPos(hwnd_, HWND_TOP,
                     monitor.rcMonitor.left, monitor.rcMonitor.top,
                     monitor.rcMonitor.right - monitor.rcMonitor.left,
                     monitor.rcMonitor.bottom - monitor.rcMonitor.top,
                     SWP_FRAMECHANGED | SWP_NOOWNERZORDER);
        fullscreen_ = true;
    } else {
        LONG_PTR style = GetWindowLongPtrW(hwnd_, GWL_STYLE);
        SetWindowLongPtrW(hwnd_, GWL_STYLE, style | WS_OVERLAPPEDWINDOW);
        SetWindowPlacement(hwnd_, &placement_);
        SetWindowPos(hwnd_, nullptr, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED);
        fullscreen_ = false;
    }
    resizePending_ = true;
}

void WinWindow::SyncCursor() {
    if (!app_) {
        return;
    }
    uint32_t code = cgb_host_cursor(app_);
    if (code != lastCursor_) {
        lastCursor_ = code;
        SetCursor(CursorFor(code));
    }
}

void WinWindow::SyncImeCaret() {
    if (!app_) {
        return;
    }
    float x = 0.0f, y = 0.0f, w = 0.0f, h = 0.0f;
    if (!cgb_host_caret(app_, &x, &y, &w, &h)) {
        return;
    }
    HIMC context = ImmGetContext(hwnd_);
    if (!context) {
        return;
    }
    POINT point = {static_cast<LONG>(x * Scale()), static_cast<LONG>(y * Scale())};
    ClientToScreen(hwnd_, &point);

    CANDIDATEFORM candidate = {};
    candidate.dwIndex = 0;
    candidate.dwStyle = CFS_CANDIDATEPOS;
    candidate.ptCurrentPos = point;
    ImmSetCandidateWindow(context, &candidate);

    COMPOSITIONFORM composition = {};
    composition.dwStyle = CFS_POINT;
    composition.ptCurrentPos = point;
    ImmSetCompositionWindow(context, &composition);

    ImmReleaseContext(hwnd_, context);
}

void WinWindow::HandleChar(wchar_t c) {
    if (c < 0x20 || c == 0x7F) {
        // Enter / Tab / Backspace / Escape arrive as key messages; do not also
        // deliver them as text.
        return;
    }
    std::wstring text;
    if (c >= 0xD800 && c <= 0xDBFF) {
        pendingHighSurrogate_ = c;
        return;
    }
    if (c >= 0xDC00 && c <= 0xDFFF) {
        if (pendingHighSurrogate_ == 0) {
            return;
        }
        text.push_back(pendingHighSurrogate_);
        text.push_back(c);
        pendingHighSurrogate_ = 0;
    } else {
        pendingHighSurrogate_ = 0;
        text.push_back(c);
    }
    std::string utf8 = cgbinput::ToUtf8(text);
    if (!utf8.empty()) {
        cgb_host_text(app_, utf8.c_str());
    }
}

void WinWindow::HandleIme(UINT msg, WPARAM /*wParam*/, LPARAM lParam) {
    if (!app_) {
        return;
    }
    HIMC context = ImmGetContext(hwnd_);
    switch (msg) {
        case WM_IME_STARTCOMPOSITION:
            composing_ = true;
            cgb_host_ime(app_, 0, nullptr, -1, -1);
            break;
        case WM_IME_COMPOSITION: {
            if (!context) {
                break;
            }
            if (lParam & GCS_RESULTSTR) {
                LONG bytes = ImmGetCompositionStringW(context, GCS_RESULTSTR, nullptr, 0);
                if (bytes > 0) {
                    std::wstring text(static_cast<size_t>(bytes) / sizeof(wchar_t), L'\0');
                    ImmGetCompositionStringW(context, GCS_RESULTSTR, text.data(),
                                             static_cast<DWORD>(bytes));
                    std::string utf8 = cgbinput::ToUtf8(text);
                    cgb_host_ime(app_, 3, utf8.c_str(), -1, -1);
                }
                composing_ = false;
            }
            if (lParam & GCS_COMPSTR) {
                LONG bytes = ImmGetCompositionStringW(context, GCS_COMPSTR, nullptr, 0);
                std::wstring text;
                if (bytes > 0) {
                    text.resize(static_cast<size_t>(bytes) / sizeof(wchar_t));
                    ImmGetCompositionStringW(context, GCS_COMPSTR, text.data(),
                                             static_cast<DWORD>(bytes));
                }
                std::string utf8 = cgbinput::ToUtf8(text);
                composing_ = !utf8.empty();
                cgb_host_ime(app_, 2, utf8.c_str(), static_cast<int32_t>(utf8.size()),
                            static_cast<int32_t>(utf8.size()));
            }
            break;
        }
        case WM_IME_ENDCOMPOSITION:
            composing_ = false;
            cgb_host_ime(app_, 1, nullptr, -1, -1);
            break;
        default:
            break;
    }
    if (context) {
        ImmReleaseContext(hwnd_, context);
    }
}

float WinWindow::Scale() const {
    return dpi_ != 0 ? static_cast<float>(dpi_) / 96.0f : 1.0f;
}

HCURSOR WinWindow::CursorFor(uint32_t code) {
    LPCWSTR id = IDC_ARROW;
    switch (code) {
        case 1:
            id = IDC_HAND;
            break;
        case 2:
            id = IDC_IBEAM;
            break;
        case 3:
            id = IDC_SIZEWE;
            break;
        case 4:
            id = IDC_SIZENS;
            break;
        case 5:
        case 6:
            // No stock open/closed-hand cursor; the four-way arrow stands in.
            id = IDC_SIZEALL;
            break;
        default:
            id = IDC_ARROW;
            break;
    }
    return LoadCursorW(nullptr, id);
}

LRESULT CALLBACK WinWindow::WndProcThunk(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    WinWindow* self;
    if (msg == WM_NCCREATE) {
        auto* create = reinterpret_cast<CREATESTRUCTW*>(lParam);
        self = reinterpret_cast<WinWindow*>(create->lpCreateParams);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
        if (self) {
            self->hwnd_ = hwnd;
        }
    } else {
        self = reinterpret_cast<WinWindow*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    }
    if (self) {
        return self->WndProc(hwnd, msg, wParam, lParam);
    }
    return DefWindowProcW(hwnd, msg, wParam, lParam);
}

LRESULT WinWindow::WndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    switch (msg) {
        case WM_GETMINMAXINFO: {
            auto* info = reinterpret_cast<MINMAXINFO*>(lParam);
            UINT dpi = hwnd ? GetDpiForWindow(hwnd) : dpi_;
            if (dpi == 0) {
                dpi = 96;
            }
            info->ptMinTrackSize.x = MulDiv(kMinWidthDip, static_cast<int>(dpi), 96);
            info->ptMinTrackSize.y = MulDiv(kMinHeightDip, static_cast<int>(dpi), 96);
            return 0;
        }
        case WM_SIZE:
            resizePending_ = true;
            return 0;
        case WM_DPICHANGED: {
            dpi_ = HIWORD(wParam);
            auto* suggested = reinterpret_cast<RECT*>(lParam);
            SetWindowPos(hwnd, nullptr, suggested->left, suggested->top,
                         suggested->right - suggested->left, suggested->bottom - suggested->top,
                         SWP_NOZORDER | SWP_NOACTIVATE);
            resizePending_ = true;
            return 0;
        }
        case WM_ERASEBKGND:
            return 1;  // the GPU paints; do not flash the background
        case WM_PAINT: {
            PAINTSTRUCT paint = {};
            BeginPaint(hwnd, &paint);
            EndPaint(hwnd, &paint);
            return 0;
        }
        case WM_SETCURSOR:
            if (LOWORD(lParam) == HTCLIENT && app_) {
                SetCursor(CursorFor(lastCursor_));
                return TRUE;
            }
            return DefWindowProcW(hwnd, msg, wParam, lParam);
        case WM_MOUSEMOVE:
            if (app_) {
                if (!trackingLeave_) {
                    TRACKMOUSEEVENT track = {};
                    track.cbSize = sizeof(track);
                    track.dwFlags = TME_LEAVE;
                    track.hwndTrack = hwnd;
                    TrackMouseEvent(&track);
                    trackingLeave_ = true;
                }
                float x = 0.0f, y = 0.0f;
                cgbinput::ClientToLogical(lParam, Scale(), &x, &y);
                cgb_host_pointer_move(app_, x, y);
            }
            return 0;
        case WM_MOUSELEAVE:
            trackingLeave_ = false;
            if (app_) {
                cgb_host_pointer_leave(app_);
            }
            return 0;
        case WM_LBUTTONDOWN:
        case WM_RBUTTONDOWN:
        case WM_MBUTTONDOWN:
        case WM_LBUTTONDBLCLK:
        case WM_RBUTTONDBLCLK:
        case WM_MBUTTONDBLCLK:
            if (app_) {
                uint32_t button = (msg == WM_RBUTTONDOWN || msg == WM_RBUTTONDBLCLK) ? 1u
                                  : (msg == WM_MBUTTONDOWN || msg == WM_MBUTTONDBLCLK) ? 2u
                                                                                        : 0u;
                uint32_t clickCount =
                    (msg == WM_LBUTTONDBLCLK || msg == WM_RBUTTONDBLCLK || msg == WM_MBUTTONDBLCLK)
                        ? 2u
                        : 1u;
                float x = 0.0f, y = 0.0f;
                cgbinput::ClientToLogical(lParam, Scale(), &x, &y);
                cgb_host_pointer_down(app_, x, y, button, clickCount);
            }
            return 0;
        case WM_LBUTTONUP:
        case WM_RBUTTONUP:
        case WM_MBUTTONUP:
            if (app_) {
                uint32_t button = (msg == WM_RBUTTONUP) ? 1u : (msg == WM_MBUTTONUP) ? 2u : 0u;
                float x = 0.0f, y = 0.0f;
                cgbinput::ClientToLogical(lParam, Scale(), &x, &y);
                cgb_host_pointer_up(app_, x, y, button);
            }
            return 0;
        case WM_MOUSEWHEEL:
        case WM_MOUSEHWHEEL:
            if (app_) {
                POINT point = {GET_X_LPARAM(lParam), GET_Y_LPARAM(lParam)};
                ScreenToClient(hwnd, &point);
                float scale = Scale();
                float x = static_cast<float>(point.x) / scale;
                float y = static_cast<float>(point.y) / scale;
                float notches =
                    static_cast<float>(GET_WHEEL_DELTA_WPARAM(wParam)) / static_cast<float>(WHEEL_DELTA);
                if (msg == WM_MOUSEWHEEL) {
                    cgb_host_scroll(app_, x, y, 0.0f, -notches * kWheelStep);
                } else {
                    cgb_host_scroll(app_, x, y, notches * kWheelStep, 0.0f);
                }
            }
            return 0;
        case WM_KEYDOWN:
        case WM_SYSKEYDOWN:
            if (app_ && !composing_) {
                std::string characters = cgbinput::KeyChar(static_cast<UINT>(wParam), lParam);
                cgb_host_key_down(app_, static_cast<uint32_t>(wParam),
                                 characters.empty() ? nullptr : characters.c_str(),
                                 cgbinput::ModifierBits());
            }
            return 0;
        case WM_KEYUP:
        case WM_SYSKEYUP:
            if (app_ && !composing_) {
                std::string characters = cgbinput::KeyChar(static_cast<UINT>(wParam), lParam);
                cgb_host_key_up(app_, static_cast<uint32_t>(wParam),
                               characters.empty() ? nullptr : characters.c_str());
            }
            return 0;
        case WM_CHAR:
        case WM_SYSCHAR:
            if (app_ && !composing_) {
                HandleChar(static_cast<wchar_t>(wParam));
            }
            return 0;
        case WM_IME_STARTCOMPOSITION:
        case WM_IME_COMPOSITION:
        case WM_IME_ENDCOMPOSITION:
            HandleIme(msg, wParam, lParam);
            return DefWindowProcW(hwnd, msg, wParam, lParam);
        case WM_DROPFILES: {
            if (app_) {
                HDROP drop = reinterpret_cast<HDROP>(wParam);
                UINT count = DragQueryFileW(drop, 0xFFFFFFFF, nullptr, 0);
                for (UINT i = 0; i < count; ++i) {
                    UINT length = DragQueryFileW(drop, i, nullptr, 0);
                    if (length == 0) {
                        continue;
                    }
                    std::wstring path(length, L'\0');
                    DragQueryFileW(drop, i, path.data(), length + 1);
                    std::string utf8 = cgbinput::ToUtf8(path);
                    if (!utf8.empty()) {
                        cgb_host_dropped_file(app_, utf8.c_str());
                    }
                }
                DragFinish(drop);
            }
            return 0;
        }
        case WM_CLOSE:
            // The wgpu surface borrows the HWND, so drop the Rust app (and its
            // surface) while the window is still alive — the same ordering the
            // macOS host uses in `applicationWillTerminate`.
            if (app_) {
                cgb_host_destroy(app_);
                app_ = nullptr;
            }
            DestroyWindow(hwnd);
            return 0;
        case WM_DESTROY:
            hwnd_ = nullptr;
            PostQuitMessage(0);
            return 0;
        default:
            return DefWindowProcW(hwnd, msg, wParam, lParam);
    }
}
