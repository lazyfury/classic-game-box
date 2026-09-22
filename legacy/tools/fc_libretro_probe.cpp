// ---------------------------------------------------------------------------
// fc_libretro_probe - load the libretro core the way a front end does, and
// prove the pieces of the ABI this project depends on actually work.
//
// Build:  cmake --build build --target fc_libretro_probe
// Run:    ./build/fc_libretro_probe build/fc_libretro.dylib [game.nes]
//
// Why this exists
// ---------------
// The unit tests call the retro_* functions directly, which proves they do
// something. This does not: it dlopen()s the shared object, resolves every
// symbol by name in a symbol table it did not build, and then drives the core
// through a stand-in front end. That is the difference between "the functions
// compile" and "a front end can load this".
//
// It checks the five things stage L1 promised:
//
//   1. every required symbol is present (a missing one is what a front end
//      reports as "core failed to load", with no clue which symbol)
//   2. the picture arrives, 256x240, in the format the core asked for
//   3. the sound arrives, as stereo frames
//   4. the input path is polled once and queried per frame
//   5. a save state round trips: run, save, run, restore, run -- and the
//      second run reaches the same pixels as the first
//
// The last one is the whole reason rewind exists in a libretro front end: it
// is serialize and unserialize in a loop, and nothing else.
// ---------------------------------------------------------------------------

#include "libretro.h"
#include "fc_libretro_ext.h"

#include <cstdarg>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <dlfcn.h>
#include <fstream>
#include <string>
#include <vector>

namespace {

// ---------------------------------------------------------------------------
// The symbol table
//
// Looked up by the same names a front end looks up. `fatal` collects a missing
// one instead of dereferencing null later.
// ---------------------------------------------------------------------------

struct Core {
    void* handle = nullptr;

    unsigned (*api_version)(void) = nullptr;
    void (*get_system_info)(retro_system_info*) = nullptr;
    void (*get_system_av_info)(retro_system_av_info*) = nullptr;
    void (*set_environment)(retro_environment_t) = nullptr;
    void (*set_video_refresh)(retro_video_refresh_t) = nullptr;
    void (*set_audio_sample)(retro_audio_sample_t) = nullptr;
    void (*set_audio_sample_batch)(retro_audio_sample_batch_t) = nullptr;
    void (*set_input_poll)(retro_input_poll_t) = nullptr;
    void (*set_input_state)(retro_input_state_t) = nullptr;
    void (*init)(void) = nullptr;
    void (*deinit)(void) = nullptr;
    void (*reset)(void) = nullptr;
    void (*run)(void) = nullptr;
    size_t (*serialize_size)(void) = nullptr;
    bool (*serialize)(void*, size_t) = nullptr;
    bool (*unserialize)(const void*, size_t) = nullptr;
    void (*cheat_reset)(void) = nullptr;
    void (*cheat_set)(unsigned, bool, const char*) = nullptr;
    bool (*load_game)(const retro_game_info*) = nullptr;
    bool (*load_game_special)(unsigned, const retro_game_info*, size_t) = nullptr;
    void (*unload_game)(void) = nullptr;
    unsigned (*get_region)(void) = nullptr;
    void* (*get_memory_data)(unsigned) = nullptr;
    size_t (*get_memory_size)(unsigned) = nullptr;
    void (*set_controller_port_device)(unsigned, unsigned) = nullptr;

    // Optional: a standard front end never asks for it.
    const fc_libretro_ext_v1* (*get_ext)(void) = nullptr;
};

int g_failures = 0;

void check(bool condition, const char* what)
{
    std::printf("  %s %s\n", condition ? "ok  " : "FAIL", what);
    if (!condition) {
        ++g_failures;
    }
}

// ---------------------------------------------------------------------------
// The stand-in front end
//
// A libretro front end is mostly a set of callbacks, and this is the smallest
// set that lets the core run. Every one of them records what it saw, because
// the point of the probe is to check that the core *called* them.
// ---------------------------------------------------------------------------

struct Recording {
    bool pixel_format_asked = false;
    enum retro_pixel_format pixel_format = RETRO_PIXEL_FORMAT_0RGB1555;
    bool input_descriptors_set = false;
    bool controller_info_set = false;
    bool memory_map_published = false;
    unsigned memory_map_descriptors = 0;

    int video_calls = 0;
    int video_width = 0;
    int video_height = 0;
    int video_pitch = 0;
    bool video_null = false;
    /// The last picture the core handed over. Stable for the life of the
    /// machine, which is what lets the probe hash it from outside.
    const uint32_t* video_ptr = nullptr;

    int audio_frames = 0;
    int audio_calls = 0;

    int input_polls = 0;
    int input_queries = 0;

    /// Whether to report every button as held, so the input path can be seen
    /// to reach the machine rather than merely being called.
    bool report_buttons = false;
};

Recording g_rec;

/// The front end's logger. A plain function rather than a lambda: Clang will
/// not convert a lambda to a C variadic function pointer.
void probe_log(enum retro_log_level level, const char* fmt, ...)
{
    (void)level;
    va_list args;
    va_start(args, fmt);
    std::printf("  [core] ");
    std::vprintf(fmt, args);
    std::printf("\n");
    va_end(args);
}

bool environment_cb(unsigned cmd, void* data)
{
    switch (cmd) {
    case RETRO_ENVIRONMENT_SET_PIXEL_FORMAT:
        g_rec.pixel_format_asked = true;
        g_rec.pixel_format = *static_cast<const enum retro_pixel_format*>(data);
        return g_rec.pixel_format == RETRO_PIXEL_FORMAT_XRGB8888;

    case RETRO_ENVIRONMENT_GET_LOG_INTERFACE:
        static_cast<retro_log_callback*>(data)->log = probe_log;
        return true;

    case RETRO_ENVIRONMENT_GET_CAN_DUPE:
        *static_cast<bool*>(data) = true;
        return true;

    case RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS:
        g_rec.input_descriptors_set = true;
        return true;

    case RETRO_ENVIRONMENT_SET_CONTROLLER_INFO:
        g_rec.controller_info_set = true;
        return true;

    case RETRO_ENVIRONMENT_SET_MEMORY_MAPS:
        g_rec.memory_map_published = true;
        g_rec.memory_map_descriptors =
            static_cast<const retro_memory_map*>(data)->num_descriptors;
        return true;

    default:
        return false;
    }
}

void video_cb(const void* data, unsigned width, unsigned height, size_t pitch)
{
    ++g_rec.video_calls;
    g_rec.video_width = static_cast<int>(width);
    g_rec.video_height = static_cast<int>(height);
    g_rec.video_pitch = static_cast<int>(pitch);
    g_rec.video_null = data == nullptr;
    g_rec.video_ptr = static_cast<const uint32_t*>(data);
}

size_t audio_batch_cb(const int16_t* data, size_t frames)
{
    ++g_rec.audio_calls;
    g_rec.audio_frames += static_cast<int>(frames);

    // Read a sample so an all-zero buffer is a fact rather than an assumption.
    if (frames > 0) {
        volatile int16_t sample = data[0];
        (void)sample;
    }
    return frames;
}

void audio_sample_cb(int16_t left, int16_t right)
{
    (void)left;
    (void)right;
}

void input_poll_cb(void)
{
    ++g_rec.input_polls;
}

int16_t input_state_cb(unsigned port, unsigned device, unsigned index, unsigned id)
{
    (void)port;
    (void)device;
    (void)index;
    (void)id;
    ++g_rec.input_queries;
    return g_rec.report_buttons ? 1 : 0;
}

// ---------------------------------------------------------------------------
// A fingerprint of the picture. Same idea as the one in packages/fc-core/tests/test_state.cpp:
// cheap, and equal only when the pixels are equal.
// ---------------------------------------------------------------------------

uint64_t framebuffer_hash(const uint32_t* pixels, size_t count)
{
    uint64_t hash = 1469598103934665603ull;
    for (size_t i = 0; i < count; ++i) {
        hash ^= pixels[i];
        hash *= 1099511628211ull;
    }
    return hash;
}

std::vector<uint8_t> read_file(const std::string& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return {};
    }
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(file),
                                std::istreambuf_iterator<char>());
}

} // namespace

int main(int argc, char** argv)
{
    const std::string core_path = (argc > 1) ? argv[1] : "build/fc_libretro.dylib";
    const std::string rom_path =
        (argc > 2) ? argv[2] : "packages/fc-core/tests/data/testroms/nestest.nes";

    std::printf("core : %s\nrom  : %s\n\n", core_path.c_str(), rom_path.c_str());

    // -- 1. load and resolve --------------------------------------------------

    Core core;
    core.handle = dlopen(core_path.c_str(), RTLD_NOW | RTLD_LOCAL);
    if (core.handle == nullptr) {
        std::printf("FAIL dlopen: %s\n", dlerror());
        return 1;
    }
    std::printf("ok   dlopen\n");

    struct Symbol {
        const char* name;
        void** target;
    };
    const Symbol symbols[] = {
        { "retro_api_version", reinterpret_cast<void**>(&core.api_version) },
        { "retro_get_system_info", reinterpret_cast<void**>(&core.get_system_info) },
        { "retro_get_system_av_info", reinterpret_cast<void**>(&core.get_system_av_info) },
        { "retro_set_environment", reinterpret_cast<void**>(&core.set_environment) },
        { "retro_set_video_refresh", reinterpret_cast<void**>(&core.set_video_refresh) },
        { "retro_set_audio_sample", reinterpret_cast<void**>(&core.set_audio_sample) },
        { "retro_set_audio_sample_batch", reinterpret_cast<void**>(&core.set_audio_sample_batch) },
        { "retro_set_input_poll", reinterpret_cast<void**>(&core.set_input_poll) },
        { "retro_set_input_state", reinterpret_cast<void**>(&core.set_input_state) },
        { "retro_init", reinterpret_cast<void**>(&core.init) },
        { "retro_deinit", reinterpret_cast<void**>(&core.deinit) },
        { "retro_reset", reinterpret_cast<void**>(&core.reset) },
        { "retro_run", reinterpret_cast<void**>(&core.run) },
        { "retro_serialize_size", reinterpret_cast<void**>(&core.serialize_size) },
        { "retro_serialize", reinterpret_cast<void**>(&core.serialize) },
        { "retro_unserialize", reinterpret_cast<void**>(&core.unserialize) },
        { "retro_cheat_reset", reinterpret_cast<void**>(&core.cheat_reset) },
        { "retro_cheat_set", reinterpret_cast<void**>(&core.cheat_set) },
        { "retro_load_game", reinterpret_cast<void**>(&core.load_game) },
        { "retro_load_game_special", reinterpret_cast<void**>(&core.load_game_special) },
        { "retro_unload_game", reinterpret_cast<void**>(&core.unload_game) },
        { "retro_get_region", reinterpret_cast<void**>(&core.get_region) },
        { "retro_get_memory_data", reinterpret_cast<void**>(&core.get_memory_data) },
        { "retro_get_memory_size", reinterpret_cast<void**>(&core.get_memory_size) },
        { "retro_set_controller_port_device", reinterpret_cast<void**>(&core.set_controller_port_device) },
    };

    bool missing = false;
    for (const Symbol& symbol : symbols) {
        *symbol.target = dlsym(core.handle, symbol.name);
        if (*symbol.target == nullptr) {
            std::printf("FAIL missing symbol: %s\n", symbol.name);
            missing = true;
        }
    }
    check(!missing, "all 25 required symbols resolved");
    if (missing) {
        dlclose(core.handle);
        return 1;
    }

    core.get_ext = reinterpret_cast<const fc_libretro_ext_v1* (*)(void)>(
        dlsym(core.handle, "fc_libretro_get_ext"));
    check(core.get_ext != nullptr, "the optional custom extension is exported");

    check(core.api_version() == RETRO_API_VERSION, "retro_api_version is 1");

    // -- 2. what the core says it is -----------------------------------------

    retro_system_info info{};
    core.get_system_info(&info);
    std::printf("\nsystem: %s %s [%s] fullpath=%d\n", info.library_name,
                info.library_version, info.valid_extensions, info.need_fullpath);
    check(std::strcmp(info.valid_extensions, "nes") == 0, "valid_extensions is nes");
    check(info.need_fullpath == false, "need_fullpath is false");

    retro_system_av_info av{};
    core.get_system_av_info(&av);
    std::printf("av    : %ux%u @ %.4f fps, %g Hz\n", av.geometry.base_width,
                av.geometry.base_height, av.timing.fps, av.timing.sample_rate);
    check(av.geometry.base_width == 256 && av.geometry.base_height == 240,
          "geometry is 256x240");
    check(av.timing.fps > 59.0 && av.timing.fps < 61.0, "frame rate is near 60");
    check(av.timing.sample_rate == 44100.0, "sample rate is 44100");

    // -- 3. the ROM -----------------------------------------------------------

    const std::vector<uint8_t> rom = read_file(rom_path);
    if (rom.empty()) {
        std::printf("FAIL could not read ROM: %s\n", rom_path.c_str());
        dlclose(core.handle);
        return 1;
    }

    // -- 4. wire up the front end and run ------------------------------------

    core.set_environment(environment_cb);
    core.set_video_refresh(video_cb);
    core.set_audio_sample(audio_sample_cb);
    core.set_audio_sample_batch(audio_batch_cb);
    core.set_input_poll(input_poll_cb);
    core.set_input_state(input_state_cb);

    core.init();

    retro_game_info game{};
    game.path = rom_path.c_str();
    game.data = rom.data();
    game.size = rom.size();
    game.meta = nullptr;

    const bool loaded = core.load_game(&game);
    check(loaded, "retro_load_game accepted the ROM");
    if (!loaded) {
        core.deinit();
        dlclose(core.handle);
        return 1;
    }
    check(g_rec.input_descriptors_set, "input descriptors were published");
    check(g_rec.controller_info_set, "controller info was published");
    check(g_rec.memory_map_published, "a memory map was published for cheat search");
    check(g_rec.memory_map_descriptors >= 1, "the memory map names console RAM");
    check(g_rec.pixel_format_asked, "the core asked for a pixel format");
    check(g_rec.pixel_format == RETRO_PIXEL_FORMAT_XRGB8888,
          "the pixel format asked for is XRGB8888");

    const int warmup = 60;
    for (int i = 0; i < warmup; ++i) {
        core.run();
    }
    std::printf("\nafter %d frames: %d video calls, %d audio calls, %d audio frames, "
                "%d input polls, %d input queries\n",
                warmup, g_rec.video_calls, g_rec.audio_calls, g_rec.audio_frames,
                g_rec.input_polls, g_rec.input_queries);

    check(g_rec.video_calls >= warmup, "the picture was delivered every frame");
    check(g_rec.video_width == 256 && g_rec.video_height == 240,
          "the picture is 256x240");
    check(g_rec.video_pitch == 256 * 4, "the picture pitch is 256 * 4 bytes");
    check(!g_rec.video_null, "the picture pointer is not null");
    check(g_rec.audio_frames > 0, "sound was produced");
    check(g_rec.input_polls >= warmup, "input was polled every frame");
    check(g_rec.input_queries >= warmup * 16,
          "both ports were queried every frame (8 buttons each)");

    // -- 5. the memory views and the custom extension ------------------------

    void* system_ram = core.get_memory_data(RETRO_MEMORY_SYSTEM_RAM);
    const size_t system_ram_size = core.get_memory_size(RETRO_MEMORY_SYSTEM_RAM);
    check(system_ram != nullptr, "console RAM is exposed");
    check(system_ram_size == 0x800, "console RAM is 2KB");

    void* save_ram = core.get_memory_data(RETRO_MEMORY_SAVE_RAM);
    const size_t save_ram_size = core.get_memory_size(RETRO_MEMORY_SAVE_RAM);
    // nestest is an NROM with no battery, so there is nothing to persist. A
    // battery-backed cartridge is what the unit tests cover.
    check(save_ram == nullptr && save_ram_size == 0,
          "save RAM is withheld from a cartridge with no battery");

    if (core.get_ext != nullptr) {
        const fc_libretro_ext_v1* ext = core.get_ext();
        check(ext->abi_version == FC_LIBRETRO_EXT_VERSION,
              "the extension reports its version");
        check(ext->struct_size == sizeof(fc_libretro_ext_v1),
              "the extension reports its own size");

        ext->poke(0x0010, 0x42);
        check(ext->peek(0x0010) == 0x42, "the extension's poke reaches peek");
        check(ext->peek(0x0810) == 0x42, "the extension's peek honours the RAM mask");
        check(ext->total_cycles() > 0, "the extension reports the cycle count");
        check(ext->cpu_pc() >= 0x8000, "the extension reports a program counter in ROM");

        const uint8_t cheat[] = { 0x10, 0x00, 0x77, 0x03 };
        check(ext->set_raw_cheats(cheat, 1) == 1, "a raw cheat can be installed");
        for (int i = 0; i < 2; ++i) {
            core.run();
        }
        check(ext->peek(0x0010) == 0x77, "a frozen cheat is rewritten every frame");
        ext->set_raw_cheats(nullptr, 0);
        check(ext->raw_cheat_count() == 0, "the cheat list can be cleared");

        // Game Genie goes through the standard ABI and lands as a ROM patch.
        // The original byte is whatever this ROM has; only the change and the
        // change back are asserted, so any content file works.
        const int original = ext->peek(0x91D9);
        core.cheat_set(0, true, "SXIOPO");
        check(ext->peek(0x91D9) == 0xAD,
              "a Game Genie code patches the byte the cartridge returns");
        core.cheat_reset();
        check(ext->peek(0x91D9) == original,
              "resetting cheats puts the ROM byte back");
    }

    // -- 6. save state round trip --------------------------------------------

    const size_t state_size = core.serialize_size();
    std::printf("state size: %zu bytes\n", state_size);
    check(state_size > 0, "retro_serialize_size is non-zero after load");

    std::vector<uint8_t> state(state_size);
    check(core.serialize(state.data(), state.size()), "retro_serialize succeeded");

    constexpr size_t kPixels = 256u * 240u;

    // From the saved moment, run forward ten frames and fingerprint the
    // picture. Then put the machine back and do it again. Equal hashes are the
    // property rewind is built on: a libretro front end implements rewind as
    // exactly this loop, with nothing else added.
    for (int i = 0; i < 10; ++i) {
        core.run();
    }
    const uint64_t forward_hash = framebuffer_hash(g_rec.video_ptr, kPixels);

    check(core.unserialize(state.data(), state.size()), "retro_unserialize succeeded");
    for (int i = 0; i < 10; ++i) {
        core.run();
    }
    const uint64_t replay_hash = framebuffer_hash(g_rec.video_ptr, kPixels);

    std::printf("state hash: forward=%016llx replay=%016llx\n",
                static_cast<unsigned long long>(forward_hash),
                static_cast<unsigned long long>(replay_hash));
    check(forward_hash == replay_hash,
          "ten frames after a restore reach the same pixels as the first time");

    core.unload_game();
    core.deinit();
    dlclose(core.handle);

    std::printf("\n%s (%d failure%s)\n", g_failures == 0 ? "PASS" : "FAIL",
                g_failures, g_failures == 1 ? "" : "s");
    return g_failures == 0 ? 0 : 1;
}
