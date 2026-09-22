#pragma once

// ---------------------------------------------------------------------------
// Fixed width integer types used across the whole emulator.
//
// Why not just use `int`?
//
//   In C++ the width of `int` is *implementation defined*. On this Mac it is
//   32 bit, but the language only guarantees "at least 16 bit". A NES CPU is
//   an 8 bit machine and its address bus is 16 bit, so every register and
//   every bus line has an exact width. If we let the compiler pick, a bug is
//   only a matter of time.
//
//   <cstdint> gives us exact widths: uint8_t is *exactly* 8 bits, always.
// ---------------------------------------------------------------------------

#include <cstdint>

namespace fc {

using u8  = std::uint8_t;   // 1 byte  : a NES register, a memory cell
using u16 = std::uint16_t;  // 2 bytes : a NES address (0x0000 - 0xFFFF)
using u32 = std::uint32_t;
using u64 = std::uint64_t;

using f32 = float;
using f64 = double;

using s8  = std::int8_t;    // signed view of one byte (-128 .. 127)
using s16 = std::int16_t;   // signed view of two bytes
using s32 = std::int32_t;
using s64 = std::int64_t;

// These asserts turn "I hope the widths are right" into "the compiler proves
// it". If the code is ever built on a platform where they are wrong, the
// build fails immediately instead of misbehaving at runtime.
static_assert(sizeof(u8)  == 1, "u8 must be exactly 1 byte");
static_assert(sizeof(u16) == 2, "u16 must be exactly 2 bytes");
static_assert(sizeof(u32) == 4, "u32 must be exactly 4 bytes");
static_assert(sizeof(u64) == 8, "u64 must be exactly 8 bytes");
static_assert(sizeof(s8)  == 1, "s8 must be exactly 1 byte");

} // namespace fc
