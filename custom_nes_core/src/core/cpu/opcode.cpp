#include "core/cpu/opcode.hpp"

namespace fc {
namespace {

using AM = AddressingMode;
using Op = Operation;

// ---------------------------------------------------------------------------
// The official NMOS 6502 opcode matrix.
//
// Layout: 16 rows of 16. The row is the high nibble, the column the low one.
// Read it in groups of four to keep the rows scannable.
//
// Op::Unknown marks the 105 codes the official chip leaves undefined.
// Those are the illegal opcodes - some of them do something on real
// hardware, but nothing here relies on that.
//
// The table is mechanical but it is also the single source of truth for
// instruction length and operand layout. It is worth reading once, slowly.
// ---------------------------------------------------------------------------

constexpr OpcodeInfo kOpcodeTable[256] = {

    // 0x0_ : BRK ORA     ASL PHP
    { Op::BRK, AM::Implied },   { Op::ORA, AM::IndirectX }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::ORA, AM::ZeroPage },  { Op::ASL, AM::ZeroPage }, { Op::Unknown, AM::Unknown },
    { Op::PHP, AM::Implied },   { Op::ORA, AM::Immediate }, { Op::ASL, AM::Accumulator }, { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::ORA, AM::Absolute },  { Op::ASL, AM::Absolute }, { Op::Unknown, AM::Unknown },

    // 0x1_ : BPL ORA     ASL CLC
    { Op::BPL, AM::Relative },  { Op::ORA, AM::IndirectY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::ORA, AM::ZeroPageX }, { Op::ASL, AM::ZeroPageX }, { Op::Unknown, AM::Unknown },
    { Op::CLC, AM::Implied },   { Op::ORA, AM::AbsoluteY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::ORA, AM::AbsoluteX }, { Op::ASL, AM::AbsoluteX }, { Op::Unknown, AM::Unknown },

    // 0x2_ : JSR AND BIT ROL PLP
    { Op::JSR, AM::Absolute },  { Op::AND, AM::IndirectX }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::BIT, AM::ZeroPage },  { Op::AND, AM::ZeroPage },  { Op::ROL, AM::ZeroPage }, { Op::Unknown, AM::Unknown },
    { Op::PLP, AM::Implied },   { Op::AND, AM::Immediate }, { Op::ROL, AM::Accumulator }, { Op::Unknown, AM::Unknown },
    { Op::BIT, AM::Absolute },  { Op::AND, AM::Absolute },  { Op::ROL, AM::Absolute }, { Op::Unknown, AM::Unknown },

    // 0x3_ : BMI AND ROL SEC
    { Op::BMI, AM::Relative },  { Op::AND, AM::IndirectY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::AND, AM::ZeroPageX }, { Op::ROL, AM::ZeroPageX }, { Op::Unknown, AM::Unknown },
    { Op::SEC, AM::Implied },   { Op::AND, AM::AbsoluteY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::AND, AM::AbsoluteX }, { Op::ROL, AM::AbsoluteX }, { Op::Unknown, AM::Unknown },

    // 0x4_ : RTI EOR LSR PHA JMP
    { Op::RTI, AM::Implied },   { Op::EOR, AM::IndirectX }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::EOR, AM::ZeroPage },  { Op::LSR, AM::ZeroPage }, { Op::Unknown, AM::Unknown },
    { Op::PHA, AM::Implied },   { Op::EOR, AM::Immediate }, { Op::LSR, AM::Accumulator }, { Op::Unknown, AM::Unknown },
    { Op::JMP, AM::Absolute },  { Op::EOR, AM::Absolute },  { Op::LSR, AM::Absolute }, { Op::Unknown, AM::Unknown },

    // 0x5_ : BVC EOR LSR CLI
    { Op::BVC, AM::Relative },  { Op::EOR, AM::IndirectY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::EOR, AM::ZeroPageX }, { Op::LSR, AM::ZeroPageX }, { Op::Unknown, AM::Unknown },
    { Op::CLI, AM::Implied },   { Op::EOR, AM::AbsoluteY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::EOR, AM::AbsoluteX }, { Op::LSR, AM::AbsoluteX }, { Op::Unknown, AM::Unknown },

    // 0x6_ : RTS ADC ROR PLA JMP(ind)
    { Op::RTS, AM::Implied },   { Op::ADC, AM::IndirectX }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::ADC, AM::ZeroPage },  { Op::ROR, AM::ZeroPage }, { Op::Unknown, AM::Unknown },
    { Op::PLA, AM::Implied },   { Op::ADC, AM::Immediate }, { Op::ROR, AM::Accumulator }, { Op::Unknown, AM::Unknown },
    { Op::JMP, AM::Indirect },  { Op::ADC, AM::Absolute },  { Op::ROR, AM::Absolute }, { Op::Unknown, AM::Unknown },

    // 0x7_ : BVS ADC ROR SEI
    { Op::BVS, AM::Relative },  { Op::ADC, AM::IndirectY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::ADC, AM::ZeroPageX }, { Op::ROR, AM::ZeroPageX }, { Op::Unknown, AM::Unknown },
    { Op::SEI, AM::Implied },   { Op::ADC, AM::AbsoluteY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::ADC, AM::AbsoluteX }, { Op::ROR, AM::AbsoluteX }, { Op::Unknown, AM::Unknown },

    // 0x8_ : STA STX STY DEY TXA
    { Op::Unknown, AM::Unknown },   { Op::STA, AM::IndirectX }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::STY, AM::ZeroPage },  { Op::STA, AM::ZeroPage },  { Op::STX, AM::ZeroPage }, { Op::Unknown, AM::Unknown },
    { Op::DEY, AM::Implied },   { Op::Unknown, AM::Unknown },   { Op::TXA, AM::Implied },  { Op::Unknown, AM::Unknown },
    { Op::STY, AM::Absolute },  { Op::STA, AM::Absolute },  { Op::STX, AM::Absolute }, { Op::Unknown, AM::Unknown },

    // 0x9_ : BCC STA STY STX TYA TXS
    { Op::BCC, AM::Relative },  { Op::STA, AM::IndirectY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::STY, AM::ZeroPageX }, { Op::STA, AM::ZeroPageX }, { Op::STX, AM::ZeroPageY }, { Op::Unknown, AM::Unknown },
    { Op::TYA, AM::Implied },   { Op::STA, AM::AbsoluteY }, { Op::TXS, AM::Implied },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::STA, AM::AbsoluteX }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },

    // 0xA_ : LDY LDX LDA TAY TAX
    { Op::LDY, AM::Immediate }, { Op::LDA, AM::IndirectX }, { Op::LDX, AM::Immediate }, { Op::Unknown, AM::Unknown },
    { Op::LDY, AM::ZeroPage },  { Op::LDA, AM::ZeroPage },  { Op::LDX, AM::ZeroPage }, { Op::Unknown, AM::Unknown },
    { Op::TAY, AM::Implied },   { Op::LDA, AM::Immediate }, { Op::TAX, AM::Implied },  { Op::Unknown, AM::Unknown },
    { Op::LDY, AM::Absolute },  { Op::LDA, AM::Absolute },  { Op::LDX, AM::Absolute }, { Op::Unknown, AM::Unknown },

    // 0xB_ : BCS LDA LDY CLV TSX
    { Op::BCS, AM::Relative },  { Op::LDA, AM::IndirectY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::LDY, AM::ZeroPageX }, { Op::LDA, AM::ZeroPageX }, { Op::LDX, AM::ZeroPageY }, { Op::Unknown, AM::Unknown },
    { Op::CLV, AM::Implied },   { Op::LDA, AM::AbsoluteY }, { Op::TSX, AM::Implied },  { Op::Unknown, AM::Unknown },
    { Op::LDY, AM::AbsoluteX }, { Op::LDA, AM::AbsoluteX }, { Op::LDX, AM::AbsoluteY }, { Op::Unknown, AM::Unknown },

    // 0xC_ : CPY CMP DEC INY DEX
    { Op::CPY, AM::Immediate }, { Op::CMP, AM::IndirectX }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::CPY, AM::ZeroPage },  { Op::CMP, AM::ZeroPage },  { Op::DEC, AM::ZeroPage }, { Op::Unknown, AM::Unknown },
    { Op::INY, AM::Implied },   { Op::CMP, AM::Immediate }, { Op::DEX, AM::Implied },  { Op::Unknown, AM::Unknown },
    { Op::CPY, AM::Absolute },  { Op::CMP, AM::Absolute },  { Op::DEC, AM::Absolute }, { Op::Unknown, AM::Unknown },

    // 0xD_ : BNE CMP DEC CLD
    { Op::BNE, AM::Relative },  { Op::CMP, AM::IndirectY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::CMP, AM::ZeroPageX }, { Op::DEC, AM::ZeroPageX }, { Op::Unknown, AM::Unknown },
    { Op::CLD, AM::Implied },   { Op::CMP, AM::AbsoluteY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::CMP, AM::AbsoluteX }, { Op::DEC, AM::AbsoluteX }, { Op::Unknown, AM::Unknown },

    // 0xE_ : CPX SBC INC INX NOP
    { Op::CPX, AM::Immediate }, { Op::SBC, AM::IndirectX }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::CPX, AM::ZeroPage },  { Op::SBC, AM::ZeroPage },  { Op::INC, AM::ZeroPage }, { Op::Unknown, AM::Unknown },
    { Op::INX, AM::Implied },   { Op::SBC, AM::Immediate }, { Op::NOP, AM::Implied },  { Op::Unknown, AM::Unknown },
    { Op::CPX, AM::Absolute },  { Op::SBC, AM::Absolute },  { Op::INC, AM::Absolute }, { Op::Unknown, AM::Unknown },

    // 0xF_ : BEQ SBC INC SED
    { Op::BEQ, AM::Relative },  { Op::SBC, AM::IndirectY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::SBC, AM::ZeroPageX }, { Op::INC, AM::ZeroPageX }, { Op::Unknown, AM::Unknown },
    { Op::SED, AM::Implied },   { Op::SBC, AM::AbsoluteY }, { Op::Unknown, AM::Unknown },  { Op::Unknown, AM::Unknown },
    { Op::Unknown, AM::Unknown },   { Op::SBC, AM::AbsoluteX }, { Op::INC, AM::AbsoluteX }, { Op::Unknown, AM::Unknown },
};

static_assert(sizeof(kOpcodeTable) / sizeof(kOpcodeTable[0]) == 256,
              "the opcode table must cover all 256 byte values");

// ---------------------------------------------------------------------------
// Cycle counts, from the NMOS 6502 datasheet.
//
// 16 rows of 16, in the same layout as the opcode matrix above, so the two
// can be read side by side. These are the values every emulator is checked
// against; they are data, not something to derive.
//
// Undefined opcodes are listed as 2 - the length we assume for them. Nothing
// executes them, so the number never reaches the cycle counter (the CPU halts
// on them first), but leaving it 0 would be misleading in a listing.
// ---------------------------------------------------------------------------

constexpr u8 kCycleTable[256] = {

    // 0x0_   BRK  ORA  ---  ---  ---  ORA  ASL  ---  PHP  ORA  ASL  ---  ---  ORA  ASL  ---
              7,   6,   2,   2,   2,   3,   5,   2,   3,   2,   2,   2,   2,   4,   6,   2,

    // 0x1_   BPL  ORA  ---  ---  ---  ORA  ASL  ---  CLC  ORA  ---  ---  ---  ORA  ASL  ---
              2,   5,   2,   2,   2,   4,   6,   2,   2,   4,   2,   2,   2,   4,   7,   2,

    // 0x2_   JSR  AND  ---  ---  BIT  AND  ROL  ---  PLP  AND  ROL  ---  BIT  AND  ROL  ---
              6,   6,   2,   2,   3,   3,   5,   2,   4,   2,   2,   2,   4,   4,   6,   2,

    // 0x3_   BMI  AND  ---  ---  ---  AND  ROL  ---  SEC  AND  ---  ---  ---  AND  ROL  ---
              2,   5,   2,   2,   2,   4,   6,   2,   2,   4,   2,   2,   2,   4,   7,   2,

    // 0x4_   RTI  EOR  ---  ---  ---  EOR  LSR  ---  PHA  EOR  LSR  ---  JMP  EOR  LSR  ---
              6,   6,   2,   2,   2,   3,   5,   2,   3,   2,   2,   2,   3,   4,   6,   2,

    // 0x5_   BVC  EOR  ---  ---  ---  EOR  LSR  ---  CLI  EOR  ---  ---  ---  EOR  LSR  ---
              2,   5,   2,   2,   2,   4,   6,   2,   2,   4,   2,   2,   2,   4,   7,   2,

    // 0x6_   RTS  ADC  ---  ---  ---  ADC  ROR  ---  PLA  ADC  ROR  ---  JMP  ADC  ROR  ---
              6,   6,   2,   2,   2,   3,   5,   2,   4,   2,   2,   2,   5,   4,   6,   2,

    // 0x7_   BVS  ADC  ---  ---  ---  ADC  ROR  ---  SEI  ADC  ---  ---  ---  ADC  ROR  ---
              2,   5,   2,   2,   2,   4,   6,   2,   2,   4,   2,   2,   2,   4,   7,   2,

    // 0x8_   ---  STA  ---  ---  STY  STA  STX  ---  DEY  ---  TXA  ---  STY  STA  STX  ---
              2,   6,   2,   2,   3,   3,   3,   2,   2,   2,   2,   2,   4,   4,   4,   2,

    // 0x9_   BCC  STA  ---  ---  STY  STA  STX  ---  TYA  STA  TXS  ---  ---  STA  ---  ---
              2,   6,   2,   2,   4,   4,   4,   2,   2,   5,   2,   2,   2,   5,   2,   2,

    // 0xA_   LDY  LDA  LDX  ---  LDY  LDA  LDX  ---  TAY  LDA  TAX  ---  LDY  LDA  LDX  ---
              2,   6,   2,   2,   3,   3,   3,   2,   2,   2,   2,   2,   4,   4,   4,   2,

    // 0xB_   BCS  LDA  ---  ---  LDY  LDA  LDX  ---  CLV  LDA  TSX  ---  LDY  LDA  LDX  ---
              2,   5,   2,   2,   4,   4,   4,   2,   2,   4,   2,   2,   4,   4,   4,   2,

    // 0xC_   CPY  CMP  ---  ---  CPY  CMP  DEC  ---  INY  CMP  DEX  ---  CPY  CMP  DEC  ---
              2,   6,   2,   2,   3,   3,   5,   2,   2,   2,   2,   2,   4,   4,   6,   2,

    // 0xD_   BNE  CMP  ---  ---  ---  CMP  DEC  ---  CLD  CMP  ---  ---  ---  CMP  DEC  ---
              2,   5,   2,   2,   2,   4,   6,   2,   2,   4,   2,   2,   2,   4,   7,   2,

    // 0xE_   CPX  SBC  ---  ---  CPX  SBC  INC  ---  INX  SBC  NOP  ---  CPX  SBC  INC  ---
              2,   6,   2,   2,   3,   3,   5,   2,   2,   2,   2,   2,   4,   4,   6,   2,

    // 0xF_   BEQ  SBC  ---  ---  ---  SBC  INC  ---  SED  SBC  ---  ---  ---  SBC  INC  ---
              2,   5,   2,   2,   2,   4,   6,   2,   2,   4,   2,   2,   2,   4,   7,   2,
};

static_assert(sizeof(kCycleTable) / sizeof(kCycleTable[0]) == 256,
              "the cycle table must cover all 256 byte values");

} // namespace

const OpcodeInfo& opcode_info(u8 opcode) noexcept
{
    return kOpcodeTable[opcode];
}

u8 opcode_cycles(u8 opcode) noexcept
{
    return kCycleTable[opcode];
}

} // namespace fc
