#include "core/cpu/cpu.hpp"

#include "core/alu.hpp"
#include "core/bit.hpp"

namespace fc {

namespace {
constexpr u16 kResetVector = 0xFFFC; // $FFFC-$FFFD hold the entry address
constexpr u16 kNmiVector   = 0xFFFA; // $FFFA-$FFFB, once per frame
constexpr u16 kIrqVector   = 0xFFFE; // $FFFE-$FFFF, shared with BRK
constexpr u16 kStackPage   = 0x0100; // the stack lives only in page 1

/// An interrupt is not an instruction: E7 on most 6502 documents. The CPU
/// pushes three bytes and loads a vector, and that takes 7 cycles.
constexpr int kInterruptCycles = 7;
} // namespace

// ---------------------------------------------------------------------------
// reset
// ---------------------------------------------------------------------------

void Cpu::reset() noexcept
{
    reg_.reset();

    // The 6502 does not start at 0. It reads a 16 bit address from the reset
    // vector and jumps there. Two bytes, little endian:
    //
    //     [$FFFC] = 0x00
    //     [$FFFD] = 0x80
    //     -> entry point 0x8000
    const u8 lo = read(kResetVector);
    const u8 hi = read(static_cast<u16>(kResetVector + 1));
    reg_.pc = bit::make_u16(lo, hi);

    cycles_ = 0;
    halted_ = false;
    unimplemented_opcode_ = 0;
    instruction_pc_ = reg_.pc;
    last_opcode_ = 0;
    branch_extra_cycles_ = 0;
    nmi_pending_ = false;
    // The IRQ line is owned by the caller (the bus / PPU in Phase 2).
}

// ---------------------------------------------------------------------------
// fetch
// ---------------------------------------------------------------------------

u8 Cpu::fetch_byte() noexcept
{
    // Read the byte at PC, then move PC on. Because PC is u16 it wraps
    // 0xFFFF -> 0x0000 by itself, exactly like 16 address wires.
    return read(reg_.pc++);
}

// ---------------------------------------------------------------------------
// stack
// ---------------------------------------------------------------------------

void Cpu::push(u8 value) noexcept
{
    write(static_cast<u16>(kStackPage | reg_.sp), value);
    reg_.sp = static_cast<u8>(reg_.sp - 1u);
}

u8 Cpu::pop() noexcept
{
    reg_.sp = static_cast<u8>(reg_.sp + 1u);
    return read(static_cast<u16>(kStackPage | reg_.sp));
}

void Cpu::push_word(u16 value) noexcept
{
    // The 6502 pushes the HIGH byte first, so the low byte ends up on top of
    // the stack and comes back first. That is what makes RTS work.
    push(bit::hi_byte(value));
    push(bit::lo_byte(value));
}

u16 Cpu::pull_word() noexcept
{
    const u8 lo = pop();
    const u8 hi = pop();
    return bit::make_u16(lo, hi);
}

u8 Cpu::status_for_push(bool break_flag) const noexcept
{
    // Bit 5 is not a real flag: it always reads as 1.
    // Bit 4 is not a real flag either. It exists ONLY in the pushed copy, and
    // is what lets software tell a BRK from a hardware interrupt:
    //   PHP / BRK        push it set
    //   NMI / IRQ        push it clear
    u8 value = reg_.p;
    value = bit::assign(value, static_cast<int>(Flag::Unused), true);
    value = bit::assign(value, static_cast<int>(Flag::Break), break_flag);
    return value;
}

void Cpu::restore_status(u8 value) noexcept
{
    // Bit 5 always comes back as 1.
    value = bit::assign(value, static_cast<int>(Flag::Unused), true);
    // Bit 4 is dropped: it is not a flag the CPU can hold, and the only two
    // things that ever push it (PHP/BRK and interrupts) overwrite it anyway.
    value = bit::assign(value, static_cast<int>(Flag::Break), false);
    reg_.p = value;
}

// ---------------------------------------------------------------------------
// step - one full fetch / decode / address / execute round
// ---------------------------------------------------------------------------

int Cpu::step() noexcept
{
    if (halted_) {
        return 0;
    }

    // An interrupt is only ever taken BETWEEN instructions. That is why every
    // instruction is atomic from the CPU's point of view: a game can rely on
    // a read-modify-write never being split.
    if (interrupt_pending()) {
        const u64 before = cycles_;
        const bool is_nmi = nmi_pending_;
        nmi_pending_ = false;
        enter_interrupt(is_nmi ? kNmiVector : kIrqVector, false);
        cycles_ += kInterruptCycles;
        return static_cast<int>(cycles_ - before);
    }

    const u64 cycles_before = cycles_;

    // --- fetch the opcode ---------------------------------------------------
    instruction_pc_ = reg_.pc;
    last_opcode_ = fetch_byte();

    // --- decode: the table turns one byte into (operation, mode) ------------
    const OpcodeInfo& info = opcode_info(last_opcode_);

    // --- address: fetch operand bytes, then compute the effective address ---
    AddressingRequest request{};
    request.mode = info.mode;
    request.instruction_pc = instruction_pc_;
    request.x = reg_.x;   // sampled BEFORE the instruction, as the hardware does
    request.y = reg_.y;

    const int operand_bytes = operand_length(info.mode);
    if (operand_bytes >= 1) {
        request.operand_lo = fetch_byte();
    }
    if (operand_bytes >= 2) {
        request.operand_hi = fetch_byte();
    }

    const Operand operand = resolve(request, *bus_);

    // --- execute ------------------------------------------------------------
    branch_extra_cycles_ = 0;
    execute(info, operand);
    cycles_ += static_cast<u64>(cycle_cost(last_opcode_, operand));

    // The bus may have stolen time on its own - OAM DMA is the canonical
    // case. The CPU does not need to know what a device is to wait for one.
    cycles_ += static_cast<u64>(bus_->take_stall_cycles());

    return static_cast<int>(cycles_ - cycles_before);
}

int Cpu::run(int max_steps) noexcept
{
    int executed = 0;
    while (!halted_ && executed < max_steps) {
        step();
        ++executed;
    }
    return executed;
}

void Cpu::halt(u8 opcode) noexcept
{
    halted_ = true;
    unimplemented_opcode_ = opcode;
}

// ---------------------------------------------------------------------------
// interrupts
// ---------------------------------------------------------------------------

bool Cpu::interrupt_pending() const noexcept
{
    // NMI cannot be masked. IRQ can: that is what the I flag is for.
    if (nmi_pending_) {
        return true;
    }
    return irq_line_ && !reg_.flag(Flag::IrqDisable);
}

void Cpu::enter_interrupt(u16 vector, bool is_break) noexcept
{
    // Where the interrupted program should resume.
    // For a hardware interrupt this is the next instruction. For BRK the
    // caller has already added the extra byte.
    push_word(reg_.pc);
    push(status_for_push(is_break));

    // Servicing an interrupt disables further IRQs. NMI ignores this.
    reg_.set_flag(Flag::IrqDisable, true);

    const u8 lo = read(vector);
    const u8 hi = read(static_cast<u16>(vector + 1));
    reg_.pc = bit::make_u16(lo, hi);

    // Note: the 7 cycles are added by the caller, because BRK gets its count
    // from the opcode table instead.
}

void Cpu::service_interrupt(bool is_break) noexcept
{
    enter_interrupt(kIrqVector, is_break);
    cycles_ += kInterruptCycles;
}

void Cpu::service_nmi() noexcept
{
    nmi_pending_ = false;
    enter_interrupt(kNmiVector, false);
    cycles_ += kInterruptCycles;
}

// ---------------------------------------------------------------------------
// operation support
// ---------------------------------------------------------------------------

bool Cpu::handles(Operation op) noexcept
{
    // Phase 1 implements every legal operation. Only the illegal-opcode
    // placeholder is left over, and it never gets executed.
    return op != Operation::Unknown;
}

bool Cpu::implements(u8 opcode) noexcept
{
    return opcode_info(opcode).is_legal();
}

u8 Cpu::operand_value(const OpcodeInfo& info, const Operand& operand) noexcept
{
    switch (operand.kind) {
    case OperandKind::Value:
        return operand.value;

    case OperandKind::Address:
        return read(operand.address);

    case OperandKind::None:
        // Only the accumulator mode has a "none" operand that still carries
        // a value.
        if (info.mode == AddressingMode::Accumulator) {
            return reg_.a;
        }
        return 0;

    case OperandKind::Target:
        return 0;
    }
    return 0;
}

void Cpu::store_operand(const OpcodeInfo& info, const Operand& operand, u8 value) noexcept
{
    if (info.mode == AddressingMode::Accumulator) {
        reg_.a = value;
        return;
    }
    if (operand.kind == OperandKind::Address) {
        write(operand.address, value);
    }
}

// ---------------------------------------------------------------------------
// helpers used by several operations
// ---------------------------------------------------------------------------

void Cpu::compare(u8 left, u8 right) noexcept
{
    // CMP is a subtraction that throws the result away and keeps the flags.
    //
    //   C = 1 when left >= right   (no borrow)
    //   Z = 1 when left == right
    //   N = bit 7 of (left - right)
    //
    // The comparison is UNSIGNED. To compare signed values a program must
    // combine N with V, see docs/computer-science/overflow-flag.md.
    // alu::subtract(a, b) computes a + ~b + 1 and reports the carry out.
    // That carry is 1 exactly when no borrow was needed, which is CMP's C.
    // (Phase 0.4 had this inverted; the test above is what caught it.)
    const alu::AddResult result = alu::subtract(left, right);

    reg_.set_flag(Flag::Carry, result.carry);
    reg_.set_flag(Flag::Zero, result.zero);
    reg_.set_flag(Flag::Negative, result.negative);
}

void Cpu::branch(bool condition, const Operand& operand) noexcept
{
    if (!condition) {
        return;
    }

    // PC has already moved past the two instruction bytes.
    const u16 next_instruction = reg_.pc;
    reg_.pc = operand.address;

    branch_extra_cycles_ = 1;
    if (bit::hi_byte(next_instruction) != bit::hi_byte(operand.address)) {
        branch_extra_cycles_ = 2;   // taking a branch across a page costs one more
    }
}

// ---------------------------------------------------------------------------
// cycle accounting
//
// APPROXIMATE. Phase 1 replaces this with the authoritative per-opcode table.
// It is right for the common cases and close for the rest.
// ---------------------------------------------------------------------------

int Cpu::cycle_cost(u8 opcode, const Operand& operand) const noexcept
{
    const OpcodeInfo& info = opcode_info(opcode);

    // The datasheet number already covers the fetch, the operand fetches, the
    // memory access and the read-modify-write write-back.
    int cycles = opcode_cycles(opcode);

    // The two data dependent penalties.
    if (pays_page_penalty(info.op, info.mode) && operand.page_crossed) {
        ++cycles;
    }
    cycles += branch_extra_cycles_;

    return cycles;
}

// ---------------------------------------------------------------------------
// decode + execute
//
// One case per OPERATION, not per opcode. The addressing mode was already
// resolved before we get here, so `operand_value` and `store_operand` work
// unchanged whether the data came from `#$42`, `$42`, `$8000,X` or `($42),Y`.
// ---------------------------------------------------------------------------

void Cpu::execute(const OpcodeInfo& info, const Operand& operand) noexcept
{
    switch (info.op) {

    // -- implied: transfers between registers --------------------------------
    // These ignore the operand entirely; it is None.

    case Operation::TAX:
        reg_.x = reg_.a;
        reg_.update_nz(reg_.x);
        break;

    case Operation::TAY:
        reg_.y = reg_.a;
        reg_.update_nz(reg_.y);
        break;

    case Operation::TXA:
        reg_.a = reg_.x;
        reg_.update_nz(reg_.a);
        break;

    case Operation::TYA:
        reg_.a = reg_.y;
        reg_.update_nz(reg_.a);
        break;

    case Operation::TSX:
        reg_.x = reg_.sp;
        reg_.update_nz(reg_.x);
        break;

    case Operation::TXS:
        reg_.sp = reg_.x;
        break;   // TXS is the one transfer that does NOT touch the flags

    // -- implied: increment / decrement --------------------------------------
    // All of these wrap: $FF + 1 = $00. The 6502 does not saturate.

    case Operation::INX:
        reg_.x = static_cast<u8>(reg_.x + 1u);
        reg_.update_nz(reg_.x);
        break;

    case Operation::INY:
        reg_.y = static_cast<u8>(reg_.y + 1u);
        reg_.update_nz(reg_.y);
        break;

    case Operation::DEX:
        reg_.x = static_cast<u8>(reg_.x - 1u);
        reg_.update_nz(reg_.x);
        break;

    case Operation::DEY:
        reg_.y = static_cast<u8>(reg_.y - 1u);
        reg_.update_nz(reg_.y);
        break;

    // -- implied: flag control ----------------------------------------------

    case Operation::CLC: reg_.set_flag(Flag::Carry, false); break;
    case Operation::SEC: reg_.set_flag(Flag::Carry, true);  break;
    case Operation::CLI: reg_.set_flag(Flag::IrqDisable, false); break;
    case Operation::SEI: reg_.set_flag(Flag::IrqDisable, true);  break;
    case Operation::CLD: reg_.set_flag(Flag::Decimal, false); break;
    case Operation::SED: reg_.set_flag(Flag::Decimal, true);  break;
    case Operation::CLV: reg_.set_flag(Flag::Overflow, false); break;

    // -- implied: nothing at all ---------------------------------------------

    case Operation::NOP:
        break;

    // -- loads ---------------------------------------------------------------
    // `operand_value` already did the read, whatever the mode was.

    case Operation::LDA:
        reg_.a = operand_value(info, operand);
        reg_.update_nz(reg_.a);
        break;

    case Operation::LDX:
        reg_.x = operand_value(info, operand);
        reg_.update_nz(reg_.x);
        break;

    case Operation::LDY:
        reg_.y = operand_value(info, operand);
        reg_.update_nz(reg_.y);
        break;

    // -- stores --------------------------------------------------------------

    case Operation::STA:
        store_operand(info, operand, reg_.a);
        break;

    case Operation::STX:
        store_operand(info, operand, reg_.x);
        break;

    case Operation::STY:
        store_operand(info, operand, reg_.y);
        break;

    // -- compares ------------------------------------------------------------

    case Operation::CMP:
        compare(reg_.a, operand_value(info, operand));
        break;

    case Operation::CPX:
        compare(reg_.x, operand_value(info, operand));
        break;

    case Operation::CPY:
        compare(reg_.y, operand_value(info, operand));
        break;

    // -- increments / decrements in memory -----------------------------------

    case Operation::INC: {
        const u8 value = static_cast<u8>(operand_value(info, operand) + 1u);
        store_operand(info, operand, value);
        reg_.update_nz(value);
        break;
    }

    case Operation::DEC: {
        const u8 value = static_cast<u8>(operand_value(info, operand) - 1u);
        store_operand(info, operand, value);
        reg_.update_nz(value);
        break;
    }

    // -- shifts and rotates --------------------------------------------------
    //
    // The bit pushed out of the byte goes into C; for the rotating versions
    // the old C comes back in at the other end. This is how multi byte
    // shifts and the NES's own scrolling tricks work.

    case Operation::ASL: {   // shift left: bit 7 -> C, 0 -> bit 0
        const u8 value = operand_value(info, operand);
        reg_.set_flag(Flag::Carry, (value & 0x80u) != 0);
        const u8 result = static_cast<u8>(value << 1);
        store_operand(info, operand, result);
        reg_.update_nz(result);
        break;
    }

    case Operation::LSR: {   // shift right: bit 0 -> C, 0 -> bit 7
        const u8 value = operand_value(info, operand);
        reg_.set_flag(Flag::Carry, (value & 0x01u) != 0);
        const u8 result = static_cast<u8>(value >> 1);
        store_operand(info, operand, result);
        reg_.update_nz(result);
        break;
    }

    case Operation::ROL: {   // rotate left through C
        const u8 value = operand_value(info, operand);
        const bool old_carry = reg_.flag(Flag::Carry);
        reg_.set_flag(Flag::Carry, (value & 0x80u) != 0);
        u8 result = static_cast<u8>(value << 1);
        if (old_carry) {
            result = static_cast<u8>(result | 0x01u);
        }
        store_operand(info, operand, result);
        reg_.update_nz(result);
        break;
    }

    case Operation::ROR: {   // rotate right through C
        const u8 value = operand_value(info, operand);
        const bool old_carry = reg_.flag(Flag::Carry);
        reg_.set_flag(Flag::Carry, (value & 0x01u) != 0);
        u8 result = static_cast<u8>(value >> 1);
        if (old_carry) {
            result = static_cast<u8>(result | 0x80u);
        }
        store_operand(info, operand, result);
        reg_.update_nz(result);
        break;
    }

    // -- jumps ---------------------------------------------------------------
    // The effective address IS the destination.

    case Operation::JMP:
        reg_.pc = operand.address;
        break;

    // -- conditional branches ------------------------------------------------
    // Each one tests one flag and jumps to the resolved Target.

    case Operation::BCC: branch(!reg_.flag(Flag::Carry), operand);     break;
    case Operation::BCS: branch(reg_.flag(Flag::Carry), operand);      break;
    case Operation::BEQ: branch(reg_.flag(Flag::Zero), operand);       break;
    case Operation::BNE: branch(!reg_.flag(Flag::Zero), operand);      break;
    case Operation::BMI: branch(reg_.flag(Flag::Negative), operand);   break;
    case Operation::BPL: branch(!reg_.flag(Flag::Negative), operand);  break;
    case Operation::BVS: branch(reg_.flag(Flag::Overflow), operand);   break;
    case Operation::BVC: branch(!reg_.flag(Flag::Overflow), operand);  break;

    // -- logic ---------------------------------------------------------------
    //
    // AND / ORA / EOR are the three bitwise operations the ALU can do.
    // They always write back to A and always set N and Z.
    // See docs/computer-science/bitwise-operations.md.

    case Operation::AND:
        reg_.a = static_cast<u8>(reg_.a & operand_value(info, operand));
        reg_.update_nz(reg_.a);
        break;

    case Operation::ORA:
        reg_.a = static_cast<u8>(reg_.a | operand_value(info, operand));
        reg_.update_nz(reg_.a);
        break;

    case Operation::EOR:
        reg_.a = static_cast<u8>(reg_.a ^ operand_value(info, operand));
        reg_.update_nz(reg_.a);
        break;

    case Operation::BIT: {
        // BIT is the odd one out: it tests A against memory WITHOUT consuming
        // the result. Z says whether (A & M) is zero; N and V are copied
        // straight out of the operand's top two bits.
        //
        // That is what makes it useful for polling hardware: reading a status
        // byte and branching on one bit in a single instruction.
        const u8 value = operand_value(info, operand);
        reg_.set_flag(Flag::Zero, (reg_.a & value) == 0);
        reg_.set_flag(Flag::Negative, (value & 0x80u) != 0);
        reg_.set_flag(Flag::Overflow, (value & 0x40u) != 0);
        // A is untouched, and so is the carry.
        break;
    }

    // -- addition and subtraction --------------------------------------------
    //
    // Both go through the same adder, which is why both produce C and V.
    // The full derivation is in docs/computer-science/overflow-flag.md.

    case Operation::ADC: {
        // A = A + M + C
        const alu::AddResult result = alu::add(
            reg_.a, operand_value(info, operand), reg_.flag(Flag::Carry));

        reg_.a = result.result;
        reg_.set_flag(Flag::Carry, result.carry);       // unsigned overflow
        reg_.set_flag(Flag::Overflow, result.overflow); // signed overflow
        reg_.update_nz(reg_.a);
        break;
    }

    case Operation::SBC: {
        // A = A - M - (1 - C),  which the hardware computes as A + ~M + C.
        //
        // The carry flag is inverted on the way in and means "no borrow" on
        // the way out. That is why SEC precedes a subtraction and CLC before
        // an addition: the flag is the same wire, read two different ways.
        const bool borrow_in = !reg_.flag(Flag::Carry);
        const alu::AddResult result = alu::subtract(
            reg_.a, operand_value(info, operand), borrow_in);

        reg_.a = result.result;
        reg_.set_flag(Flag::Carry, result.carry);       // carry out == no borrow
        reg_.set_flag(Flag::Overflow, result.overflow);
        reg_.update_nz(reg_.a);
        break;
    }

    // -- stack ---------------------------------------------------------------

    case Operation::PHA:
        push(reg_.a);
        break;

    case Operation::PHP:
        push(status_for_push(true));
        break;

    case Operation::PLA:
        reg_.a = pop();
        reg_.update_nz(reg_.a);
        break;

    case Operation::PLP:
        restore_status(pop());
        break;

    // -- subroutines and interrupts ------------------------------------------

    case Operation::JSR: {
        // JSR pushes the address of its own LAST byte, not the next
        // instruction. RTS then adds 1 to get back. The asymmetry is real -
        // it is how the chip is built, and it is why JSR and RTS have
        // different cycle counts for the same trip.
        const u16 return_address = static_cast<u16>(reg_.pc - 1u);
        push_word(return_address);
        reg_.pc = operand.address;
        break;
    }

    case Operation::RTS:
        reg_.pc = static_cast<u16>(pull_word() + 1u);
        break;

    case Operation::RTI:
        // RTI is not RTS: it also restores the status register, and it does
        // NOT add 1 to the return address.
        restore_status(pop());
        reg_.pc = pull_word();
        break;

    case Operation::BRK: {
        // The opcode is 1 byte, but BRK behaves as a 2 byte instruction for
        // the interrupt sequence: it skips the byte after itself. That byte
        // is conventionally a padding value and is never executed.
        reg_.pc = static_cast<u16>(reg_.pc + 1u);
        enter_interrupt(kIrqVector, true);
        // The cycle count comes from the opcode table, not from
        // enter_interrupt(), so nothing is added here.
        break;
    }

    // -- the illegal opcode placeholder --------------------------------------

    case Operation::Unknown:
        halt(last_opcode_);
        break;
    }
}

} // namespace fc
