#include "core/bit.hpp"

#include <array>
#include <cstddef>

namespace fc::bit {
namespace {

constexpr char kHexDigits[] = "0123456789ABCDEF";

char hex_digit(int nibble)
{
    return kHexDigits[nibble & 0x0F];
}

} // namespace

std::string to_hex(u8 value)
{
    std::string out = "0x";
    out.push_back(hex_digit(value >> 4));
    out.push_back(hex_digit(value));
    return out;
}

std::string to_hex(u16 value)
{
    std::string out = "0x";
    for (int shift = 12; shift >= 0; shift -= 4) {
        out.push_back(hex_digit(value >> shift));
    }
    return out;
}

} // namespace fc::bit
