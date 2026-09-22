// ---------------------------------------------------------------------------
// fc_headless - run a ROM through the C API, with scripted input.
//
// Build:  cmake --build build --target fc_headless
// Run:    ./build/fc_headless game.nes --frames 300 --outdir /tmp/native
//         ./build/fc_headless game.nes --frames 600 \
//              --script 100:START=1,105:START=0,300:RIGHT=1 \
//              --snapshots 90,250,400,600
//
// Why this exists next to wasm/headless.mjs
// -----------------------------------------
// wasm/headless.mjs and the Electron renderer share wasm/emulator.mjs, so a
// bug in that file would be invisible to a test that compares only those two.
// This tool shares nothing with either of them: it goes through
// packages/fc-core/src/ffi/emulator_api.h directly, in C++, and links no JavaScript at all.
//
// It is the independent third opinion, and it is what makes the other two
// comparable rather than merely self-consistent.
//
// The command line is deliberately identical to wasm/headless.mjs, so one
// scenario string drives every build being compared:
//
//   --frames N        how many frames to run
//   --outdir DIR      write frame_<k>.ppm for each snapshot frame
//   --dump FILE       write the final frame as a single PPM
//   --script SPEC     100:START=1,105:START=0
//   --snapshots LIST  1,5,30,300          (default: 1,5,30,last)
//   --quiet           no summary
//
// A button change is applied *before* its frame runs, which is where a real
// press lands: the game samples the controller port once per frame, during
// vblank, so what matters is the state when the frame starts.
// ---------------------------------------------------------------------------

#include "ffi/emulator_api.h"

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <vector>

namespace {

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

/// The eight switches, by name. The values come from the C enum, which is the
/// same enum wasm/emulator.mjs's `Button` and electron's ButtonName mirror.
struct ButtonEntry {
    std::string_view name;
    fc_button value;
};

constexpr ButtonEntry kButtons[] = {
    { "A", FC_BUTTON_A },
    { "B", FC_BUTTON_B },
    { "SELECT", FC_BUTTON_SELECT },
    { "START", FC_BUTTON_START },
    { "UP", FC_BUTTON_UP },
    { "DOWN", FC_BUTTON_DOWN },
    { "LEFT", FC_BUTTON_LEFT },
    { "RIGHT", FC_BUTTON_RIGHT },
};

std::optional<fc_button> button_from_name(const std::string& name)
{
    for (const auto& entry : kButtons) {
        if (entry.name == name) {
            return entry.value;
        }
    }
    return std::nullopt;
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

struct ScriptEvent {
    int frame = 0;
    fc_button button = FC_BUTTON_A;
    bool pressed = false;
};

struct Options {
    std::string rom_path;
    int frames = 60;
    std::string out_dir;
    std::string dump_path;
    std::string wav_path;
    std::string samples_path;
    std::string save_path;
    int roundtrip_frame = 0;
    std::set<int> snapshots;
    std::vector<ScriptEvent> script;
    bool quiet = false;
};

std::vector<std::string> split(const std::string& text, char separator)
{
    std::vector<std::string> parts;
    std::string current;

    for (const char character : text) {
        if (character == separator) {
            parts.push_back(current);
            current.clear();
        } else {
            current.push_back(character);
        }
    }
    parts.push_back(current);
    return parts;
}

std::string trim(const std::string& text)
{
    const std::size_t first = text.find_first_not_of(" \t");
    if (first == std::string::npos) {
        return {};
    }
    const std::size_t last = text.find_last_not_of(" \t");
    return text.substr(first, last - first + 1);
}

/// Read "100:START=1,105:START=0". Same syntax and same meaning as the two
/// JavaScript implementations.
bool parse_script(const std::string& spec, std::vector<ScriptEvent>& out, std::string& error)
{
    if (trim(spec).empty()) {
        return true;
    }

    for (const auto& raw : split(spec, ',')) {
        const std::string entry = trim(raw);
        const auto colon = entry.find(':');
        const auto equals = entry.find('=', colon == std::string::npos ? 0 : colon);

        if (colon == std::string::npos || equals == std::string::npos) {
            error = "--script: cannot read \"" + entry + "\". Expected frame:BUTTON=0|1";
            return false;
        }

        ScriptEvent event;
        event.frame = std::stoi(entry.substr(0, colon));

        const auto button = button_from_name(entry.substr(colon + 1, equals - colon - 1));
        if (!button) {
            error = "--script: \"" + entry.substr(colon + 1, equals - colon - 1) + "\" is not a button";
            return false;
        }
        event.button = *button;
        event.pressed = entry.substr(equals + 1) == "1";

        out.push_back(event);
    }
    return true;
}

void usage()
{
    std::cout <<
        "usage: fc_headless <rom.nes> [options]\n"
        "\n"
        "  --frames N        how many frames to run            (default 60)\n"
        "  --outdir DIR      write a PPM for each snapshot frame\n"
        "  --dump FILE       write the final frame as a single PPM\n"
        "  --wav FILE        write everything the APU produced, as 16 bit PCM\n"
        "  --samples FILE    write the same samples as raw float32, so two builds\n"
        "                    can be compared bit for bit\n"
        "  --save FILE       write a save state after the last frame\n"
        "  --roundtrip N     save and immediately reload at frame N. If the state\n"
        "                    is complete, the run is byte for byte the same as\n"
        "                    one without it\n"
        "  --script SPEC     press buttons at frames, as in 100:START=1,105:START=0\n"
        "  --snapshots LIST  frames to dump                    (default 1,5,30,last)\n"
        "  --quiet           no summary\n";
}

/// Returns the parsed options, or nullopt after printing why not.
std::optional<Options> parse_arguments(int argc, char** argv)
{
    Options options;

    for (int i = 1; i < argc; ++i) {
        const std::string argument = argv[i];
        const bool has_value = (i + 1) < argc;

        if (argument == "--frames" && has_value) {
            options.frames = std::stoi(argv[++i]);
        } else if (argument == "--outdir" && has_value) {
            options.out_dir = argv[++i];
        } else if (argument == "--dump" && has_value) {
            options.dump_path = argv[++i];
        } else if (argument == "--wav" && has_value) {
            options.wav_path = argv[++i];
        } else if (argument == "--samples" && has_value) {
            options.samples_path = argv[++i];
        } else if (argument == "--save" && has_value) {
            options.save_path = argv[++i];
        } else if (argument == "--roundtrip" && has_value) {
            options.roundtrip_frame = std::stoi(argv[++i]);
        } else if (argument == "--quiet") {
            options.quiet = true;
        } else if (argument == "--help" || argument == "-h") {
            usage();
            return std::nullopt;
        } else if (argument == "--script" && has_value) {
            std::string error;
            if (!parse_script(argv[++i], options.script, error)) {
                std::cerr << error << "\n";
                return std::nullopt;
            }
        } else if (argument == "--snapshots" && has_value) {
            for (const auto& entry : split(argv[++i], ',')) {
                options.snapshots.insert(std::stoi(trim(entry)));
            }
        } else if (!argument.empty() && argument[0] != '-' && options.rom_path.empty()) {
            options.rom_path = argument;
        }
    }

    if (options.rom_path.empty()) {
        usage();
        return std::nullopt;
    }

    if (options.snapshots.empty()) {
        options.snapshots = { 1, 5, 30, options.frames };
    }
    return options;
}

// ---------------------------------------------------------------------------
// Pixels
// ---------------------------------------------------------------------------

/// A binary PPM (P6). Every image tool on the planet reads it, and it is about
/// eight lines of code, which is the right amount of file format for a project
/// whose real output is a 240KB array.
bool write_ppm(const std::string& path, const uint32_t* pixels)
{
    std::ofstream out(path, std::ios::binary);
    if (!out) {
        return false;
    }

    out << "P6\n" << FC_SCREEN_WIDTH << ' ' << FC_SCREEN_HEIGHT << "\n255\n";

    for (int i = 0; i < FC_SCREEN_WIDTH * FC_SCREEN_HEIGHT; ++i) {
        const uint32_t pixel = pixels[i];
        const char rgb[3] = {
            static_cast<char>((pixel >> 16) & 0xFF),
            static_cast<char>((pixel >> 8) & 0xFF),
            static_cast<char>(pixel & 0xFF),
        };
        out.write(rgb, 3);
    }
    return true;
}

std::optional<std::vector<uint8_t>> read_file(const std::string& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return std::nullopt;
    }
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(file),
                                std::istreambuf_iterator<char>());
}

/// The same samples as raw little endian float32, with no header and no
/// quantisation.
///
/// The 16 bit WAV above is for listening; this is for comparing. A single
/// least significant bit lost to the conversion would hide exactly the kind
/// of drift a cross compilation check exists to find.
bool write_samples(const std::string& path, const std::vector<float>& samples)
{
    std::ofstream out(path, std::ios::binary);
    if (!out) {
        return false;
    }
    out.write(reinterpret_cast<const char*>(samples.data()),
              static_cast<std::streamsize>(samples.size() * sizeof(float)));
    return true;
}

/// A mono 16 bit PCM WAV.
///
/// 44 bytes of header and then the samples. The format has not changed since
/// 1991 and that is the point: it plays everywhere.
bool write_wav(const std::string& path, const std::vector<float>& samples, int sample_rate)
{
    std::ofstream out(path, std::ios::binary);
    if (!out) {
        return false;
    }

    const auto data_bytes = static_cast<uint32_t>(samples.size() * 2);

    const auto put_u32 = [&out](uint32_t value) {
        const char bytes[4] = {
            static_cast<char>(value & 0xFF),
            static_cast<char>((value >> 8) & 0xFF),
            static_cast<char>((value >> 16) & 0xFF),
            static_cast<char>((value >> 24) & 0xFF),
        };
        out.write(bytes, 4);
    };
    const auto put_u16 = [&out](uint16_t value) {
        const char bytes[2] = {
            static_cast<char>(value & 0xFF),
            static_cast<char>((value >> 8) & 0xFF),
        };
        out.write(bytes, 2);
    };

    out.write("RIFF", 4);
    put_u32(36 + data_bytes);
    out.write("WAVE", 4);
    out.write("fmt ", 4);
    put_u32(16);
    put_u16(1);                                                    // PCM
    put_u16(1);                                                    // mono
    put_u32(static_cast<uint32_t>(sample_rate));
    put_u32(static_cast<uint32_t>(sample_rate) * 2);               // bytes per second
    put_u16(2);                                                    // block align
    put_u16(16);                                                   // bits per sample
    out.write("data", 4);
    put_u32(data_bytes);

    for (const float sample : samples) {
        const float clamped = std::clamp(sample, -1.0F, 1.0F);
        put_u16(static_cast<uint16_t>(static_cast<int16_t>(clamped * 32767.0F)));
    }
    return true;
}

} // namespace

int main(int argc, char** argv)
{
    const auto options = parse_arguments(argc, argv);
    if (!options) {
        return 1;
    }

    const auto rom = read_file(options->rom_path);
    if (!rom) {
        std::cerr << "could not read " << options->rom_path << "\n";
        return 1;
    }

    fc_machine* machine = fc_create();
    if (!fc_load_rom(machine, rom->data(), rom->size())) {
        std::cerr << "could not load " << options->rom_path << ": "
                  << fc_last_error(machine) << "\n";
        fc_destroy(machine);
        return 1;
    }

    if (!options->quiet) {
        std::cout << "ROM            : " << options->rom_path << "\n";
        std::cout << "ROM size       : " << rom->size() << " bytes\n";
        std::cout << "cartridge      : " << fc_rom_summary(machine) << "\n";
        std::cout << "screen         : " << FC_SCREEN_WIDTH << "x" << FC_SCREEN_HEIGHT << "\n";
        std::cout << "sample rate    : " << fc_sample_rate() << "\n";
    }

    // Machine readable, always printed. Whether the mapper in the slot saves
    // its bank registers decides whether the round trip test below can say
    // anything about this ROM at all.
    std::cout << "mapperstate " << (fc_mapper_saves_state(machine) ? 1 : 0) << "\n";

    // Exactly what demo_ppu does before its loop, so the builds can be
    // compared byte for byte.
    fc_reset(machine);

    if (!options->out_dir.empty()) {
        std::error_code error;
        std::filesystem::create_directories(options->out_dir, error);
    }

    std::vector<float> audio(8192);

    // Everything the APU produced, kept so it can be written out at the end.
    // 600 frames is about 440,000 samples, under two megabytes; not worth
    // streaming.
    std::vector<float> collected;

    int audible_frames = 0;
    float peak = 0.0F;
    int frames_run = 0;
    bool halted = false;

    for (int frame = 1; frame <= options->frames; ++frame) {
        // The round trip. Saving and immediately reloading has to change
        // nothing at all: same registers, same RAM, same PPU phase, same APU
        // envelope positions, same mapper banks. If any one of those is
        // missing from the state, this run and the run without --roundtrip
        // diverge, and the difference shows up in the frame hashes and in the
        // samples rather than needing a separate comparison.
        if (options->roundtrip_frame > 0 && frame == options->roundtrip_frame) {
            std::size_t state_size = 0;
            uint8_t* state = fc_save_state(machine, &state_size);
            if (state == nullptr) {
                std::cerr << "could not save a state at frame " << frame << "\n";
                fc_destroy(machine);
                return 1;
            }
            if (!fc_load_state(machine, state, state_size)) {
                std::cerr << "could not load the state back at frame " << frame
                          << ": " << fc_last_error(machine) << "\n";
                fc_free_state(state);
                fc_destroy(machine);
                return 1;
            }
            fc_free_state(state);

            if (!options->quiet) {
                std::cout << "roundtrip      : saved and reloaded at frame " << frame
                          << " (" << state_size << " bytes)\n";
            }
        }

        for (const auto& event : options->script) {
            if (event.frame == frame) {
                fc_set_button(machine, event.button, event.pressed, 0);
            }
        }

        if (!fc_run_frame(machine)) {
            std::cerr << "the CPU halted at frame " << frame << " (emulator bug)\n";
            halted = true;
            break;
        }
        ++frames_run;

        // Drain the audio every frame. The APU queues samples until someone
        // takes them, so a front end that never drains would grow the queue
        // without bound. Here the samples are only measured, which is how this
        // tool can say whether the game is making noise.
        const std::size_t taken = fc_take_samples(machine, audio.data(), audio.size());
        bool loud = false;
        for (std::size_t i = 0; i < taken; ++i) {
            if (audio[i] > peak) {
                peak = audio[i];
            }
            if (audio[i] > 0.01F) {
                loud = true;
            }
        }
        if (loud) {
            ++audible_frames;
        }
        if (taken > 0 && (!options->wav_path.empty() || !options->samples_path.empty())) {
            collected.insert(collected.end(), audio.begin(), audio.begin() + static_cast<std::ptrdiff_t>(taken));
        }

        if (options->snapshots.count(frame) > 0 && !options->out_dir.empty()) {
            const std::string path = options->out_dir + "/frame_" + std::to_string(frame) + ".ppm";
            if (!write_ppm(path, fc_framebuffer(machine))) {
                std::cerr << "could not write " << path << "\n";
                fc_destroy(machine);
                return 1;
            }
        }
    }

    if (!options->dump_path.empty()) {
        if (!write_ppm(options->dump_path, fc_framebuffer(machine))) {
            std::cerr << "could not write " << options->dump_path << "\n";
            fc_destroy(machine);
            return 1;
        }
    }

    if (!options->wav_path.empty() && !write_wav(options->wav_path, collected, fc_sample_rate())) {
        std::cerr << "could not write " << options->wav_path << "\n";
        fc_destroy(machine);
        return 1;
    }

    if (!options->samples_path.empty() && !write_samples(options->samples_path, collected)) {
        std::cerr << "could not write " << options->samples_path << "\n";
        fc_destroy(machine);
        return 1;
    }

    if (!options->save_path.empty()) {
        std::size_t state_size = 0;
        uint8_t* state = fc_save_state(machine, &state_size);
        if (state == nullptr) {
            std::cerr << "could not save a state\n";
            fc_destroy(machine);
            return 1;
        }
        std::ofstream out(options->save_path, std::ios::binary);
        out.write(reinterpret_cast<const char*>(state), static_cast<std::streamsize>(state_size));
        const bool wrote = out.good();
        fc_free_state(state);

        if (!wrote) {
            std::cerr << "could not write " << options->save_path << "\n";
            fc_destroy(machine);
            return 1;
        }
        if (!options->quiet) {
            std::cout << "state written  : " << options->save_path
                      << " (" << state_size << " bytes)\n";
        }
    }

    if (!options->quiet) {
        std::cout << "frames run     : " << frames_run << "\n";
        std::cout << "frame counter  : " << fc_frame_count(machine) << "\n";
        std::cout << "cpu cycles     : " << fc_total_cycles(machine) << "\n";
        std::cout << "halted         : " << (fc_is_halted(machine) ? "true" : "false") << "\n";
        std::cout << "audio peak     : " << peak << "\n";
        std::cout << "frames audible : " << audible_frames << "\n";
        std::cout << "audio samples  : " << collected.size() << "\n";
        std::cout << "mapper state   : " << (fc_mapper_saves_state(machine) ? "saved" : "NOT saved") << "\n";
    }

    fc_destroy(machine);
    return halted ? 1 : 0;
}
