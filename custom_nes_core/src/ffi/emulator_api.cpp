#include "ffi/emulator_api.h"

#include "core/nes/cartridge.hpp"
#include "core/nes/machine.hpp"

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <new>
#include <span>
#include <string>
#include <vector>

// ---------------------------------------------------------------------------
// The C API's implementation.
//
// Everything here is a thin translation. If a function in this file grows a
// branch that is not "convert a type" or "handle a null", that branch is in
// the wrong place and belongs in the core, where it can be tested.
// ---------------------------------------------------------------------------

struct fc_machine {
    fc::nes::Machine machine;
    std::string error;
    std::string summary;
    bool halted = false;
};

extern "C" {

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

fc_machine* fc_create(void)
{
    // A machine is small enough that an allocation failure here means the
    // process is already dead, so there is nothing useful to return but NULL.
    return new (std::nothrow) fc_machine();
}

void fc_destroy(fc_machine* machine)
{
    delete machine;
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

bool fc_load_rom(fc_machine* machine, const uint8_t* data, size_t size)
{
    if (machine == nullptr) {
        return false;
    }

    machine->error.clear();
    machine->summary.clear();
    machine->halted = false;

    if (data == nullptr || size == 0) {
        machine->error = "no ROM data";
        return false;
    }

    const std::span<const fc::u8> rom(data, size);
    if (!machine->machine.load_rom(rom, machine->error)) {
        return false;
    }

    const auto* cartridge = machine->machine.cartridge();
    if (cartridge != nullptr) {
        machine->summary = cartridge->summary();
    }
    return true;
}

const char* fc_last_error(const fc_machine* machine)
{
    if (machine == nullptr) {
        return "";
    }
    return machine->error.c_str();
}

const char* fc_rom_summary(const fc_machine* machine)
{
    if (machine == nullptr) {
        return "";
    }
    return machine->summary.c_str();
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

void fc_reset(fc_machine* machine)
{
    if (machine == nullptr) {
        return;
    }
    machine->machine.reset();
    machine->halted = false;
}

bool fc_run_frame(fc_machine* machine)
{
    if (machine == nullptr || machine->halted) {
        return false;
    }

    if (!machine->machine.run_frame()) {
        machine->halted = true;
        return false;
    }
    return true;
}

bool fc_is_halted(const fc_machine* machine)
{
    return (machine != nullptr) && machine->halted;
}

uint32_t fc_frame_count(const fc_machine* machine)
{
    if (machine == nullptr) {
        return 0;
    }
    return static_cast<uint32_t>(machine->machine.ppu().frame_count());
}

// ---------------------------------------------------------------------------
// Video
// ---------------------------------------------------------------------------

const uint32_t* fc_framebuffer(const fc_machine* machine)
{
    if (machine == nullptr) {
        return nullptr;
    }
    return machine->machine.framebuffer().pixels.data();
}

uint32_t fc_pixel(const fc_machine* machine, int x, int y)
{
    if (machine == nullptr) {
        return 0;
    }
    return machine->machine.framebuffer().at(x, y);
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

int fc_sample_rate(void)
{
    return fc::nes::Apu::kSampleRate;
}

size_t fc_take_samples(fc_machine* machine, float* out, size_t max_samples)
{
    if (machine == nullptr) {
        return 0;
    }
    return machine->machine.apu().drain(out, max_samples);
}

size_t fc_samples_pending(const fc_machine* machine)
{
    if (machine == nullptr) {
        return 0;
    }
    return machine->machine.apu().samples_pending();
}

void fc_clear_samples(fc_machine* machine)
{
    if (machine == nullptr) {
        return;
    }
    machine->machine.apu().clear_samples();
}

// ---------------------------------------------------------------------------
// The lock free sample queue
//
// The two indices are the only shared state. The producer publishes its
// writes with a release store to `write`; the consumer publishes the space it
// has freed with a release store to `read`. Each side then loads the other's
// index with acquire, so a sample is never read before it was written and
// storage is never overwritten before the consumer is done with it. Nothing
// here ever waits, which is the whole point: an audio callback that blocks on
// a mutex is an audio callback that misses its deadline.
// ---------------------------------------------------------------------------

struct fc_audio_queue {
    std::vector<float> data;

    // Monotonic 64 bit positions. Wrapping is only a problem after about
    // six million years of 44.1 kHz audio, so the arithmetic stays simple.
    std::atomic<std::uint64_t> write{ 0 };
    std::atomic<std::uint64_t> read{ 0 };
    std::atomic<std::uint64_t> underruns{ 0 };
};

fc_audio_queue* fc_audio_queue_create(uint32_t capacity)
{
    if (capacity == 0) {
        return nullptr;
    }

    auto* queue = new (std::nothrow) fc_audio_queue();
    if (queue == nullptr) {
        return nullptr;
    }
    queue->data.resize(capacity);
    return queue;
}

void fc_audio_queue_destroy(fc_audio_queue* queue)
{
    delete queue;
}

uint32_t fc_audio_queue_push(fc_audio_queue* queue, const float* samples, uint32_t count)
{
    if (queue == nullptr || samples == nullptr || count == 0) {
        return 0;
    }

    const std::uint64_t capacity = queue->data.size();
    const std::uint64_t write = queue->write.load(std::memory_order_relaxed);
    const std::uint64_t read = queue->read.load(std::memory_order_acquire);
    const std::uint64_t space = capacity - (write - read);

    const auto toWrite = static_cast<uint32_t>(std::min<std::uint64_t>(count, space));
    for (uint32_t i = 0; i < toWrite; ++i) {
        queue->data[(write + i) % capacity] = samples[i];
    }

    // Release: everything above is visible before the consumer sees the new
    // write index.
    queue->write.store(write + toWrite, std::memory_order_release);
    return toWrite;
}

uint32_t fc_audio_queue_pop(fc_audio_queue* queue, float* out, uint32_t count)
{
    if (queue == nullptr || out == nullptr || count == 0) {
        return 0;
    }

    const std::uint64_t capacity = queue->data.size();
    const std::uint64_t read = queue->read.load(std::memory_order_relaxed);
    const std::uint64_t write = queue->write.load(std::memory_order_acquire);
    const std::uint64_t available = write - read;

    const auto taken = static_cast<uint32_t>(std::min<std::uint64_t>(count, available));
    for (uint32_t i = 0; i < taken; ++i) {
        out[i] = queue->data[(read + i) % capacity];
    }

    // The callback asked for `count` samples, so the tail must be silence,
    // never whatever happened to be in the buffer. An underrun is a short
    // gap, not a burst of noise.
    for (uint32_t i = taken; i < count; ++i) {
        out[i] = 0.0f;
    }
    if (taken < count) {
        queue->underruns.fetch_add(1, std::memory_order_relaxed);
    }

    queue->read.store(read + taken, std::memory_order_release);
    return taken;
}

uint32_t fc_audio_queue_fill(const fc_audio_queue* queue)
{
    if (queue == nullptr) {
        return 0;
    }
    const std::uint64_t write = queue->write.load(std::memory_order_acquire);
    const std::uint64_t read = queue->read.load(std::memory_order_acquire);
    return static_cast<uint32_t>(write - read);
}

uint64_t fc_audio_queue_underruns(const fc_audio_queue* queue)
{
    if (queue == nullptr) {
        return 0;
    }
    return queue->underruns.load(std::memory_order_relaxed);
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

void fc_set_button(fc_machine* machine, fc_button button, bool pressed, int port)
{
    if (machine == nullptr) {
        return;
    }

    const int index = (port == 1) ? 1 : 0;
    const auto value = static_cast<fc::nes::Controller::Button>(button);
    machine->machine.controller(index).set_button(value, pressed);
}

void fc_release_all_buttons(fc_machine* machine)
{
    if (machine == nullptr) {
        return;
    }
    machine->machine.controller(0).release_all();
    machine->machine.controller(1).release_all();
}

// ---------------------------------------------------------------------------
// Save states
//
// The only place in this file that allocates something the caller has to
// release. It is a plain malloc rather than new[] because the matching
// fc_free_state is the thinnest possible thing, and because a caller in
// another language has no idea what operator new is.
// ---------------------------------------------------------------------------

uint8_t* fc_save_state(const fc_machine* machine, size_t* size)
{
    if (size != nullptr) {
        *size = 0;
    }
    if (machine == nullptr || machine->machine.cartridge() == nullptr) {
        return nullptr;
    }

    std::vector<fc::u8> bytes;
    machine->machine.save_state(bytes);
    if (bytes.empty()) {
        return nullptr;
    }

    auto* buffer = static_cast<uint8_t*>(std::malloc(bytes.size()));
    if (buffer == nullptr) {
        return nullptr;
    }
    std::memcpy(buffer, bytes.data(), bytes.size());

    if (size != nullptr) {
        *size = bytes.size();
    }
    return buffer;
}

void fc_free_state(uint8_t* data)
{
    std::free(data);
}

size_t fc_state_size(const fc_machine* machine)
{
    if (machine == nullptr) {
        return 0;
    }
    return machine->machine.state_size();
}

size_t fc_save_state_into(const fc_machine* machine, uint8_t* out, size_t capacity)
{
    if (machine == nullptr || out == nullptr || capacity == 0) {
        return 0;
    }
    if (machine->machine.cartridge() == nullptr) {
        return 0;
    }

    const std::span<fc::u8> buffer(reinterpret_cast<fc::u8*>(out), capacity);
    return machine->machine.save_state_into(buffer);
}

bool fc_load_state(fc_machine* machine, const uint8_t* data, size_t size)
{
    if (machine == nullptr || data == nullptr || size == 0) {
        return false;
    }

    machine->error.clear();

    const std::span<const fc::u8> bytes(data, size);
    if (!machine->machine.load_state(bytes)) {
        machine->error = "not a save state, or not one for this cartridge";
        return false;
    }

    // The CPU is running again, whatever it was doing before.
    machine->halted = false;
    return true;
}

bool fc_mapper_saves_state(const fc_machine* machine)
{
    if (machine == nullptr) {
        return false;
    }
    return machine->machine.mapper_saves_state();
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

uint64_t fc_total_cycles(const fc_machine* machine)
{
    if (machine == nullptr) {
        return 0;
    }
    return machine->machine.cpu().total_cycles();
}

uint16_t fc_cpu_pc(const fc_machine* machine)
{
    if (machine == nullptr) {
        return 0;
    }
    return machine->machine.cpu().registers().pc;
}

// ---------------------------------------------------------------------------
// Debugging and cheats
// ---------------------------------------------------------------------------

uint8_t fc_peek(fc_machine* machine, uint16_t address)
{
    if (machine == nullptr) {
        return 0;
    }
    return machine->machine.bus().peek(address);
}

void fc_poke(fc_machine* machine, uint16_t address, uint8_t value)
{
    if (machine == nullptr) {
        return;
    }
    machine->machine.bus().write(address, value);
}

void fc_set_cheats(fc_machine* machine, const uint8_t* data, int count)
{
    if (machine == nullptr) {
        return;
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

    machine->machine.cheats().set(cheats);
}

int fc_cheat_count(const fc_machine* machine)
{
    if (machine == nullptr) {
        return 0;
    }
    return static_cast<int>(machine->machine.cheats().size());
}

} // extern "C"
