// ---------------------------------------------------------------------------
// fc-gamepad -- the console's controller ports, read from real pads.
//
// One job: turn whatever the players are holding into the eight switches of an
// NES controller each, and say so on stdout. It knows nothing about the
// emulator, the Electron process, or what a button press does; it only reports.
//
// The protocol is one JSON object per line, flushed immediately, and it is
// documented in native/gamepad/README.md. This program is the Windows
// implementation of it (see backend_windows.cpp); the Swift program in
// native/gamepad is the macOS one, and the two write the same bytes.
//
// stdin is a signal, not input: the parent keeps it open for as long as it
// wants this process to live, and EOF is the clean exit. That is what stops a
// killed Electron from leaving an orphan holding the HID device.
//
//   fc-gamepad [--frames N] [--interval-ms N]
// ---------------------------------------------------------------------------

#include <atomic>
#include <chrono>
#include <cstdlib>
#include <iostream>
#include <string>
#include <thread>

#include "backend.h"
#include "protocol.h"
#include "reporter.h"

#ifdef _WIN32
#include <windows.h>
#include <fcntl.h>
#include <io.h>
#include <mmsystem.h>
#else
#include <unistd.h>
#endif

namespace {

struct Options {
    /// 0 means "run until stdin closes", which is how the application runs
    /// it. Anything else is a test or a smoke check: poll that many times and
    /// exit.
    long frames = 0;
    int intervalMs = 16;
};

long long processId() {
#ifdef _WIN32
    return static_cast<long long>(GetCurrentProcessId());
#else
    return static_cast<long long>(::getpid());
#endif
}

void printUsage(const char* program) {
    std::cerr
        << "usage: " << program << " [--frames N] [--interval-ms N]\n"
        << "\n"
        << "Reads game controllers and writes JSON Lines to stdout.\n"
        << "Runs until stdin closes; --frames N polls N times and exits.\n";
}

bool parseLong(const std::string& text, long& value) {
    try {
        std::size_t used = 0;
        const long parsed = std::stol(text, &used);
        if (used != text.size()) {
            return false;
        }
        value = parsed;
        return true;
    } catch (...) {
        return false;
    }
}

bool parseOptions(int argc, char** argv, Options& options) {
    for (int i = 1; i < argc; ++i) {
        const std::string argument = argv[i];
        if (argument == "--help" || argument == "-h") {
            printUsage(argv[0]);
            std::exit(0);
        }

        long value = 0;
        if (argument == "--frames" && i + 1 < argc && parseLong(argv[++i], value) && value >= 0) {
            options.frames = value;
        } else if (argument == "--interval-ms" && i + 1 < argc && parseLong(argv[++i], value) && value >= 0) {
            options.intervalMs = static_cast<int>(value);
        } else {
            std::cerr << "unknown or incomplete argument: " << argument << "\n";
            printUsage(argv[0]);
            return false;
        }
    }
    return true;
}

/// The pipe the parent reads is a byte stream, not a console. On Windows the
/// C runtime translates every '\n' to "\r\n" unless this is turned off, and
/// the protocol says the line is exactly the JSON object.
void useBinaryStdout() {
#ifdef _WIN32
    _setmode(_fileno(stdout), _O_BINARY);
#endif
}

/// Watch stdin and report when it closes. A flag rather than calling exit()
/// from the thread: exiting while the polling thread is halfway through
/// writing a line would truncate it, and the parent would have to defend
/// against half a JSON object.
void watchStdin(std::atomic<bool>& closed) {
    std::thread([&closed] {
        std::string line;
        while (std::getline(std::cin, line)) {
            // Anything the parent sends is ignored: stdin is a watchdog, not
            // input. Reading it is how EOF is noticed.
        }
        closed.store(true);
    }).detach();
}

int intervalOrDefault(const Options& options) {
    return options.intervalMs > 0 ? options.intervalMs : 16;
}

void sleepFor(int milliseconds) {
    if (milliseconds > 0) {
        std::this_thread::sleep_for(std::chrono::milliseconds(milliseconds));
    }
}

}  // namespace

int main(int argc, char** argv) {
    Options options;
    if (!parseOptions(argc, argv, options)) {
        return 2;
    }

#ifdef _WIN32
    // The default timer resolution is about 15.6ms, so a 16ms sleep is
    // sometimes served as two ticks. A pad that reports at 30Hz instead of
    // 60Hz is a pad that feels laggy, and this one call is what prevents it.
    timeBeginPeriod(1);
#endif

    useBinaryStdout();

    fc::StdoutLineWriter writer;
    writer.writeLine(fc::helloMessage(processId()));

    std::unique_ptr<fc::Backend> backend = fc::makeBackend();
    fc::Reporter reporter(*backend, writer);

    // A scripted number of polls: the request is bounded, so there is no
    // watchdog and no stdin to close.
    if (options.frames > 0) {
        for (long frame = 0; frame < options.frames; ++frame) {
            reporter.poll();
            if (frame + 1 < options.frames) {
                sleepFor(intervalOrDefault(options));
            }
        }
#ifdef _WIN32
        timeEndPeriod(1);
#endif
        return 0;
    }

    std::atomic<bool> stdinClosed{false};
    watchStdin(stdinClosed);

    while (!stdinClosed.load()) {
        reporter.poll();
        sleepFor(intervalOrDefault(options));
    }

#ifdef _WIN32
    timeEndPeriod(1);
#endif
    return 0;
}
