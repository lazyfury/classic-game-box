#pragma once

// ---------------------------------------------------------------------------
// ALU - the part of the CPU that actually computes.
//
// The 6502 has no subtractor, no multiplier, no divider. It has:
//   * an 8 bit adder (with a carry input)
//   * some logic gates
//   * a shifter
//
// This header models the adder, because the adder is where the two most
// misunderstood flags come from: C (carry) and V (overflow).
//
//   C  = "the result does not fit in 8 bits if you read it as UNSIGNED"
//        0..255
//
//   V  = "the result does not fit in 8 bits if you read it as SIGNED"
//        -128..127
//
// These are different questions and they have different answers. That is the
// single most common source of emulator bugs.
// ---------------------------------------------------------------------------

#include "core/types.hpp"

namespace fc::alu {

/// Everything one addition produces.
struct AddResult {
    u8   result;     // the 8 bits that go into the accumulator
    bool carry;      // C : unsigned overflow  (bit 7 carried out)
    bool overflow;   // V : signed overflow    (two's complement)
    bool zero;       // Z : result == 0
    bool negative;   // N : bit 7 of the result (NOT the same thing as V)
};

// ---------------------------------------------------------------------------
// Implementation 1: the "programmer's rule"
//
//   V = operands share a sign, but the result does not.
//
// Rationale: a positive plus a negative can never leave the representable
// range (the result lies between the two operands). Only same-sign addition
// can overflow.
// ---------------------------------------------------------------------------

[[nodiscard]] constexpr bool signed_overflow_rule(u8 a, u8 b, u8 result) noexcept
{
    constexpr u8 sign_mask = 0x80;
    // (a and result differ) AND (b and result differ)  ->  a and b agree
    return ((a ^ result) & (b ^ result) & sign_mask) != 0;
}

// ---------------------------------------------------------------------------
// Implementation 2: the "hardware rule"
//
//   V = carry_into_bit7 XOR carry_out_of_bit7
//
// Rationale: the sign bit is just another full adder. If a carry arrives but
// none leaves (or vice versa), the sign bit was corrupted.
//
// This walks the 8 full adders one by one, exactly like the silicon does.
// Exposing the two carries is useful for the future debugger and for teaching.
// ---------------------------------------------------------------------------

struct AdderTrace {
    u8   result;
    bool carry_into_bit7;   // the carry produced by bit 6
    bool carry_out_of_bit7; // this is also C
};

[[nodiscard]] constexpr AdderTrace trace_add(u8 a, u8 b, bool carry_in = false) noexcept
{
    bool carry = carry_in;
    u8 result = 0;
    bool carry_into_bit7 = false;
    bool carry_out_of_bit7 = false;

    for (int i = 0; i < 8; ++i) {
        const bool x = ((a >> i) & u8{1}) != 0;
        const bool y = ((b >> i) & u8{1}) != 0;

        const bool sum = x ^ y ^ carry;
        const bool carry_out = (x && y) || (x && carry) || (y && carry);

        if (sum) {
            result = static_cast<u8>(result | static_cast<u8>(u8{1} << i));
        }
        if (i == 6) {
            carry_into_bit7 = carry_out;
        }
        if (i == 7) {
            carry_out_of_bit7 = carry_out;
        }
        carry = carry_out;
    }

    return AdderTrace{ result, carry_into_bit7, carry_out_of_bit7 };
}

[[nodiscard]] constexpr bool signed_overflow_from_carries(u8 a, u8 b,
                                                          bool carry_in = false) noexcept
{
    const AdderTrace t = trace_add(a, b, carry_in);
    return t.carry_into_bit7 != t.carry_out_of_bit7;
}

// ---------------------------------------------------------------------------
// The addition itself.
//
// We compute in 32 bit so the 9th bit survives long enough to be observed,
// then truncate to 8 bit. On real hardware that 9th bit is the carry line.
// ---------------------------------------------------------------------------

[[nodiscard]] constexpr AddResult add(u8 a, u8 b, bool carry_in = false) noexcept
{
    const u32 wide = static_cast<u32>(a) + static_cast<u32>(b) + (carry_in ? 1u : 0u);
    const u8 result = static_cast<u8>(wide & 0xFFu);

    AddResult out{};
    out.result   = result;
    out.carry    = wide > 0xFFu;                       // unsigned view
    out.overflow = signed_overflow_rule(a, b, result);  // signed view
    out.zero     = result == 0;
    out.negative = (result & 0x80) != 0;
    return out;
}

/// 6502 SBC: A - M - (1 - C) == A + ~M + C.
/// Note how "carry" doubles as "no borrow" - another consequence of two's
/// complement letting one adder do both jobs.
[[nodiscard]] constexpr AddResult subtract(u8 a, u8 b, bool borrow = false) noexcept
{
    return add(a, static_cast<u8>(~b), !borrow);
}

} // namespace fc::alu
