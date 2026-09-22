// ---------------------------------------------------------------------------
// demo_bitwise - a guided tour of Phase 0.1
//
// Build:  cmake --build build
// Run:    ./build/demo_bitwise
//
// Read the output next to docs/computer-science/*.md.
// ---------------------------------------------------------------------------

#include "core/bit.hpp"
#include "core/types.hpp"

#include <iomanip>
#include <iostream>
#include <string>

using namespace fc;

namespace {

void title(const std::string& text)
{
    std::cout << "\n=== " << text << " ===\n";
}

// --- 1 ---------------------------------------------------------------------
// A byte is 8 bits. Each bit has a "weight", a power of two.
void show_bit_weights()
{
    title("1. Anatomy of one byte");

    std::cout << "bit index :   7   6   5   4   3   2   1   0\n";
    std::cout << "value     : 128  64  32  16   8   4   2   1\n";

    const u8 value = 0b0100'0010;
    std::cout << "example   :   0   1   0   0   0   0   1   0\n";
    std::cout << "            " << bit::to_binary(value, 8, true) << "\n\n";

    std::cout << "Reading it: 64 + 2 = " << static_cast<int>(value) << "\n";
    std::cout << "MSB (bit 7) = " << (bit::test(value, 7) ? 1 : 0)
              << ", LSB (bit 0) = " << (bit::test(value, 0) ? 1 : 0) << "\n";
}

// --- 2 ---------------------------------------------------------------------
// Binary, hexadecimal, decimal are three *views* of the same bits.
void show_number_systems()
{
    title("2. Same bits, three notations");

    std::cout << "dec   bin         hex\n";
    for (int i = 0; i <= 16; ++i) {
        const u8 v = static_cast<u8>(i);
        std::cout << std::setw(3) << i << "   "
                  << bit::to_binary(v, 8, true) << "   "
                  << bit::to_hex(v) << "\n";
    }
    std::cout << "...\n";
    for (int i : { 0x7F, 0x80, 0xFF }) {
        const u8 v = static_cast<u8>(i);
        std::cout << std::setw(3) << i << "   "
                  << bit::to_binary(v, 8, true) << "   "
                  << bit::to_hex(v) << "\n";
    }

    std::cout << "\nNote: 0xFF == 255 as unsigned. One more would need 9 bits.\n";
    std::cout << "This 'wrapping at 256' is exactly what a real 8 bit ALU does.\n";
}

// --- 3 ---------------------------------------------------------------------
void show_bitwise_operators()
{
    title("3. Bitwise operators apply to each bit independently");

    const u8 a = 0b1100'1010; // 0xCA
    const u8 b = 0b1010'0110; // 0xA6

    auto line = [](const char* op, u8 result) {
        std::cout << "  " << op << " -> " << bit::to_binary(result, 8, true)
                  << "   " << bit::to_hex(result) << "\n";
    };

    std::cout << "a       " << bit::to_binary(a, 8, true) << "   " << bit::to_hex(a) << "\n";
    std::cout << "b       " << bit::to_binary(b, 8, true) << "   " << bit::to_hex(b) << "\n\n";

    std::cout << "AND (&) keeps a bit only if BOTH are 1 (used to MASK bits)\n";
    line("a & b", static_cast<u8>(a & b));

    std::cout << "\nOR  (|) keeps a bit if EITHER is 1 (used to SET bits)\n";
    line("a | b", static_cast<u8>(a | b));

    std::cout << "\nXOR (^) keeps a bit if they DIFFER (used to TOGGLE bits)\n";
    line("a ^ b", static_cast<u8>(a ^ b));

    std::cout << "\nNOT (~) flips every bit\n";
    line("~a   ", static_cast<u8>(~a));

    std::cout << "\nSHIFT moves bits left/right, filling with zeros\n";
    line("a<<1 ", static_cast<u8>(a << 1));
    line("a>>1 ", static_cast<u8>(a >> 1));

    std::cout << "\nWatch out: a<<1 dropped bit 7 and put a 0 in bit 0.\n";
    std::cout << "That lost bit is what the CPU's carry flag (C) records.\n";
}

// --- 4 ---------------------------------------------------------------------
void show_single_bit_operations()
{
    title("4. set / clear / toggle a single bit");

    u8 value = 0b0000'0000;
    std::cout << "start          " << bit::to_binary(value, 8, true) << "\n";

    value = bit::set(value, 3);
    std::cout << "set bit 3      " << bit::to_binary(value, 8, true) << "\n";

    value = bit::set(value, 7);
    std::cout << "set bit 7      " << bit::to_binary(value, 8, true) << "\n";

    value = bit::clear(value, 3);
    std::cout << "clear bit 3    " << bit::to_binary(value, 8, true) << "\n";

    value = bit::toggle(value, 0);
    std::cout << "toggle bit 0   " << bit::to_binary(value, 8, true) << "\n";

    std::cout << "\nThis is *the* pattern for CPU flags: the status register P\n";
    std::cout << "is eight independent bits, each flipped by set()/clear().\n";
}

// --- 5 ---------------------------------------------------------------------
void show_twos_complement()
{
    title("5. Two's complement: how a CPU subtracts");

    std::cout << "The hardware only has an adder. To compute 5 - 3 it does 5 + (-3).\n";
    std::cout << "-3 is encoded as ~3 + 1:\n\n";

    const u8 three = 3;
    const u8 minus_three = bit::negate(three);
    std::cout << "  3         = " << bit::to_binary(three, 8, true) << "\n";
    std::cout << " ~3         = " << bit::to_binary(static_cast<u8>(~three), 8, true) << "\n";
    std::cout << " ~3 + 1 = -3= " << bit::to_binary(minus_three, 8, true)
              << "   (unsigned " << static_cast<int>(minus_three) << ")\n\n";

    const u8 sum = static_cast<u8>(5 + minus_three);
    std::cout << "  5 + (-3)  = " << bit::to_binary(sum, 8, true)
              << "   = " << static_cast<int>(sum) << "\n\n";

    std::cout << "One pattern, two meanings:\n";
    std::cout << "  bits       unsigned  signed(two's complement)\n";
    for (int raw : { 0x00, 0x01, 0x7F, 0x80, 0xFF }) {
        const u8 v = static_cast<u8>(raw);
        std::cout << "  " << bit::to_binary(v, 8, true) << "   "
                  << std::setw(8) << static_cast<int>(v) << "  "
                  << std::setw(8) << static_cast<int>(bit::as_signed(v)) << "\n";
    }
    std::cout << "\n0xFF is 255 to unsigned code and -1 to signed code.\n";
    std::cout << "The bits never changed; only the interpretation did.\n";
}

// --- 6 ---------------------------------------------------------------------
void show_little_endian()
{
    title("6. Two bytes make a 16 bit address (little endian)");

    const u16 address = 0x1234;
    const u8 lo = bit::lo_byte(address);
    const u8 hi = bit::hi_byte(address);

    std::cout << "address      = " << bit::to_hex(address) << "  "
              << bit::to_binary(address, 16, true) << "\n";
    std::cout << "low  byte    = " << bit::to_hex(lo) << "\n";
    std::cout << "high byte    = " << bit::to_hex(hi) << "\n\n";
    std::cout << "In NES memory these two bytes sit next to each other, LOW first:\n";
    std::cout << "  [0x0000] = 0x34\n";
    std::cout << "  [0x0001] = 0x12\n\n";
    std::cout << "Reassembled: hi << 8 | lo = " << bit::to_hex(bit::make_u16(lo, hi)) << "\n";
    std::cout << "The 6502 does exactly this every time it reads an address.\n";
}

} // namespace

int main()
{
    std::cout << "Classic Game Box - Phase 0.1: bits, bytes and numbers\n";

    show_bit_weights();
    show_number_systems();
    show_bitwise_operators();
    show_single_bit_operations();
    show_twos_complement();
    show_little_endian();

    title("Summary");
    std::cout << "bit      : one 0 or 1\n";
    std::cout << "byte     : 8 bits, 0..255 unsigned, -128..127 signed\n";
    std::cout << "hex      : a compact way to write 4 bits per digit\n";
    std::cout << "two's c. : ~x + 1, lets an adder subtract\n";
    std::cout << "little e.: low byte first, how the 6502 stores 16 bit values\n";

    return 0;
}
