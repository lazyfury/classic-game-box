// ---------------------------------------------------------------------------
// The libretro ABI.
//
// tests/test_ffi.cpp asks whether the C interface does what the front end
// needs. This asks the same question of the other door into the machine, the
// one RetroArch uses, and it is worth having both because they are allowed to
// fail independently: a change that suits fc_* and quietly breaks retro_* is
// exactly the kind of thing that would only show up on somebody else's
// computer, in somebody else's emulator.
//
// The front end here is a stand-in. Real front ends are large programs whose
// behaviour cannot be asserted; a stand-in can be small enough that every call
// it receives is a fact a test can read. That is the whole design of this
// file: a recording of what the core did, and assertions about the recording.
//
// Read together with src/libretro/fc_libretro.cpp, and with
// tools/fc_libretro_probe.cpp, which does the same thing through dlopen() so
// that "the symbols exist in the shared object" is checked as well.
// ---------------------------------------------------------------------------

#include "libretro.h"
#include "fc_libretro_ext.h"

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <cstdarg>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

namespace {

// ---------------------------------------------------------------------------
// The stand-in front end
//
// A file static pointer rather than a parameter, because libretro callbacks
// are plain C function pointers with the front end's state hidden behind them.
// The tests are single threaded, so one pointer is enough.
// ---------------------------------------------------------------------------

struct FakeFrontend {
    bool pixel_format_asked = false;
    enum retro_pixel_format pixel_format = RETRO_PIXEL_FORMAT_0RGB1555;
    bool input_descriptors_published = false;
    bool controller_info_published = false;
    bool memory_map_published = false;
    unsigned memory_map_descriptors = 0;

    int video_calls = 0;
    unsigned video_width = 0;
    unsigned video_height = 0;
    size_t video_pitch = 0;
    const uint32_t* video_pointer = nullptr;

    int audio_calls = 0;
    size_t audio_frames = 0;
    /// False if any stereo frame had left != right, which would mean the mono
    /// source was being played rather than copied.
    bool audio_is_dual_mono = true;

    int input_polls = 0;
    /// Every (port, id) the core asked about, in order.
    std::vector<std::pair<unsigned, unsigned>> queries;

    /// What to answer. Empty means "nothing is held".
    std::vector<std::pair<unsigned, unsigned>> held;
};

FakeFrontend* g_front = nullptr;

bool environment_cb(unsigned cmd, void* data)
{
    switch (cmd) {
    case RETRO_ENVIRONMENT_SET_PIXEL_FORMAT:
        g_front->pixel_format_asked = true;
        g_front->pixel_format = *static_cast<const enum retro_pixel_format*>(data);
        return g_front->pixel_format == RETRO_PIXEL_FORMAT_XRGB8888;

    case RETRO_ENVIRONMENT_GET_LOG_INTERFACE:
        // No logger: a core that logs when nobody is listening is a core that
        // crashes, and this test would rather find that out.
        return false;

    case RETRO_ENVIRONMENT_GET_CAN_DUPE:
        *static_cast<bool*>(data) = true;
        return true;

    case RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS:
        g_front->input_descriptors_published = true;
        return true;

    case RETRO_ENVIRONMENT_SET_CONTROLLER_INFO:
        g_front->controller_info_published = true;
        return true;

    case RETRO_ENVIRONMENT_SET_MEMORY_MAPS:
        g_front->memory_map_published = true;
        g_front->memory_map_descriptors =
            static_cast<const retro_memory_map*>(data)->num_descriptors;
        return true;

    default:
        return false;
    }
}

void video_cb(const void* data, unsigned width, unsigned height, size_t pitch)
{
    ++g_front->video_calls;
    g_front->video_width = width;
    g_front->video_height = height;
    g_front->video_pitch = pitch;
    g_front->video_pointer = static_cast<const uint32_t*>(data);
}

size_t audio_batch_cb(const int16_t* data, size_t frames)
{
    ++g_front->audio_calls;
    g_front->audio_frames += frames;
    for (size_t i = 0; i < frames; ++i) {
        if (data[i * 2] != data[i * 2 + 1]) {
            g_front->audio_is_dual_mono = false;
        }
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
    ++g_front->input_polls;
}

int16_t input_state_cb(unsigned port, unsigned device, unsigned index, unsigned id)
{
    (void)device;
    (void)index;
    g_front->queries.emplace_back(port, id);
    for (const auto& held : g_front->held) {
        if (held.first == port && held.second == id) {
            return 1;
        }
    }
    return 0;
}

// ---------------------------------------------------------------------------
// A synthetic cartridge
//
// The same NROM program tests/test_state.cpp uses: it turns on a pulse channel
// so the APU has something to say, increments a byte of RAM, and loops. A real
// ROM would drag a file and a mapper into a test about the ABI.
// ---------------------------------------------------------------------------

std::vector<uint8_t> make_rom(bool battery = false)
{
    std::vector<uint8_t> rom = { 'N', 'E', 'S', 0x1A, 2, 1, 0, 0 };
    // flags 6 bit 1 is the battery. A save RAM test needs it set, because
    // without it the cartridge is not promising the RAM outlives power.
    if (battery) {
        rom[6] |= 0x02;
    }
    rom.insert(rom.end(), 8, 0);

    std::vector<uint8_t> prg(2 * 16384, 0xEA);
    const std::vector<uint8_t> program = {
        0xA9, 0xBF,             // LDA #$BF
        0x8D, 0x00, 0x40,       // STA $4000
        0xE6, 0x00,             // INC $00
        0x4C, 0x00, 0x80,       // JMP $8000
    };
    std::copy(program.begin(), program.end(), prg.begin());

    const std::size_t vectors = prg.size() - 6;
    for (int i = 0; i < 3; ++i) {
        prg[vectors + static_cast<std::size_t>(i) * 2] = 0x00;
        prg[vectors + static_cast<std::size_t>(i) * 2 + 1] = 0x80;
    }

    rom.insert(rom.end(), prg.begin(), prg.end());
    rom.resize(rom.size() + 8192, 0);
    return rom;
}

uint64_t framebuffer_hash(const uint32_t* pixels, size_t count)
{
    uint64_t hash = 1469598103934665603ull;
    for (size_t i = 0; i < count; ++i) {
        hash ^= pixels[i];
        hash *= 1099511628211ull;
    }
    return hash;
}

// ---------------------------------------------------------------------------
// The fixture
//
// SetUp wires a fresh front end and initialises the core; TearDown takes it
// apart. Every test therefore starts at the same place even though libretro's
// state is a process-wide singleton.
// ---------------------------------------------------------------------------

class LibretroTest : public ::testing::Test {
protected:
    void SetUp() override
    {
        front_ = FakeFrontend{};
        g_front = &front_;

        retro_set_environment(environment_cb);
        retro_set_video_refresh(video_cb);
        retro_set_audio_sample(audio_sample_cb);
        retro_set_audio_sample_batch(audio_batch_cb);
        retro_set_input_poll(input_poll_cb);
        retro_set_input_state(input_state_cb);
        retro_init();
    }

    void TearDown() override
    {
        retro_deinit();
        g_front = nullptr;
    }

    /// Put the synthetic cartridge in and hand back the retro_game_info that
    /// keeps pointing at it. The ROM outlives the call; libretro says the data
    /// is only valid until retro_load_game returns, and this core copies what
    /// it needs, so that is honoured.
    retro_game_info load(bool battery = false)
    {
        rom_ = make_rom(battery);
        retro_game_info game{};
        game.path = "synthetic.nes";
        game.data = rom_.data();
        game.size = rom_.size();
        game.meta = nullptr;
        EXPECT_TRUE(retro_load_game(&game));
        return game;
    }

    void run(int frames)
    {
        for (int i = 0; i < frames; ++i) {
            retro_run();
        }
    }

    FakeFrontend front_;
    std::vector<uint8_t> rom_;
};

constexpr size_t kPixels = 256u * 240u;

// ---------------------------------------------------------------------------
// What the core says it is
// ---------------------------------------------------------------------------

TEST_F(LibretroTest, ApiVersionIsTheOneTheHeaderDeclares)
{
    EXPECT_EQ(retro_api_version(), RETRO_API_VERSION);
}

TEST_F(LibretroTest, SystemInfoDescribesAnInMemoryNes)
{
    retro_system_info info{};
    retro_get_system_info(&info);

    EXPECT_STREQ(info.library_name, "Classic Game Box");
    EXPECT_STREQ(info.valid_extensions, "nes");
    // need_fullpath false is what lets a front end hand over bytes rather than
    // a file, which is what makes patching and archives the front end's job.
    EXPECT_FALSE(info.need_fullpath);
    EXPECT_FALSE(info.block_extract);
}

TEST_F(LibretroTest, AvInfoIsTheNtscNes)
{
    retro_system_av_info av{};
    retro_get_system_av_info(&av);

    EXPECT_EQ(av.geometry.base_width, 256u);
    EXPECT_EQ(av.geometry.base_height, 240u);
    EXPECT_EQ(av.geometry.max_width, 256u);
    EXPECT_EQ(av.geometry.max_height, 240u);
    // The exact NTSC rate, not 60: the difference is a second every ten
    // minutes, and a front end paces itself from this number.
    EXPECT_NEAR(av.timing.fps, 60.0988, 0.0001);
    EXPECT_DOUBLE_EQ(av.timing.sample_rate, 44100.0);
}

TEST_F(LibretroTest, RegionIsNtsc)
{
    EXPECT_EQ(retro_get_region(), RETRO_REGION_NTSC);
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

TEST_F(LibretroTest, LoadRejectsAMissingGame)
{
    EXPECT_FALSE(retro_load_game(nullptr));
}

TEST_F(LibretroTest, LoadRejectsAnEmptyBuffer)
{
    retro_game_info game{};
    game.path = "empty.nes";
    game.data = "";
    game.size = 0;
    EXPECT_FALSE(retro_load_game(&game));
}

TEST_F(LibretroTest, LoadAsksForXrgb8888AndPublishesTheInput)
{
    load();

    EXPECT_TRUE(front_.pixel_format_asked);
    EXPECT_EQ(front_.pixel_format, RETRO_PIXEL_FORMAT_XRGB8888);
    EXPECT_TRUE(front_.input_descriptors_published);
    EXPECT_TRUE(front_.controller_info_published);
}

TEST_F(LibretroTest, LoadPublishesAMemoryMapForCheatSearch)
{
    // Without a battery the only searchable memory is the console's 2KB.
    load(false);
    EXPECT_TRUE(front_.memory_map_published);
    EXPECT_EQ(front_.memory_map_descriptors, 1u);

    // With one, the save RAM is searchable too.
    retro_unload_game();
    load(true);
    EXPECT_EQ(front_.memory_map_descriptors, 2u);
}

TEST_F(LibretroTest, LoadGameSpecialIsRefusedBecauseThereAreNoSubsystems)
{
    EXPECT_FALSE(retro_load_game_special(0, nullptr, 0));
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

TEST_F(LibretroTest, EveryRunDeliversOnePictureAndSomeSound)
{
    load();
    run(5);

    EXPECT_EQ(front_.video_calls, 5);
    EXPECT_EQ(front_.video_width, 256u);
    EXPECT_EQ(front_.video_height, 240u);
    // Four bytes a pixel, so a scanline is 1024 bytes and the front end's row
    // stepping is right.
    EXPECT_EQ(front_.video_pitch, 256u * 4u);
    ASSERT_NE(front_.video_pointer, nullptr);

    EXPECT_GT(front_.audio_calls, 0);
    EXPECT_GT(front_.audio_frames, 0u);
    // The APU is mono; both channels carry the same sample.
    EXPECT_TRUE(front_.audio_is_dual_mono);
}

TEST_F(LibretroTest, TheAudioCountMatchesTheFrameRate)
{
    load();
    const int frames = 60;
    run(frames);

    // 44100 / 60.0988 = 733.8 samples a frame, so sixty frames is around
    // 44000 -- and being off by a factor of two is the mistake this catches.
    const double per_frame =
        static_cast<double>(front_.audio_frames) / static_cast<double>(frames);
    EXPECT_GT(per_frame, 700.0);
    EXPECT_LT(per_frame, 760.0);
}

TEST_F(LibretroTest, InputIsPolledOnceAndEachPortIsQueriedPerFrame)
{
    load();
    run(3);

    EXPECT_EQ(front_.input_polls, 3);
    // Two ports, eight buttons each, three frames.
    EXPECT_EQ(front_.queries.size(), 3u * 2u * 8u);
}

TEST_F(LibretroTest, EveryButtonIsQueriedAndOnlyOncePerPortPerFrame)
{
    load();
    run(1);

    const unsigned expected[] = {
        RETRO_DEVICE_ID_JOYPAD_B,      RETRO_DEVICE_ID_JOYPAD_A,
        RETRO_DEVICE_ID_JOYPAD_SELECT, RETRO_DEVICE_ID_JOYPAD_START,
        RETRO_DEVICE_ID_JOYPAD_UP,     RETRO_DEVICE_ID_JOYPAD_DOWN,
        RETRO_DEVICE_ID_JOYPAD_LEFT,   RETRO_DEVICE_ID_JOYPAD_RIGHT,
    };

    for (unsigned port = 0; port < 2; ++port) {
        for (unsigned id : expected) {
            const auto count = std::count(front_.queries.begin(),
                                          front_.queries.end(),
                                          std::make_pair(port, id));
            EXPECT_EQ(count, 1) << "port " << port << " id " << id;
        }
    }
}

TEST_F(LibretroTest, ADisconnectedPortIsNotQueried)
{
    load();
    retro_set_controller_port_device(0, RETRO_DEVICE_NONE);
    run(1);

    for (const auto& query : front_.queries) {
        EXPECT_NE(query.first, 0u) << "a port with nothing plugged in was queried";
    }
    EXPECT_FALSE(front_.queries.empty()) << "port 1 should still be queried";
}

TEST_F(LibretroTest, AButtonThatIsHeldIsSeenByTheCore)
{
    // The core is the black box here, so what this can assert is the direction
    // of the wire: the front end's answer reaches the input state function
    // that the core called, per button, every frame.
    load();
    for (unsigned id = 0; id < 8; ++id) {
        front_.held.emplace_back(0u, id);
    }
    run(2);
    EXPECT_EQ(front_.input_polls, 2);
}

// ---------------------------------------------------------------------------
// Reset
// ---------------------------------------------------------------------------

TEST_F(LibretroTest, ResetStartsTheMachineOver)
{
    load();
    run(30);
    retro_reset();
    // A frame after a reset must still produce a picture; if reset had broken
    // the machine this is where it would stop.
    const int before = front_.video_calls;
    run(1);
    EXPECT_EQ(front_.video_calls, before + 1);
}

// ---------------------------------------------------------------------------
// Save states
// ---------------------------------------------------------------------------

TEST_F(LibretroTest, SerializeSizeIsOnlyMeaningfulWithACartridge)
{
    EXPECT_EQ(retro_serialize_size(), 0u);
    load();
    EXPECT_GT(retro_serialize_size(), 0u);
}

TEST_F(LibretroTest, SerializeRefusesABufferThatIsTooSmall)
{
    load();
    uint8_t byte = 0;
    EXPECT_FALSE(retro_serialize(&byte, 1));
}

TEST_F(LibretroTest, UnserializeRefusesRubbish)
{
    load();
    const uint8_t rubbish[16] = {};
    EXPECT_FALSE(retro_unserialize(rubbish, sizeof(rubbish)));
}

TEST_F(LibretroTest, ARestoreReplaysToTheSamePixels)
{
    load();
    run(40);

    const size_t size = retro_serialize_size();
    std::vector<uint8_t> state(size);
    ASSERT_TRUE(retro_serialize(state.data(), state.size()));

    // Forward from the saved moment, then back and forward again. This is
    // precisely what a front end does for rewind, and equal pictures are the
    // only property that makes rewind usable.
    run(10);
    const uint64_t forward = framebuffer_hash(front_.video_pointer, kPixels);

    ASSERT_TRUE(retro_unserialize(state.data(), state.size()));
    run(10);
    const uint64_t replay = framebuffer_hash(front_.video_pointer, kPixels);

    EXPECT_EQ(forward, replay);
}

TEST_F(LibretroTest, UnserializeClearsTheHaltedFlag)
{
    // Not reachable with a well-behaved ROM, but the contract is that restore
    // gives back a running machine, and that is cheap to state.
    load();
    run(1);
    const size_t size = retro_serialize_size();
    std::vector<uint8_t> state(size);
    ASSERT_TRUE(retro_serialize(state.data(), state.size()));
    EXPECT_TRUE(retro_unserialize(state.data(), state.size()));
    run(1);
    EXPECT_GT(front_.video_calls, 1);
}

// ---------------------------------------------------------------------------
// Stubs that are deliberate
// ---------------------------------------------------------------------------

TEST_F(LibretroTest, ConsoleRamIsExposedForCheatSearch)
{
    load();

    void* ram = retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM);
    ASSERT_NE(ram, nullptr);
    // 2KB, and the same bytes the CPU sees at $0000.
    EXPECT_EQ(retro_get_memory_size(RETRO_MEMORY_SYSTEM_RAM), 0x800u);

    // Writing through the view is visible on the CPU's address bus, which is
    // what makes it usable for a cheat search.
    static_cast<uint8_t*>(ram)[0] = 0x5A;
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    ASSERT_NE(ext, nullptr);
    EXPECT_EQ(ext->peek(0x0000), 0x5A);
    // And the mirrors are the same byte, because the mask is the hardware.
    EXPECT_EQ(ext->peek(0x0800), 0x5A);
}

TEST_F(LibretroTest, SaveRamIsOnlyOfferedWhenTheBatteryBitIsSet)
{
    load(false);
    EXPECT_EQ(retro_get_memory_data(RETRO_MEMORY_SAVE_RAM), nullptr);
    EXPECT_EQ(retro_get_memory_size(RETRO_MEMORY_SAVE_RAM), 0u);

    // A second load with the bit set, on a fresh machine.
    retro_unload_game();
    load(true);

    void* save = retro_get_memory_data(RETRO_MEMORY_SAVE_RAM);
    ASSERT_NE(save, nullptr);
    EXPECT_EQ(retro_get_memory_size(RETRO_MEMORY_SAVE_RAM), 0x2000u);
}

TEST_F(LibretroTest, SaveRamIsTheSameMemoryTheCartridgeAnswersAt6000)
{
    load(true);
    auto* save = static_cast<uint8_t*>(retro_get_memory_data(RETRO_MEMORY_SAVE_RAM));
    ASSERT_NE(save, nullptr);

    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    ASSERT_NE(ext, nullptr);

    // Writing through the bus lands in the buffer a front end would persist;
    // writing the buffer is visible to the CPU. Both directions, because a
    // save file written from the wrong copy is silently empty.
    ext->poke(0x6000, 0x5A);
    EXPECT_EQ(save[0], 0x5A);
    save[1] = 0xA5;
    EXPECT_EQ(ext->peek(0x6001), 0xA5);
}

// ---------------------------------------------------------------------------
// The custom extension
// ---------------------------------------------------------------------------

TEST_F(LibretroTest, TheCustomExtensionIsPresentAndVersioned)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    ASSERT_NE(ext, nullptr);
    EXPECT_EQ(ext->abi_version, FC_LIBRETRO_EXT_VERSION);
    EXPECT_EQ(ext->struct_size, sizeof(fc_libretro_ext_v1));
}

TEST_F(LibretroTest, TheExtensionCarriesTheApuSamplesAtFullPrecision)
{
    load();
    run(2);

    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    ASSERT_NE(ext, nullptr);

    // A frame is about 734 mono samples. The point is that they arrive as the
    // APU's own floats rather than the int16 libretro's callback carries, so a
    // front end that hashes samples byte for byte still matches.
    float samples[4096] = {};
    const std::size_t count = ext->take_samples(samples, 4096);
    EXPECT_GT(count, 0u);
    EXPECT_LT(count, 4096u);

    // Drained, like the libretro audio queue: asking twice gets nothing.
    EXPECT_EQ(ext->take_samples(samples, 4096), 0u);
    run(1);
    EXPECT_GT(ext->take_samples(samples, 4096), 0u);
}

TEST_F(LibretroTest, TheExtensionIsUsableWithoutACartridge)
{
    // Every field has to survive being asked before a game is loaded, because
    // a front end's status bar does exactly that.
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    ASSERT_NE(ext, nullptr);
    EXPECT_EQ(ext->peek(0x0000), 0);
    EXPECT_EQ(ext->raw_cheat_count(), 0);
    EXPECT_FALSE(ext->mapper_saves_state());
    EXPECT_STREQ(ext->rom_summary(), "");
    EXPECT_EQ(ext->total_cycles(), 0u);
    EXPECT_EQ(ext->cpu_pc(), 0u);
    ext->poke(0x0000, 1);
    EXPECT_EQ(ext->set_raw_cheats(nullptr, 0), -1);
    float samples[16] = {};
    EXPECT_EQ(ext->take_samples(samples, 16), 0u);
}

TEST_F(LibretroTest, PeekAndPokeGoThroughTheBus)
{
    load();
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();

    ext->poke(0x0010, 0x42);
    EXPECT_EQ(ext->peek(0x0010), 0x42);
    EXPECT_EQ(ext->peek(0x0810), 0x42);
}

TEST_F(LibretroTest, RawCheatsFreezeAValueEveryFrame)
{
    load();
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();

    // 0x0010 is a byte the synthetic program never touches, so anything found
    // there was put there by the cheat and not by the ROM.
    const uint8_t cheat[] = { 0x10, 0x00, 0x77, 0x03 };  // freeze + enabled
    EXPECT_EQ(ext->set_raw_cheats(cheat, 1), 1);
    EXPECT_EQ(ext->raw_cheat_count(), 1);

    run(2);
    EXPECT_EQ(ext->peek(0x0010), 0x77);

    // The whole list replaces the old one, and an empty one is how it is
    // cleared -- there is no remove verb for the same reason there is no
    // half-updated cheat list.
    EXPECT_EQ(ext->set_raw_cheats(nullptr, 0), 0);
    EXPECT_EQ(ext->raw_cheat_count(), 0);
}

TEST_F(LibretroTest, DiagnosticsDescribeTheMachine)
{
    load();
    run(10);
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();

    EXPECT_GT(ext->total_cycles(), 0u);
    // The program runs out of PRG ROM, so the PC is up in cartridge space.
    EXPECT_GE(ext->cpu_pc(), 0x8000u);
    // NROM has no bank registers, but it does own its CHR RAM, so its board
    // is one that saves. The field is the mapper's answer, not a guess.
    EXPECT_TRUE(ext->mapper_saves_state());
    EXPECT_NE(std::string(ext->rom_summary()).find("mapper 0"), std::string::npos);
}

TEST_F(LibretroTest, ANonCodeIsRejectedQuietly)
{
    load();
    // A string that is not a code must not crash and must not claim success.
    retro_cheat_reset();
    retro_cheat_set(0, true, "NOT A CODE");
    SUCCEED();
}

TEST_F(LibretroTest, A_GameGenieCodePatchesRomReads)
{
    load();
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    ASSERT_NE(ext, nullptr);

    // $91D9 is a NOP in the synthetic program. SXIOPO decodes to $91D9 with
    // value $AD, so the byte the CPU would read changes only after the code
    // is installed -- and changes back when it is reset.
    EXPECT_EQ(ext->peek(0x91D9), 0xEA);

    retro_cheat_set(0, true, "SXIOPO");
    EXPECT_EQ(ext->peek(0x91D9), 0xAD);

    // Switching the one code off must take the patch back out, not merely
    // remember that it is off.
    retro_cheat_set(0, false, "SXIOPO");
    EXPECT_EQ(ext->peek(0x91D9), 0xEA);

    retro_cheat_set(0, true, "SXIOPO");
    EXPECT_EQ(ext->peek(0x91D9), 0xAD);
    retro_cheat_reset();
    EXPECT_EQ(ext->peek(0x91D9), 0xEA);
}

TEST_F(LibretroTest, A_ProActionReplayCodeFreezesRam)
{
    load();
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    ASSERT_NE(ext, nullptr);

    // PAR 000010A1 is address $0010, value $A1, and $0010 is a byte the
    // synthetic program never touches. The freeze runs at the start of every
    // frame, so it survives the program incrementing its own counter.
    retro_cheat_set(0, true, "000010A1");
    run(3);
    EXPECT_EQ(ext->peek(0x0010), 0xA1);

    retro_cheat_reset();
    // The machine writes a fresh value; this only proves the cheat stopped
    // rewriting it, so clear it and let a frame pass.
    ext->poke(0x0010, 0x00);
    run(1);
    EXPECT_EQ(ext->peek(0x0010), 0x00);
}

} // namespace
