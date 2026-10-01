#pragma once

// ---------------------------------------------------------------------------
// Bit manipulation helpers.
//
// Everything a CPU does is bit manipulation:
//
//   * an opcode is a byte whose bits select the instruction
//   * an addressing mode is encoded in those same bits
//   * the status register P is 8 independent 1-bit flags
//   * an address is two bytes glued together: lo | (hi << 8)
//
// Getting comfortable with this header is a prerequisite for the CPU.
// ---------------------------------------------------------------------------

#include "core/types.hpp"

#include <concepts>
#include <string>

namespace fc::bit {

// ---------------------------------------------------------------------------
// Single bit access
//
// Bits are numbered from 0 (least significant, rightmost) to 7 (most
// significant, leftmost). This matches how hardware datasheets number them.
//
//     bit:   7 6 5 4 3 2 1 0
//     value: 0 0 0 0 0 0 0 0
//             ^             ^
//             MSB           LSB
// ---------------------------------------------------------------------------

/// Read one bit. Returns true when the bit is 1.
[[nodiscard]] constexpr bool test(u8 value, int index) noexcept
{
    return ((value >> index) & u8{1}) != 0;
}

/// Force one bit to 1, leaving the others untouched.
[[nodiscard]] constexpr u8 set(u8 value, int index) noexcept
{
    return static_cast<u8>(value | (u8{1} << index));
}

/// Force one bit to 0, leaving the others untouched.
[[nodiscard]] constexpr u8 clear(u8 value, int index) noexcept
{
    return static_cast<u8>(value & ~(u8{1} << index));
}

/// Flip one bit.
[[nodiscard]] constexpr u8 toggle(u8 value, int index) noexcept
{
    return static_cast<u8>(value ^ (u8{1} << index));
}

/// Replace one bit with `on`.
[[nodiscard]] constexpr u8 assign(u8 value, int index, bool on) noexcept
{
    return on ? set(value, index) : clear(value, index);
}

// ---------------------------------------------------------------------------
// Multi-bit fields
// ---------------------------------------------------------------------------

/// Low 4 bits  (bits 3..0). A "nibble" is half a byte, i.e. one hex digit.
[[nodiscard]] constexpr u8 low_nibble(u8 value) noexcept
{
    return static_cast<u8>(value & 0x0F);
}

/// High 4 bits (bits 7..4). Note: NOT shifted down.
[[nodiscard]] constexpr u8 high_nibble(u8 value) noexcept
{
    return static_cast<u8>(value & 0xF0);
}

/// Extract an inclusive bit range [hi..lo], right aligned.
[[nodiscard]] constexpr u8 extract(u8 value, int hi, int lo) noexcept
{
    const int width = hi - lo + 1;
    const u8 mask = static_cast<u8>((u8{1} << width) - 1u);
    return static_cast<u8>((value >> lo) & mask);
}

/// Number of bits that are 1.
[[nodiscard]] constexpr int count_set_bits(u8 value) noexcept
{
    int count = 0;
    for (int i = 0; i < 8; ++i) {
        count += test(value, i) ? 1 : 0;
    }
    return count;
}

/// Parity: true when the number of 1 bits is even.
/// (The 6502 keeps this in the P register as the "overflow of the low byte"
/// convention; NES games rely on it after every arithmetic instruction.)
[[nodiscard]] constexpr bool even_parity(u8 value) noexcept
{
    return (count_set_bits(value) % 2) == 0;
}

// ---------------------------------------------------------------------------
// Two's complement
//
// The CPU has no subtraction circuit. `x - y` is implemented as
// `x + (~y + 1)`: adding the two's complement. Everything below is a direct
// model of that hardware trick.
// ---------------------------------------------------------------------------

/// Arithmetic negation in two's complement: -x == ~x + 1.
[[nodiscard]] constexpr u8 negate(u8 value) noexcept
{
    return static_cast<u8>(~value + u8{1});
}

/// Reinterpret the same 8 bits as a signed number.
[[nodiscard]] constexpr s8 as_signed(u8 value) noexcept
{
    return static_cast<s8>(value);
}

/// Reinterpret a signed number back as raw bits.
[[nodiscard]] constexpr u8 as_unsigned(s8 value) noexcept
{
    return static_cast<u8>(value);
}

// ---------------------------------------------------------------------------
// Multi-byte helpers (little endian, like the 6502)
//
// The 6502 stores the low byte first. Address 0x1234 lives in memory as
//     0x1234 -> 0x34
//     0x1235 -> 0x12
// ---------------------------------------------------------------------------

/// Glue a low byte and a high byte into a 16 bit value: hi:lo
[[nodiscard]] constexpr u16 make_u16(u8 lo, u8 hi) noexcept
{
    return static_cast<u16>(static_cast<u16>(hi) << 8 | lo);
}

[[nodiscard]] constexpr u8 lo_byte(u16 value) noexcept
{
    return static_cast<u8>(value & 0x00FF);
}

[[nodiscard]] constexpr u8 hi_byte(u16 value) noexcept
{
    return static_cast<u8>(value >> 8);
}

// ---------------------------------------------------------------------------
// Textual representation (for humans, tests and the future debugger)
// ---------------------------------------------------------------------------

/// Render as binary, e.g. to_binary(u8{0x42}) == "0100 0010".
/// The width may not exceed the type's bit count.
template <std::unsigned_integral T>
[[nodiscard]] std::string to_binary(T value,
                                    int width = static_cast<int>(sizeof(T) * 8),
                                    bool group = true)
{
    const int max_width = static_cast<int>(sizeof(T) * 8);
    if (width > max_width) {
        width = max_width;
    }

    std::string out;
    out.reserve(static_cast<std::size_t>(width + width / 4));

    for (int i = width - 1; i >= 0; --i) {
        const bool bit_on = ((value >> i) & T{1}) != 0;
        out.push_back(bit_on ? '1' : '0');
        if (group && i != 0 && i % 4 == 0) {
            out.push_back(' ');
        }
    }
    return out;
}

/// Render as hex, e.g. to_hex(u8{0x42}) == "0x42", to_hex(u16{0x1234}) == "0x1234".
[[nodiscard]] std::string to_hex(u8 value);
[[nodiscard]] std::string to_hex(u16 value);

} // namespace fc::bit
