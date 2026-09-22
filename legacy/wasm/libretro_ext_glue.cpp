// ---------------------------------------------------------------------------
// The custom extension, as plain exported functions.
//
// fc_libretro_ext.h hands out a table of function pointers, which is right for
// a native shared object: a front end dlsym()s one symbol and reads the table
// it returns. WebAssembly has no dlsym, and calling a function pointer from
// JavaScript means reaching into the wasm function table, which Emscripten
// keeps to itself.
//
// So for the wasm build the same table is wrapped in eight ordinary exports.
// They call the table and nothing else -- the table is still the one
// definition of the extension, and a field added there appears here only when
// this file is told about it, which is the check that keeps the two in step.
//
// Only the wasm build compiles this. The native core exports the table and
// leaves it at that.
// ---------------------------------------------------------------------------

#include "fc_libretro_ext.h"

#include <cstdint>

extern "C" {

int fc_ext_peek(uint16_t address)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    return ext != nullptr ? ext->peek(address) : 0;
}

void fc_ext_poke(uint16_t address, uint8_t value)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    if (ext != nullptr) {
        ext->poke(address, value);
    }
}

int fc_ext_set_raw_cheats(const uint8_t* data, int count)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    return ext != nullptr ? ext->set_raw_cheats(data, count) : -1;
}

int fc_ext_raw_cheat_count(void)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    return ext != nullptr ? ext->raw_cheat_count() : 0;
}

int fc_ext_mapper_saves_state(void)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    return (ext != nullptr && ext->mapper_saves_state()) ? 1 : 0;
}

const char* fc_ext_rom_summary(void)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    return ext != nullptr ? ext->rom_summary() : "";
}

uint64_t fc_ext_total_cycles(void)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    return ext != nullptr ? ext->total_cycles() : 0;
}

uint16_t fc_ext_cpu_pc(void)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    return ext != nullptr ? ext->cpu_pc() : 0;
}

size_t fc_ext_take_samples(float* out, size_t max)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    return ext != nullptr ? ext->take_samples(out, max) : 0;
}

const char* fc_ext_last_error(void)
{
    const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
    return ext != nullptr ? ext->last_error() : "";
}

} // extern "C"
