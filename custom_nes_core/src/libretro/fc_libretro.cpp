// ---------------------------------------------------------------------------
// The libretro core.
//
// This file is the whole of the project's libretro ABI. It owns no emulator of
// its own: every function below translates a libretro request into a call on
// the machine in src/core, the same machine src/ffi/emulator_api.cpp drives.
//
// Why it is a separate translation unit from the C FFI
// ----------------------------------------------------
// They are two doors into one room. `fc_*` exists for this project's own front
// end, tests and tools; `retro_*` exists so RetroArch, Lakka and every other
// libretro front end can load the emulator without knowing anything about it.
// Neither is built on the other, and neither is allowed to grow a piece of
// emulation the other lacks -- if a rule lives here and not in the core, it is
// in the wrong place.
//
// The shape of the ABI
// --------------------
// A libretro core is a process-wide singleton with callbacks in both
// directions:
//
//     front end                         core
//     ---------                         ----
//     retro_set_environment      ->     store the environment function
//     retro_set_video_refresh    ->     store where the picture goes
//     retro_set_audio_sample*    ->     store where the sound goes
//     retro_set_input_poll/state ->     store where the buttons are read
//     retro_init                 ->     build the machine
//     retro_load_game            ->     put a cartridge in
//     retro_run                  ->     one frame, then call video + audio
//     retro_serialize            <->    save states, and therefore rewind
//     retro_unload_game/deinit   ->     take it apart
//
// The callbacks the core is handed are functions in the *front end*. A button
// therefore travels: front end callback -> here -> Controller -> Bus -> CPU,
// and a pixel travels the other way. Nothing crosses without being asked for
// by name, which is what keeps the core from knowing a window exists.
//
// What is not here yet
// --------------------
// Nothing in the standard ABI. The custom extension in fc_libretro_ext.h
// carries what libretro has no place for: the raw cheat format, the debugger's
// peek and poke, and the diagnostics the status line shows.
// ---------------------------------------------------------------------------

#include "libretro.h"

#include "cheat_codes.hpp"
#include "core/nes/machine.hpp"
#include "core/types.hpp"
#include "fc_libretro_ext.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <span>
#include <string>
#include <vector>

namespace {

// ---------------------------------------------------------------------------
// The singleton
//
// libretro defines one core per process, so where the C interface keeps the
// machine inside an `fc_machine` the caller owns, this keeps exactly one here.
// ---------------------------------------------------------------------------

fc::nes::Machine* g_machine = nullptr;

/// True once the CPU has executed something it does not know. Every run after
/// that is a no-op rather than a re-execution.
bool g_halted = false;

/// What the front end must keep about each port, including "nothing plugged
/// in", which is a real setting a player can choose.
unsigned g_port_device[2] = { RETRO_DEVICE_JOYPAD, RETRO_DEVICE_JOYPAD };

// -- the front end's functions, stored by the retro_set_* calls ---------------

retro_environment_t g_environ = nullptr;
retro_video_refresh_t g_video = nullptr;
retro_audio_sample_t g_audio = nullptr;
retro_audio_sample_batch_t g_audio_batch = nullptr;
retro_input_poll_t g_input_poll = nullptr;
retro_input_state_t g_input_state = nullptr;

/// The front end's logger, when it has one. Cores are expected to stay quiet
/// when it does not.
retro_log_printf_t g_log = nullptr;

/// The loaded cartridge's one line description, kept because the extension
/// hands out a `const char*` and a temporary std::string would dangle the
/// moment the call returned.
std::string g_rom_summary;

/// Why the last retro_load_game failed, for a front end with no logger. Empty
/// after a successful load.
std::string g_last_error;

/// The last frame's APU samples, in the float form the APU produced them.
/// Filled by drain_audio and handed out by the extension's take_samples.
std::vector<fc::f32> g_last_samples;

// ---------------------------------------------------------------------------
// Constants
//
// One screen size and one region, because the NES has exactly one and this
// core does not emulate PAL. The frame rate is the NTSC value the rest of the
// libretro ecosystem uses (the PPU's 341 x 262 dots at 5.369 MHz), not a
// rounded 60 -- at 60.0 an emulator drifts a whole second every ten minutes.
// ---------------------------------------------------------------------------

constexpr int kScreenWidth = 256;
constexpr int kScreenHeight = 240;
constexpr int kBytesPerPixel = 4;
constexpr double kFrameRate = 60.0988;
constexpr double kSampleRate = 44100.0;

/// A frame is 44100 / 60.1 = 734 samples. 4096 is far more than one frame and
/// costs 16KB, so the buffers are file statics rather than something allocated
/// per run.
constexpr std::size_t kMaxSamplesPerFrame = 4096;

std::array<fc::f32, kMaxSamplesPerFrame> g_mono{};
std::array<std::int16_t, kMaxSamplesPerFrame * 2> g_stereo{};

// ---------------------------------------------------------------------------
// Logging
//
// retro_log_printf_t is variadic, but a va_list cannot be forwarded into it,
// so a message is formatted once here and handed over as a string. The extra
// copy is irrelevant: this only runs on load and on errors.
// ---------------------------------------------------------------------------

void log_message(enum retro_log_level level, const char* format, ...)
{
    if (g_log == nullptr) {
        return;
    }

    char buffer[512];
    va_list args;
    va_start(args, format);
    std::vsnprintf(buffer, sizeof(buffer), format, args);
    va_end(args);

    g_log(level, "%s", buffer);
}

// -- what the front end is told about the input, once, at load ---------------

const retro_input_descriptor kInputDescriptors[] = {
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_B, "B" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_A, "A" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_SELECT, "Select" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_START, "Start" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_UP, "Up" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_DOWN, "Down" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_LEFT, "Left" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_RIGHT, "Right" },

    { 1, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_B, "B" },
    { 1, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_A, "A" },
    { 1, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_SELECT, "Select" },
    { 1, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_START, "Start" },
    { 1, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_UP, "Up" },
    { 1, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_DOWN, "Down" },
    { 1, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_LEFT, "Left" },
    { 1, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_RIGHT, "Right" },

    { 0, 0, 0, 0, nullptr },
};

const retro_controller_description kControllerTypes[] = {
    { "Gamepad", RETRO_DEVICE_JOYPAD },
    { "None", RETRO_DEVICE_NONE },
};

const retro_controller_info kControllerInfo[] = {
    { kControllerTypes, 2 },
    { kControllerTypes, 2 },
};

// ---------------------------------------------------------------------------
// Input
//
// The only mapping that matters, and the one every NES core agrees on: the
// NES's B button is the RetroPad's B (the south face button) and the NES's A
// is the RetroPad's A (the east one). The names line up, which is exactly why
// libretro named its buttons after a SNES controller rather than an Xbox one.
// ---------------------------------------------------------------------------

/// One NES button, and where it arrives from.
struct ButtonMapping {
    unsigned retro_id;
    fc::nes::Controller::Button button;
};

constexpr ButtonMapping kButtonMap[] = {
    { RETRO_DEVICE_ID_JOYPAD_B, fc::nes::Controller::Button::B },
    { RETRO_DEVICE_ID_JOYPAD_A, fc::nes::Controller::Button::A },
    { RETRO_DEVICE_ID_JOYPAD_SELECT, fc::nes::Controller::Button::Select },
    { RETRO_DEVICE_ID_JOYPAD_START, fc::nes::Controller::Button::Start },
    { RETRO_DEVICE_ID_JOYPAD_UP, fc::nes::Controller::Button::Up },
    { RETRO_DEVICE_ID_JOYPAD_DOWN, fc::nes::Controller::Button::Down },
    { RETRO_DEVICE_ID_JOYPAD_LEFT, fc::nes::Controller::Button::Left },
    { RETRO_DEVICE_ID_JOYPAD_RIGHT, fc::nes::Controller::Button::Right },
};

/// Ask the front end what is held, and put both controllers where that says.
///
/// The release first is not optional: a libretro front end reports a button
/// going down, or not, and a machine that is only ever told about the buttons
/// that are down never hears about one coming up.
void apply_input()
{
    if (g_machine == nullptr || g_input_state == nullptr) {
        return;
    }

    for (int port = 0; port < 2; ++port) {
        auto& controller = g_machine->controller(port);
        controller.release_all();

        if (g_port_device[port] == RETRO_DEVICE_NONE) {
            continue;
        }

        for (const ButtonMapping& mapping : kButtonMap) {
            const bool pressed =
                g_input_state(static_cast<unsigned>(port), RETRO_DEVICE_JOYPAD, 0,
                              mapping.retro_id) != 0;
            controller.set_button(mapping.button, pressed);
        }
    }
}

// ---------------------------------------------------------------------------
// Audio
//
// The core produces mono floating point at 44100 Hz; libretro wants signed
// 16-bit stereo. The two conversions are both lossy in a way that is worth
// stating once:
//
//   * mono -> stereo is a copy, because the hardware has one speaker
//   * float -> int16 is a clamp and a scale, and the clamp is not decoration:
//     a sample outside [-1, 1] wraps to the opposite sign in a cast, which is
//     the click people blame on their speakers
// ---------------------------------------------------------------------------

void drain_audio()
{
    // Last frame's samples, kept for the custom extension. Cleared first so a
    // frame that produces nothing does not hand out the frame before it.
    g_last_samples.clear();

    if (g_machine == nullptr) {
        return;
    }

    const std::size_t taken =
        g_machine->apu().drain(g_mono.data(), kMaxSamplesPerFrame);
    if (taken == 0) {
        return;
    }

    // Retained in the APU's own float form, before the int16 conversion below
    // rounds the low bits away. A front end that compares samples byte for
    // byte asks for these through the extension; everyone else gets the int16
    // the libretro ABI specifies.
    g_last_samples.assign(g_mono.begin(), g_mono.begin() + taken);

    if (g_audio_batch == nullptr && g_audio == nullptr) {
        return;
    }

    for (std::size_t i = 0; i < taken; ++i) {
        fc::f32 sample = g_mono[i];
        if (sample > 1.0f) {
            sample = 1.0f;
        } else if (sample < -1.0f) {
            sample = -1.0f;
        }

        const auto value = static_cast<std::int16_t>(std::lround(sample * 32767.0f));
        g_stereo[i * 2] = value;
        g_stereo[i * 2 + 1] = value;
    }

    if (g_audio_batch != nullptr) {
        g_audio_batch(g_stereo.data(), taken);
        return;
    }
    for (std::size_t i = 0; i < taken; ++i) {
        g_audio(g_stereo[i * 2], g_stereo[i * 2 + 1]);
    }
}

// ---------------------------------------------------------------------------
// The custom extension
//
// These are the functions a libretro front end cannot reach through the
// standard ABI. See fc_libretro_ext.h for why each one exists and for the
// rules on changing the table.
// ---------------------------------------------------------------------------

int ext_peek(uint16_t address)
{
    if (g_machine == nullptr) {
        return 0;
    }
    return g_machine->bus().peek(address);
}

void ext_poke(uint16_t address, uint8_t value)
{
    if (g_machine == nullptr) {
        return;
    }
    // Through the bus, so $075A and $0F5A are the same byte here as they are
    // to the CPU. A debugger that wrote the RAM array directly would be a
    // second, subtly different address decoder.
    g_machine->bus().write(address, value);
}

int ext_set_raw_cheats(const uint8_t* data, int count)
{
    // Cheats are meaningless without a cartridge to write them into, and the
    // header promises -1 rather than a silent success.
    if (g_machine == nullptr || g_machine->cartridge() == nullptr) {
        return -1;
    }

    std::vector<fc::nes::Cheat> cheats;
    if (data != nullptr && count > 0) {
        cheats.reserve(static_cast<std::size_t>(count));
        for (int i = 0; i < count; ++i) {
            const uint8_t* entry = data + static_cast<std::size_t>(i) * 4;
            cheats.push_back(fc::nes::Cheat{
                static_cast<uint16_t>(entry[0] | (entry[1] << 8)),
                entry[2],
                (entry[3] & 0x01u) != 0u,
                (entry[3] & 0x02u) != 0u,
            });
        }
    }

    g_machine->cheats().set(cheats);
    return static_cast<int>(g_machine->cheats().size());
}

int ext_raw_cheat_count()
{
    return g_machine == nullptr ? 0 : static_cast<int>(g_machine->cheats().size());
}

bool ext_mapper_saves_state()
{
    return g_machine != nullptr && g_machine->mapper_saves_state();
}

const char* ext_rom_summary()
{
    return g_rom_summary.c_str();
}

uint64_t ext_total_cycles()
{
    return g_machine == nullptr ? 0 : g_machine->cpu().total_cycles();
}

uint16_t ext_cpu_pc()
{
    return g_machine == nullptr ? 0 : g_machine->cpu().registers().pc;
}

size_t ext_take_samples(float* out, size_t max)
{
    if (out == nullptr || max == 0) {
        return 0;
    }

    const size_t count = std::min(max, g_last_samples.size());
    std::copy_n(g_last_samples.begin(), count, out);
    // Drained, like the libretro audio queue: the next frame's call returns
    // the next frame's samples.
    g_last_samples.clear();
    return count;
}

const char* ext_last_error()
{
    return g_last_error.c_str();
}

/// The table itself. Field order has to match the header exactly, which the
/// compiler checks as long as every field is initialized -- and it will warn
/// if one is not.
const fc_libretro_ext_v1 kExt = {
    FC_LIBRETRO_EXT_VERSION,
    sizeof(fc_libretro_ext_v1),
    ext_peek,
    ext_poke,
    ext_set_raw_cheats,
    ext_raw_cheat_count,
    ext_mapper_saves_state,
    ext_rom_summary,
    ext_total_cycles,
    ext_cpu_pc,
    ext_take_samples,
    ext_last_error,
};

// ---------------------------------------------------------------------------
// Cheat codes
//
// libretro hands each code over by slot index and can disable one without
// removing it, so the core remembers every code by index and rebuilds the
// machine's two lists whenever anything changes. Rebuilding is cheap and
// happens on load and on a settings edit, never in the frame loop.
// ---------------------------------------------------------------------------

struct CheatSlot {
    bool enabled = false;
    std::string code;
};

std::vector<CheatSlot> g_cheats;

/// Turn the remembered code strings into the machine's two mechanisms.
///
/// A Game Genie code becomes a PRG patch on the cartridge; a Pro Action
/// Replay code becomes a frozen RAM byte. They are separate lists because
/// they are separate hardware ideas, and a machine can hold both.
void rebuild_cheats()
{
    // The machine's cheat lists are only meaningful with a cartridge to apply
    // them to, but the strings are remembered either way, so a front end that
    // sets a code before loading still gets it.
    if (g_machine == nullptr || g_machine->cartridge() == nullptr) {
        return;
    }

    std::vector<fc::nes::Cartridge::PrgPatch> patches;
    std::vector<fc::nes::Cheat> ram_cheats;

    for (const CheatSlot& slot : g_cheats) {
        if (!slot.enabled) {
            continue;
        }

        const fc::libretro::DecodedCheat decoded = fc::libretro::decode_cheat(slot.code);
        if (!decoded.ok) {
            log_message(RETRO_LOG_WARN, "Classic Game Box: not a cheat code: %s",
                        slot.code.c_str());
            continue;
        }

        if (decoded.rom_patch) {
            patches.push_back(fc::nes::Cartridge::PrgPatch{
                decoded.address, decoded.value, decoded.compare,
            });
        } else {
            ram_cheats.push_back(fc::nes::Cheat{
                decoded.address, decoded.value, /*freeze=*/true, /*enabled=*/true,
            });
        }
    }

    g_machine->cartridge()->set_prg_patches(patches);
    g_machine->cheats().set(ram_cheats);

    if (!patches.empty() || !ram_cheats.empty()) {
        log_message(RETRO_LOG_INFO, "Classic Game Box: %zu ROM patches, %zu RAM cheats",
                    patches.size(), ram_cheats.size());
    }
}

// ---------------------------------------------------------------------------
// The memory map the front end searches
//
// A cheat search needs to know which emulated addresses are backed by which
// host bytes, or it cannot turn "this byte changed" into an address a code
// can name. The descriptors live at file scope because the front end is
// promised the pointers stay valid, not just valid for the call.
// ---------------------------------------------------------------------------

retro_memory_descriptor g_memory_descriptors[2]{};
retro_memory_map g_memory_map{};

void publish_memory_maps()
{
    if (g_environ == nullptr || g_machine == nullptr) {
        return;
    }

    unsigned count = 0;

    // Console RAM: 2KB at $0000. The mirrors at $0800 and up are the same
    // bytes, and a search that walks them would report each hit four times,
    // so only the first range is published.
    g_memory_descriptors[count].flags = RETRO_MEMDESC_SYSTEM_RAM;
    g_memory_descriptors[count].ptr = g_machine->bus().ram().data();
    g_memory_descriptors[count].offset = 0;
    g_memory_descriptors[count].start = 0x0000;
    g_memory_descriptors[count].select = 0;
    g_memory_descriptors[count].disconnect = 0;
    g_memory_descriptors[count].len = fc::nes::Ram::kSize;
    g_memory_descriptors[count].addrspace = nullptr;
    ++count;

    // The cartridge's save RAM, when it has one a player would expect to keep.
    fc::nes::Cartridge* cartridge = g_machine->cartridge();
    if (cartridge != nullptr && cartridge->battery_backed()) {
        g_memory_descriptors[count].flags = RETRO_MEMDESC_SAVE_RAM;
        g_memory_descriptors[count].ptr = cartridge->prg_ram().data();
        g_memory_descriptors[count].offset = 0;
        g_memory_descriptors[count].start = fc::nes::Cartridge::kPrgRamBase;
        g_memory_descriptors[count].select = 0;
        g_memory_descriptors[count].disconnect = 0;
        g_memory_descriptors[count].len = cartridge->prg_ram().size();
        g_memory_descriptors[count].addrspace = nullptr;
        ++count;
    }

    g_memory_map.descriptors = g_memory_descriptors;
    g_memory_map.num_descriptors = count;
    g_environ(RETRO_ENVIRONMENT_SET_MEMORY_MAPS, &g_memory_map);
}

} // namespace

// ---------------------------------------------------------------------------
// The ABI
// ---------------------------------------------------------------------------

extern "C" {

RETRO_API unsigned retro_api_version(void)
{
    return RETRO_API_VERSION;
}

RETRO_API void retro_get_system_info(struct retro_system_info* info)
{
    if (info == nullptr) {
        return;
    }

    std::memset(info, 0, sizeof(*info));
    info->library_name = "Classic Game Box";
#ifdef FC_LIBRETRO_VERSION
    info->library_version = FC_LIBRETRO_VERSION;
#else
    info->library_version = "0.0.0";
#endif
    info->valid_extensions = "nes";

    // The whole ROM is handed to us in memory, so the front end may patch it,
    // decompress it, or hand it over from somewhere that is not a file.
    info->need_fullpath = false;
    info->block_extract = false;
}

RETRO_API void retro_get_system_av_info(struct retro_system_av_info* info)
{
    if (info == nullptr) {
        return;
    }

    std::memset(info, 0, sizeof(*info));
    info->geometry.base_width = kScreenWidth;
    info->geometry.base_height = kScreenHeight;
    info->geometry.max_width = kScreenWidth;
    info->geometry.max_height = kScreenHeight;
    // Zero means "the front end may assume square pixels from base_width and
    // base_height", which is the same assumption this project's own front end
    // makes when it scales by whole pixels.
    info->geometry.aspect_ratio = 0.0f;
    info->timing.fps = kFrameRate;
    info->timing.sample_rate = kSampleRate;
}

RETRO_API void retro_set_controller_port_device(unsigned port, unsigned device)
{
    if (port > 1) {
        return;
    }

    g_port_device[port] = device;
    if (device == RETRO_DEVICE_NONE && g_machine != nullptr) {
        g_machine->controller(static_cast<int>(port)).release_all();
    }
}

RETRO_API void retro_reset(void)
{
    if (g_machine == nullptr) {
        return;
    }
    g_machine->reset();
    g_halted = false;
}

RETRO_API void retro_run(void)
{
    if (g_machine == nullptr) {
        return;
    }

    if (g_input_poll != nullptr) {
        g_input_poll();
    }
    apply_input();

    if (!g_halted && !g_machine->run_frame()) {
        // An illegal opcode: the emulator has a bug, not the game. Say so once
        // and then keep handing the front end the last picture so it does not
        // wait for a frame that will never come.
        g_halted = true;
        log_message(RETRO_LOG_ERROR, "Classic Game Box: the CPU halted on an unknown opcode");
    }

    if (g_video != nullptr) {
        const auto& framebuffer = g_machine->framebuffer();
        g_video(framebuffer.pixels.data(), kScreenWidth, kScreenHeight,
                static_cast<std::size_t>(kScreenWidth) * kBytesPerPixel);
    }

    drain_audio();
}

RETRO_API size_t retro_serialize_size(void)
{
    if (g_machine == nullptr || g_machine->cartridge() == nullptr) {
        return 0;
    }
    return g_machine->state_size();
}

RETRO_API bool retro_serialize(void* data, size_t len)
{
    if (g_machine == nullptr || data == nullptr || len == 0) {
        return false;
    }

    const std::span<fc::u8> buffer(static_cast<fc::u8*>(data), len);
    return g_machine->save_state_into(buffer) != 0;
}

RETRO_API bool retro_unserialize(const void* data, size_t len)
{
    if (g_machine == nullptr || data == nullptr || len == 0) {
        return false;
    }

    const std::span<const fc::u8> buffer(static_cast<const fc::u8*>(data), len);
    if (!g_machine->load_state(buffer)) {
        return false;
    }

    // A restored machine is a running one, whatever it was doing before.
    g_halted = false;
    return true;
}

RETRO_API void retro_cheat_reset(void)
{
    g_cheats.clear();

    // Take the patches back out of the running machine as well as out of the
    // list, or a disabled cheat would keep working.
    if (g_machine != nullptr) {
        g_machine->cheats().clear();
        if (g_machine->cartridge() != nullptr) {
            g_machine->cartridge()->clear_prg_patches();
        }
    }
}

RETRO_API void retro_cheat_set(unsigned index, bool enabled, const char* code)
{
    if (code == nullptr) {
        return;
    }

    // The front end indexes cheats, and can switch one off without telling the
    // core what the others are. A slot that has never been written is simply
    // not there, which is why the list is grown rather than required to be
    // dense.
    if (g_cheats.size() <= index) {
        g_cheats.resize(static_cast<std::size_t>(index) + 1);
    }
    g_cheats[index].enabled = enabled;
    g_cheats[index].code = code;

    rebuild_cheats();
}

RETRO_API bool retro_load_game(const struct retro_game_info* game)
{
    g_last_error.clear();

    if (game == nullptr || game->data == nullptr || game->size == 0) {
        g_last_error = "no ROM data was provided";
        log_message(RETRO_LOG_ERROR, "Classic Game Box: %s", g_last_error.c_str());
        return false;
    }

    // A front end that cannot show XRGB8888 cannot show this core's picture.
    // Asking before loading keeps the failure at the door.
    enum retro_pixel_format format = RETRO_PIXEL_FORMAT_XRGB8888;
    if (g_environ != nullptr &&
        !g_environ(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, &format)) {
        g_last_error = "the front end does not support XRGB8888";
        log_message(RETRO_LOG_ERROR, "Classic Game Box: %s", g_last_error.c_str());
        return false;
    }
    if (g_machine == nullptr) {
        g_machine = new fc::nes::Machine();
    }

    const std::span<const fc::u8> rom(static_cast<const fc::u8*>(game->data),
                                      game->size);
    std::string error;
    if (!g_machine->load_rom(rom, error)) {
        // The reason, kept for a front end that has no logger. On the wasm
        // side this is how "mapper 176 is not implemented yet" reaches the
        // player instead of a bare false.
        g_last_error = error;
        log_message(RETRO_LOG_ERROR, "Classic Game Box: %s", error.c_str());
        return false;
    }

    g_machine->reset();
    g_halted = false;

    if (g_environ != nullptr) {
        g_environ(RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS,
                  const_cast<retro_input_descriptor*>(kInputDescriptors));
        g_environ(RETRO_ENVIRONMENT_SET_CONTROLLER_INFO,
                  const_cast<retro_controller_info*>(kControllerInfo));
    }

    const auto* cartridge = g_machine->cartridge();
    if (cartridge != nullptr) {
        g_rom_summary = cartridge->summary();
        log_message(RETRO_LOG_INFO, "Classic Game Box: %s", g_rom_summary.c_str());
    }

    // Both of these depend on the cartridge that just went in: the memory map
    // on whether it has save RAM, the cheats on which addresses its PRG space
    // answers. A front end may have set cheats before loading, so they are
    // applied here rather than only on the next retro_cheat_set.
    publish_memory_maps();
    rebuild_cheats();
    return true;
}

RETRO_API bool retro_load_game_special(unsigned game_type,
                                        const struct retro_game_info* info,
                                        size_t num_info)
{
    (void)game_type;
    (void)info;
    (void)num_info;
    // The NES has no subsystems: one cartridge, one machine.
    return false;
}

RETRO_API void retro_unload_game(void)
{
    delete g_machine;
    g_machine = nullptr;
    g_halted = false;
    g_rom_summary.clear();
    g_last_error.clear();
    // Codes are the front end's, but the patches they became were this
    // machine's. The next cartridge starts with neither.
    g_cheats.clear();
}

RETRO_API unsigned retro_get_region(void)
{
    return RETRO_REGION_NTSC;
}

RETRO_API void* retro_get_memory_data(unsigned id)
{
    if (g_machine == nullptr) {
        return nullptr;
    }

    switch (id & RETRO_MEMORY_MASK) {
    case RETRO_MEMORY_SYSTEM_RAM:
        // The 2KB the CPU sees at $0000-$07FF, for cheat search and for a
        // debugger. The CPU only reaches it through the mask in Ram; this is
        // the view for everything that is not the CPU.
        return g_machine->bus().ram().data();

    case RETRO_MEMORY_SAVE_RAM: {
        // The game's save file, and only when the cartridge says it keeps it.
        // A board whose $6000 is registers rather than RAM has no save to
        // hand over, and handing over the unused buffer would write an empty
        // file as though it were the player's progress.
        fc::nes::Cartridge* cartridge = g_machine->cartridge();
        if (cartridge == nullptr || !cartridge->battery_backed()) {
            return nullptr;
        }
        return cartridge->prg_ram().data();
    }

    default:
        return nullptr;
    }
}

RETRO_API size_t retro_get_memory_size(unsigned id)
{
    if (g_machine == nullptr) {
        return 0;
    }

    switch (id & RETRO_MEMORY_MASK) {
    case RETRO_MEMORY_SYSTEM_RAM:
        return fc::nes::Ram::kSize;

    case RETRO_MEMORY_SAVE_RAM: {
        const fc::nes::Cartridge* cartridge = g_machine->cartridge();
        if (cartridge == nullptr || !cartridge->battery_backed()) {
            return 0;
        }
        return cartridge->prg_ram().size();
    }

    default:
        return 0;
    }
}

// -- lifecycle ---------------------------------------------------------------

RETRO_API void retro_set_environment(retro_environment_t cb)
{
    g_environ = cb;
}

RETRO_API void retro_set_video_refresh(retro_video_refresh_t cb)
{
    g_video = cb;
}

RETRO_API void retro_set_audio_sample(retro_audio_sample_t cb)
{
    g_audio = cb;
}

RETRO_API void retro_set_audio_sample_batch(retro_audio_sample_batch_t cb)
{
    g_audio_batch = cb;
}

RETRO_API void retro_set_input_poll(retro_input_poll_t cb)
{
    g_input_poll = cb;
}

RETRO_API void retro_set_input_state(retro_input_state_t cb)
{
    g_input_state = cb;
}

RETRO_API void retro_init(void)
{
    if (g_machine == nullptr) {
        g_machine = new fc::nes::Machine();
    }
    g_halted = false;

    // A fresh core has a gamepad in each port, whatever a previous session
    // left behind. The front end says otherwise with
    // retro_set_controller_port_device() if it wants to.
    g_port_device[0] = RETRO_DEVICE_JOYPAD;
    g_port_device[1] = RETRO_DEVICE_JOYPAD;

    // The environment function was handed over before this point, which is why
    // the log interface can be asked for here rather than at load.
    if (g_environ != nullptr) {
        retro_log_callback logger{};
        if (g_environ(RETRO_ENVIRONMENT_GET_LOG_INTERFACE, &logger)) {
            g_log = logger.log;
        }
    }

    log_message(RETRO_LOG_INFO, "Classic Game Box: core initialised");
}

RETRO_API void retro_deinit(void)
{
    delete g_machine;
    g_machine = nullptr;
    g_halted = false;
    g_rom_summary.clear();
    g_last_error.clear();
    g_cheats.clear();
    g_environ = nullptr;
    g_video = nullptr;
    g_audio = nullptr;
    g_audio_batch = nullptr;
    g_input_poll = nullptr;
    g_input_state = nullptr;
    g_log = nullptr;
}

// -- the custom extension ----------------------------------------------------

/// The one symbol a standard front end never asks for and this project's own
/// front end does. See fc_libretro_ext.h.
const fc_libretro_ext_v1* fc_libretro_get_ext(void)
{
    return &kExt;
}

} // extern "C"
