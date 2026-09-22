// ---------------------------------------------------------------------------
// demo_apu - the five voices, and what a real game does with them
//
// Build:  cmake --build build
// Run:    ./build/demo_apu [path/to/game.nes] [output-dir]
//
// Read together with docs/nes/apu.md
// ---------------------------------------------------------------------------

#include "core/nes/apu.hpp"
#include "core/nes/controller.hpp"
#include "core/nes/machine.hpp"
#include "core/types.hpp"

#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <optional>
#include <string>
#include <vector>

using namespace fc;

namespace {

void title(const std::string& text)
{
    std::cout << "\n=== " << text << " ===\n";
}

std::optional<std::vector<u8>> read_file(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return std::nullopt;
    }
    return std::vector<u8>(std::istreambuf_iterator<char>(file),
                           std::istreambuf_iterator<char>());
}

std::optional<std::filesystem::path> find_rom(const std::filesystem::path& requested)
{
    const std::filesystem::path candidates[] = {
        requested,
        "packages/fc-core/tests/data/super-mario-bros.nes",
        "/Users/suke/Downloads/超级玛丽.nes",
    };
    for (const auto& path : candidates) {
        if (path.empty()) {
            continue;
        }
        std::error_code error;
        if (std::filesystem::exists(path, error)) {
            return path;
        }
    }
    return std::nullopt;
}

// ---------------------------------------------------------------------------
// A 16 bit mono WAV. About forty lines, which is the right size for a format
// this old: everything is little endian and every chunk is length prefixed.
// ---------------------------------------------------------------------------

void put_u32(std::ofstream& out, u32 value)
{
    const char bytes[4] = {
        static_cast<char>(value & 0xFF),
        static_cast<char>((value >> 8) & 0xFF),
        static_cast<char>((value >> 16) & 0xFF),
        static_cast<char>((value >> 24) & 0xFF),
    };
    out.write(bytes, 4);
}

void put_u16(std::ofstream& out, u16 value)
{
    const char bytes[2] = {
        static_cast<char>(value & 0xFF),
        static_cast<char>((value >> 8) & 0xFF),
    };
    out.write(bytes, 2);
}

bool write_wav(const std::filesystem::path& path, const std::vector<f32>& samples,
               int sample_rate)
{
    std::ofstream out(path, std::ios::binary);
    if (!out) {
        return false;
    }

    const u32 data_bytes = static_cast<u32>(samples.size() * 2u);

    out.write("RIFF", 4);
    put_u32(out, 36u + data_bytes);
    out.write("WAVE", 4);

    out.write("fmt ", 4);
    put_u32(out, 16u);                                  // chunk size
    put_u16(out, 1u);                                   // PCM
    put_u16(out, 1u);                                   // mono
    put_u32(out, static_cast<u32>(sample_rate));
    put_u32(out, static_cast<u32>(sample_rate) * 2u);   // byte rate
    put_u16(out, 2u);                                   // block align
    put_u16(out, 16u);                                  // bits per sample

    out.write("data", 4);
    put_u32(out, data_bytes);

    // The APU's mixer is unipolar: 0 is silence and sound only ever pushes
    // it up. On the real console a capacitor blocks that constant offset, so
    // what reaches the speaker is the signal with its DC removed. This is
    // that capacitor: a one pole high pass. Without it the recording would
    // sit at a constant offset and only use half its range.
    f32 previous_input = 0.0f;
    f32 previous_output = 0.0f;

    for (f32 sample : samples) {
        const f32 filtered = sample - previous_input + 0.995f * previous_output;
        previous_input = sample;
        previous_output = filtered;

        f32 clamped = filtered;
        if (clamped < -1.0f) clamped = -1.0f;
        if (clamped > 1.0f) clamped = 1.0f;
        put_u16(out, static_cast<u16>(static_cast<s16>(clamped * 32767.0f)));
    }

    return true;
}

// ---------------------------------------------------------------------------
// Drawing a waveform in a terminal
// ---------------------------------------------------------------------------

void print_waveform(const std::string& label, const std::vector<u8>& values, u8 peak)
{
    std::cout << "  " << label << "\n    ";

    const int height = 6;
    for (int row = height; row >= 1; --row) {
        if (row != height) {
            std::cout << "    ";
        }
        for (u8 value : values) {
            const int level = (peak == 0) ? 0 : (static_cast<int>(value) * height / peak);
            std::cout << (level >= row ? '#' : ' ');
        }
        std::cout << "\n";
    }
    std::cout << "    " << std::string(values.size(), '-') << "\n";
}

// --- 1 ---------------------------------------------------------------------
void showChannels()
{
    title("1. Five voices, and almost no features");

    std::cout << "  The APU is not a separate chip. It is on the same die as the CPU,\n";
    std::cout << "  in the 2A03, which is why it shares the CPU's clock and its address\n";
    std::cout << "  space.\n\n";

    struct Row { const char* name; const char* range; const char* what; };
    const Row rows[] = {
        { "Pulse 1",  "$4000-$4003", "square wave, four duty cycles" },
        { "Pulse 2",  "$4004-$4007", "the same, and it can be detuned" },
        { "Triangle", "$4008-$400B", "triangle wave, NO volume control at all" },
        { "Noise",    "$400C-$400F", "a shift register pretending to be random" },
        { "DMC",      "$4010-$4013", "delta modulated samples out of CPU memory" },
    };

    std::cout << "  channel    registers     what\n";
    std::cout << "  ---------  ------------  --------------------------------------\n";
    for (const auto& r : rows) {
        std::cout << "  " << std::left << std::setw(11) << r.name
                  << std::setw(14) << r.range << r.what << "\n";
    }
    std::cout << std::right;

    std::cout << "\n  Plus two registers that belong to the whole APU:\n\n";
    std::cout << "    $4015  write: which channels are enabled   read: which are playing\n";
    std::cout << "    $4017  the frame sequencer: 4 or 5 steps, and the IRQ inhibit\n";
}

// --- 2 ---------------------------------------------------------------------
void showDutyCycles()
{
    title("2. A square wave is eight steps of a shift register");

    std::cout << "  A pulse channel does not generate a wave. It reloads its output from\n";
    std::cout << "  one of four eight-bit patterns, once per timer expiry. The pattern is\n";
    std::cout << "  the duty cycle, and there are only four of them.\n\n";

    for (u8 duty = 0; duty < 4; ++duty) {
        nes::Apu apu;
        apu.write(0x4015, 0x01);
        apu.write(0x4000, static_cast<u8>((duty << 6) | 0x20 | 0x10 | 0x0F));
        apu.write(0x4002, 0x40);   // a low timer, so the pattern is easy to see
        apu.write(0x4003, 0x01);

        std::vector<u8> wave;
        for (int i = 0; i < 64; ++i) {
            wave.push_back(apu.pulse1().output());
            for (int t = 0; t < 66; ++t) {
                apu.tick();
            }
        }

        const char* names[4] = { "12.5%", "25%", "50%", "25% negated" };
        print_waveform(std::string("duty ") + std::to_string(duty) + "  (" + names[duty] + ")",
                       wave, 15);
        std::cout << "\n";
    }

    std::cout << "  The last one is the third one upside down. That is the whole\n";
    std::cout << "  difference, and it is the difference between the two lead voices in\n";
    std::cout << "  most NES music.\n";
}

// --- 3 ---------------------------------------------------------------------
void showEnvelope()
{
    title("3. The envelope: the only instrument the hardware has");

    std::cout << "  There is no attack, decay, sustain or release. There is a down\n";
    std::cout << "  counter that starts at 15 and steps down by one every so often, and\n";
    std::cout << "  a flag that says whether to loop back to 15 or stop at 0.\n\n";

    nes::Apu apu;
    apu.write(0x4015, 0x01);
    apu.write(0x4000, 0x20 | 0x0F);   // halt/loop set, envelope period 15 -> slowest
    apu.write(0x4002, 0x40);
    apu.write(0x4003, 0x00);

    // Sample within each frame step and keep the peak, so what the chart
    // shows is the envelope rather than the duty cycle multiplied by it.
    std::vector<u8> levels;
    for (int step = 0; step < 72; ++step) {
        u8 level = 0;
        for (int k = 0; k < 60; ++k) {
            apu.tick_cpu(249);   // 60 x 249 is about one frame step
            level = std::max(level, apu.pulse1().output());
        }
        levels.push_back(level);
    }

    print_waveform("the envelope's level, one column per frame step", levels, 15);

    std::cout << "\n  That is a decaying note. Every brass stab, every coin, every\n";
    std::cout << "  jump sound in every NES game is some arrangement of this counter\n";
    std::cout << "  plus a length counter.\n";
}

// --- 4 ---------------------------------------------------------------------
void showNoise()
{
    title("4. Noise is a shift register, not a random number generator");

    std::cout << "  Fifteen bits, with bit 0 XORed against bit 1 and fed back into bit\n";
    std::cout << "  14. It is completely deterministic, which is why a recording of\n";
    std::cout << "  Super Mario Bros sounds the same every time.\n\n";

    nes::Apu apu;
    apu.write(0x4015, 0x08);
    apu.write(0x400C, 0x3F);   // constant volume 15
    apu.write(0x400E, 0x04);   // mid period, 15 bit mode
    apu.write(0x400F, 0x00);

    std::vector<u8> wave;
    for (int i = 0; i < 96; ++i) {
        wave.push_back(apu.noise().output());
        apu.tick_cpu(80);
    }
    print_waveform("15 bit mode (hiss)", wave, 15);

    apu.write(0x400E, 0x84);   // 6 bit mode
    std::vector<u8> metallic;
    for (int i = 0; i < 96; ++i) {
        metallic.push_back(apu.noise().output());
        apu.tick_cpu(80);
    }
    print_waveform("6 bit mode (metallic)", metallic, 15);

    std::cout << "\n  Only one bit differs between the two modes, but they sound\n";
    std::cout << "  completely different: one is a snare, the other is a laser.\n";
}

// --- 5 ---------------------------------------------------------------------
void showTriangle()
{
    title("5. The triangle has no volume control at all");

    nes::Apu apu;
    apu.write(0x4015, 0x04);
    apu.write(0x4008, 0xFF);   // control on, linear counter 127
    apu.write(0x400A, 0x40);
    apu.write(0x400B, 0x00);
    apu.tick_cpu(20000);       // let the linear counter reload

    std::vector<u8> wave;
    for (int i = 0; i < 64; ++i) {
        wave.push_back(apu.triangle().output());
        for (int t = 0; t < 66; ++t) {
            apu.tick();
        }
    }
    print_waveform("the triangle's 32 step sequence", wave, 15);

    std::cout << "\n  It is either on or off. The bass line in NES music is a triangle\n";
    std::cout << "  because it is the only channel with a smooth waveform, and the only\n";
    std::cout << "  way to make it quieter is to stop and start it very quickly. Games\n";
    std::cout << "  that do that are how you can tell a triangle kick drum from a bass\n";
    std::cout << "  note.\n";
}

// --- 6 ---------------------------------------------------------------------
bool play_the_game(nes::Machine& machine)
{
    for (int i = 0; i < 400; ++i) {
        if (!machine.run_frame()) {
            return false;
        }
    }
    machine.set_button(nes::Controller::Button::Start, true);
    for (int i = 0; i < 5; ++i) {
        (void)machine.run_frame();
    }
    machine.set_button(nes::Controller::Button::Start, false);
    return true;
}

void exportAudio(nes::Machine& machine, const std::filesystem::path& out_dir)
{
    title("6. A real game's audio");

    std::cout << "  Pressed Start, now recording 10 seconds...\n\n";

    (void)machine.apu().take_samples();

    std::vector<f32> all;
    double energy = 0.0;
    f32 peak = 0.0f;
    int frames_with_sound = 0;

    for (int frame = 0; frame < 600; ++frame) {
        if (!machine.run_frame()) {
            std::cout << "  the CPU halted at frame " << frame << "\n";
            break;
        }

        auto samples = machine.apu().take_samples();
        bool loud = false;
        for (f32 sample : samples) {
            energy += static_cast<double>(sample) * static_cast<double>(sample);
            peak = std::max(peak, sample);
            if (sample > 0.01f) {
                loud = true;
            }
        }
        if (loud) {
            ++frames_with_sound;
        }
        all.insert(all.end(), samples.begin(), samples.end());
    }

    const double rms = all.empty() ? 0.0 : std::sqrt(energy / static_cast<double>(all.size()));

    std::cout << "  samples        : " << all.size() << "  ("
              << std::fixed << std::setprecision(2)
              << (static_cast<double>(all.size()) / nes::Apu::kSampleRate) << " seconds)\n";
    std::cout << "  peak           : " << peak << "\n";
    std::cout << "  rms            : " << rms << "\n";
    std::cout << "  frames audible : " << frames_with_sound << " of 600\n";
    std::cout << "  channels on    : $" << std::hex << std::setw(2)
              << std::setfill('0') << static_cast<int>(machine.apu().enabled_channels())
              << std::setfill(' ') << std::dec
              << "   (bit 0 pulse 1, 1 pulse 2, 2 triangle, 3 noise, 4 DMC)\n\n";

    struct Row { const char* name; u8 value; };
    const Row rows[] = {
        { "Pulse 1 ", machine.apu().pulse1().output() },
        { "Pulse 2 ", machine.apu().pulse2().output() },
        { "Triangle", machine.apu().triangle().output() },
        { "Noise   ", machine.apu().noise().output() },
        { "DMC     ", machine.apu().dmc().output() },
    };
    std::cout << "  What each voice is doing right now (0-15, DMC is 0-127):\n";
    for (const auto& r : rows) {
        std::cout << "    " << r.name << "  " << std::setw(3)
                  << static_cast<int>(r.value) << "\n";
    }

    const auto path = out_dir / "game_audio.wav";
    if (write_wav(path, all, nes::Apu::kSampleRate)) {
        std::cout << "\n  wrote " << path.string() << "\n";
        std::cout << "  play it with:  afplay " << path.string() << "\n";
    } else {
        std::cout << "\n  could not write " << path.string() << "\n";
    }
}

} // namespace

int main(int argc, char** argv)
{
    std::cout << "Classic Game Box - Phase 6: the APU\n";

    const std::filesystem::path requested = (argc > 1) ? argv[1] : "";
    const std::filesystem::path out_dir = (argc > 2) ? argv[2] : "frames";

    std::error_code fs_error;
    std::filesystem::create_directories(out_dir, fs_error);

    showChannels();
    showDutyCycles();
    showEnvelope();
    showNoise();
    showTriangle();

    const auto path = find_rom(requested);
    if (!path) {
        std::cout << "\n  No ROM found, skipping the real game section.\n";
        return 0;
    }

    const auto rom = read_file(*path);
    if (!rom) {
        std::cout << "\n  Could not read " << path->string() << "\n";
        return 1;
    }

    nes::Machine machine;
    std::string error;
    if (!machine.load_rom(*rom, error)) {
        std::cout << "\n  Could not load: " << error << "\n";
        return 1;
    }

    std::cout << "\n  ROM: " << path->string() << "\n";

    if (!play_the_game(machine)) {
        std::cout << "\n  the CPU halted before the game started\n";
        return 1;
    }

    exportAudio(machine, out_dir);

    title("Summary");
    std::cout << "The APU is five tiny dedicated oscillators on the CPU die.\n";
    std::cout << "Two squares, a triangle, a shift register and a delta modulator.\n";
    std::cout << "It runs at half the CPU clock; its frame sequencer runs at 240 Hz.\n";
    std::cout << "There is no filter, no mixer and no instrument: composers built\n";
    std::cout << "those in software on top of these five very dumb voices.\n";

    return 0;
}
