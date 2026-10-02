#include "LaunchOptions.h"

#include <vector>

namespace {

// Split on whitespace, honouring double quotes.
std::vector<std::wstring> Tokenize(const std::wstring& line) {
    std::vector<std::wstring> tokens;
    std::wstring current;
    bool inQuotes = false;
    bool have = false;
    for (wchar_t c : line) {
        if (c == L'"') {
            inQuotes = !inQuotes;
            have = true;
        } else if (!inQuotes && (c == L' ' || c == L'\t')) {
            if (have) {
                tokens.push_back(current);
                current.clear();
                have = false;
            }
        } else {
            current.push_back(c);
            have = true;
        }
    }
    if (have) {
        tokens.push_back(current);
    }
    return tokens;
}

}  // namespace

LaunchOptions ParseLaunchOptions(const wchar_t* cmdLine) {
    LaunchOptions options;
    std::vector<std::wstring> args = Tokenize(cmdLine ? cmdLine : L"");
    for (size_t i = 0; i < args.size(); ++i) {
        const std::wstring& arg = args[i];
        auto next = [&](std::wstring* out) -> bool {
            if (i + 1 >= args.size()) {
                return false;
            }
            *out = args[++i];
            return true;
        };
        if (arg == L"--library-dir" || arg == L"--rom-dir") {
            next(&options.libraryDir);
        } else if (arg == L"--rom") {
            next(&options.rom);
        } else if (arg.rfind(L"--library-dir=", 0) == 0) {
            options.libraryDir = arg.substr(14);
        } else if (arg.rfind(L"--rom=", 0) == 0) {
            options.rom = arg.substr(6);
        } else if (!arg.empty() && arg[0] != L'-' && options.rom.empty()) {
            options.rom = arg;
        }
    }
    return options;
}
