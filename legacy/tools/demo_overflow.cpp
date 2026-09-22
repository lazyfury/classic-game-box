// ---------------------------------------------------------------------------
// demo_overflow - why the NES needs a separate V flag
//
// Build:  cmake --build build
// Run:    ./build/demo_overflow
//
// Read together with docs/computer-science/overflow-flag.md
// ---------------------------------------------------------------------------

#include "core/alu.hpp"
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

std::string signed_str(u8 raw)
{
    return std::to_string(static_cast<int>(bit::as_signed(raw)));
}

// --- 1 ---------------------------------------------------------------------
void oneAdditionTwoQuestions()
{
    title("1. One addition, two different questions");

    struct Case { u8 a; u8 b; };
    const Case cases[] = {
        { 0x10, 0x20 },  //  16 +  32
        { 0x7F, 0x01 },  // 127 +   1
        { 0xFF, 0x01 },  //  -1 +   1
        { 0x80, 0x80 },  // -128 + -128
    };

    std::cout << "  a     b      result   C     V    a+b (signed)\n";
    std::cout << "  -------------------------------------------------\n";

    for (const auto& c : cases) {
        const auto r = alu::add(c.a, c.b);
        std::cout << "  " << bit::to_hex(c.a) << "  " << bit::to_hex(c.b)
                  << "   " << bit::to_hex(r.result) << "   "
                  << (r.carry ? "1" : "0") << "     "
                  << (r.overflow ? "1" : "0") << "     "
                  << std::setw(4) << signed_str(c.a) << " + "
                  << std::setw(4) << signed_str(c.b) << "\n";
    }

    std::cout << "\n  C = 'does not fit if you read it as UNSIGNED (0..255)'\n";
    std::cout << "  V = 'does not fit if you read it as SIGNED (-128..127)'\n";
    std::cout << "\n  Look at 0x7F + 0x01: C=0 but V=1.\n";
    std::cout << "  Reading C here would say 'no problem' - and be wrong.\n";
}

// --- 2 ---------------------------------------------------------------------
void carryIntoVsCarryOut()
{
    title("2. The hardware rule: V = carry_in XOR carry_out of bit 7");

    struct Case { u8 a; u8 b; const char* note; };
    const Case cases[] = {
        { 0x7F, 0x01, "+127 + 1  -> carry in, no carry out" },
        { 0xFF, 0x01, "  -1 + 1  -> carry in and carry out" },
        { 0x80, 0xFF, "-128 - 1  -> carry out, no carry in" },
        { 0x10, 0x20, "  16 + 32 -> neither" },
    };

    std::cout << "  operands      carry_into7  carry_out7(C)  XOR  V\n";
    std::cout << "  ----------------------------------------------------\n";

    for (const auto& c : cases) {
        const auto t = alu::trace_add(c.a, c.b);
        std::cout << "  " << bit::to_hex(c.a) << " + " << bit::to_hex(c.b)
                  << "        " << (t.carry_into_bit7 ? "1" : "0")
                  << "            " << (t.carry_out_of_bit7 ? "1" : "0")
                  << "          " << (t.carry_into_bit7 != t.carry_out_of_bit7 ? "1" : "0")
                  << "\n";
        std::cout << "      " << c.note << "\n";
    }

    std::cout << "\n  Why XOR works: the sign bit is just another adder.\n";
    std::cout << "  Carry in but none out -> a positive number got its sign bit\n";
    std::cout << "  pushed to 1, so it suddenly reads as negative. Corrupted.\n";
}

// --- 3 ---------------------------------------------------------------------
void programmerRule()
{
    title("3. The rule you actually write in code");

    std::cout << "  V = (a and result differ in sign) AND (b and result differ)\n";
    std::cout << "  which means: the two operands shared a sign, the result did not.\n\n";

    std::cout << "  Why? A positive plus a negative always lands BETWEEN them,\n";
    std::cout << "  so it can never leave -128..127. Only same-sign addition can.\n\n";

    const u8 pairs[][2] = {
        { 0x7F, 0x01 }, { 0x7F, 0x7F }, { 0x50, 0x50 },
        { 0x80, 0x80 }, { 0xFF, 0xFF }, { 0x80, 0xFF },
        { 0x40, 0x40 }, { 0x80, 0x01 }, { 0xFF, 0x01 },
    };

    std::cout << "  a     b     result   a^r    b^r    V\n";
    std::cout << "  ----------------------------------------\n";
    for (const auto& p : pairs) {
        const auto r = alu::add(p[0], p[1]);
        std::cout << "  " << bit::to_hex(p[0]) << "  " << bit::to_hex(p[1])
                  << "   " << bit::to_hex(r.result) << "   "
                  << bit::to_hex(static_cast<u8>(p[0] ^ r.result)) << "  "
                  << bit::to_hex(static_cast<u8>(p[1] ^ r.result)) << "   "
                  << (r.overflow ? "1" : "0") << "\n";
    }
    std::cout << "\n  Only when BOTH a^result and b^result have bit 7 set\n";
    std::cout << "  do the operands agree with each other but not with the result.\n";
}

// --- 4 ---------------------------------------------------------------------
void allFourCombinations()
{
    title("4. C and V are independent - all four combinations exist");

    struct Case { u8 a; u8 b; const char* label; };
    const Case cases[] = {
        { 0x10, 0x20, "16 + 32" },
        { 0xFF, 0x01, "-1 + 1" },
        { 0x7F, 0x01, "127 + 1" },
        { 0x80, 0x80, "-128 + -128" },
    };

    for (const auto& c : cases) {
        const auto r = alu::add(c.a, c.b);
        std::cout << "  C=" << (r.carry ? 1 : 0)
                  << " V=" << (r.overflow ? 1 : 0)
                  << "   result " << bit::to_hex(r.result)
                  << "   (" << c.label << ")\n";
    }

    std::cout << "\n  If C and V carried the same information, this table would\n";
    std::cout << "  only have two rows. It has four.\n";
}

// --- 5 ---------------------------------------------------------------------
void signedComparison()
{
    title("5. Why the NES cannot do signed comparison without V");

    std::cout << "  6502 has no 'compare signed' instruction. It has CMP (a\n";
    std::cout << "  subtraction) plus branches. After A - M the TRUE sign of the\n";
    std::cout << "  difference is  N XOR V,  not just N.\n\n";

    struct Case { u8 a; u8 m; };
    const Case cases[] = {
        { 0x01, 0x02 },  //   1 vs   2
        { 0x02, 0x01 },  //   2 vs   1
        { 0xFF, 0x01 },  //  -1 vs   1
        { 0x01, 0xFF },  //   1 vs  -1
        { 0x80, 0x01 },  //-128 vs   1
        { 0x7F, 0xFF },  // 127 vs  -1
        { 0xFF, 0x80 },  //  -1 vs-128
    };

    std::cout << "  A      M      A-M     N  V  N^V   A<M (signed)?\n";
    std::cout << "  -------------------------------------------------\n";

    for (const auto& c : cases) {
        const auto r = alu::subtract(c.a, c.m);
        const bool true_sign_negative = (r.negative != r.overflow);
        const int av = static_cast<int>(bit::as_signed(c.a));
        const int mv = static_cast<int>(bit::as_signed(c.m));

        std::cout << "  " << std::setw(4) << av << "   " << std::setw(4) << mv
                  << "   " << bit::to_hex(r.result)
                  << "    " << (r.negative ? 1 : 0)
                  << "  " << (r.overflow ? 1 : 0)
                  << "   " << (true_sign_negative ? 1 : 0)
                  << "     " << (av < mv ? "yes" : "no") << "\n";
    }

    std::cout << "\n  Row 5 is the interesting one: -128 - 1 overflows, so N alone\n";
    std::cout << "  says 'positive' (N=0) even though -128 < 1. Only N XOR V gives\n";
    std::cout << "  the right answer. This is why BMI/BPL must be paired with\n";
    std::cout << "  BVS/BVC for signed comparisons.\n";
}

} // namespace

int main()
{
    std::cout << "Classic Game Box - the V (overflow) flag\n";

    oneAdditionTwoQuestions();
    carryIntoVsCarryOut();
    programmerRule();
    allFourCombinations();
    signedComparison();

    title("Summary");
    std::cout << "C : unsigned overflow, the carry out of bit 7\n";
    std::cout << "V : signed overflow, carry_into7 XOR carry_out7\n";
    std::cout << "N : just a copy of bit 7, NOT a verdict\n";
    std::cout << "true sign of a difference = N XOR V\n";

    return 0;
}
