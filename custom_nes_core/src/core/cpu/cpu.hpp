#pragma once

// ---------------------------------------------------------------------------
// The CPU.
//
// Everything a CPU does is this loop:
//
//      fetch    read the byte at PC, then PC = PC + 1
//        |
//      decode   look the opcode up in the table -> (operation, addressing mode)
//        |
//      address  fetch the operand bytes, compute the EFFECTIVE ADDRESS
//        |
//      execute  do the work
//        |
//      update state (flags, registers, cycles)
//        |
//      repeat
//
// Phase 0.4 splits "decode" from "address". The opcode table already knows
// each instruction's addressing mode, so one handler per OPERATION now covers
// every addressing mode that operation supports. Before this, every
// (operation, mode) pair needed its own case.
// ---------------------------------------------------------------------------

#include "core/state_fwd.hpp"
#include "core/bus.hpp"
#include "core/cpu/addressing.hpp"
#include "core/cpu/opcode.hpp"
#include "core/cpu/registers.hpp"
#include "core/types.hpp"

namespace fc {

class Cpu {
public:
    explicit Cpu(Bus& bus) noexcept
        : bus_(&bus)
    {
    }

    /// Power-on / reset.
    ///
    /// The 6502 does not start executing at address 0. It reads a 16 bit
    /// address from the reset vector at $FFFC-$FFFD and jumps there.
    /// Those two bytes are stored little endian: low byte first.
    void reset() noexcept;

    /// Execute exactly one instruction. Returns the cycles it took.
    int step() noexcept;

    /// Run until halted or `max_steps` instructions have run.
    int run(int max_steps) noexcept;

    // -- state access --------------------------------------------------------

    [[nodiscard]] const Registers& registers() const noexcept { return reg_; }
    [[nodiscard]] Registers& registers() noexcept { return reg_; }

    /// Cycles accumulated so far. Cycle accurate since Phase 1: the datasheet
    /// table plus page-crossing and taken-branch penalties.
    [[nodiscard]] u64 total_cycles() const noexcept { return cycles_; }

    [[nodiscard]] u16 current_instruction_pc() const noexcept { return instruction_pc_; }
    [[nodiscard]] u8 last_opcode() const noexcept { return last_opcode_; }

    // -- failure reporting ---------------------------------------------------
    //
    // An unimplemented opcode is a bug in the emulator, not in the game.
    // We stop loudly instead of silently doing nothing.

    [[nodiscard]] bool is_halted() const noexcept { return halted_; }
    [[nodiscard]] u8 unimplemented_opcode() const noexcept { return unimplemented_opcode_; }

    /// True when this opcode is legal AND its operation is implemented.
    ///
    /// After Phase 1 every legal operation is implemented, so this is now
    /// just "is the opcode defined at all".
    [[nodiscard]] static bool implements(u8 opcode) noexcept;

    /// True when the execute() switch knows this operation.
    [[nodiscard]] static bool handles(Operation op) noexcept;

    // -- interrupts ----------------------------------------------------------
    //
    // The 6502 has three interrupt vectors:
    //
    //   $FFFA  NMI   non maskable, edge triggered by the PPU once per frame
    //   $FFFC  RESET power on
    //   $FFFE  IRQ   maskable by the I flag; BRK shares it
    //
    // The NES has no external IRQ source wired up, so IRQ is mostly a
    // debugging feature - but BRK uses the same vector and does matter.

    /// Latch an NMI. It will be serviced before the next instruction.
    void request_nmi() noexcept { nmi_pending_ = true; }
    [[nodiscard]] bool nmi_pending() const noexcept { return nmi_pending_; }

    /// The IRQ line is level triggered: hold it high and it fires every time
    /// I is clear.
    void set_irq_line(bool asserted) noexcept { irq_line_ = asserted; }
    [[nodiscard]] bool irq_line() const noexcept { return irq_line_; }

    /// True when an interrupt is waiting and would be taken right now.
    [[nodiscard]] bool interrupt_pending() const noexcept;

    /// Force the CPU to service an interrupt immediately, for tests and for
    /// the debugger.
    void service_interrupt(bool is_break) noexcept;   // the $FFFE vector
    void service_nmi() noexcept;                      // the $FFFA vector

    // -- stack ---------------------------------------------------------------
    //
    // The 6502 stack lives only in page 1 ($0100-$01FF). SP is the low byte,
    // so the stack is 256 bytes and grows DOWNWARD: push decrements SP.

    void push(u8 value) noexcept;
    [[nodiscard]] u8 pop() noexcept;

    void push_word(u16 value) noexcept;   // high byte first, as the 6502 does
    [[nodiscard]] u16 pull_word() noexcept;

    /// The byte that PHP / BRK / IRQ actually push.
    ///
    /// Bit 5 always reads as 1. Bit 4 is not a real flag - it only exists in
    /// the pushed copy, and is set for PHP and BRK but clear for a hardware
    /// interrupt, so software can tell them apart.
    [[nodiscard]] u8 status_for_push(bool break_flag) const noexcept;

    /// Load P from a value that came off the stack.
    void restore_status(u8 value) noexcept;

private:
    // -- bus helpers ---------------------------------------------------------
    [[nodiscard]] u8 read(u16 address) noexcept { return bus_->read(address); }
    void write(u16 address, u8 value) noexcept { bus_->write(address, value); }

    // -- fetch ---------------------------------------------------------------
    [[nodiscard]] u8 fetch_byte() noexcept;

    // -- operands ------------------------------------------------------------

    /// The current value the operation should work on.
    [[nodiscard]] u8 operand_value(const OpcodeInfo& info, const Operand& operand) noexcept;

    /// Store a result back. Handles the accumulator mode transparently.
    void store_operand(const OpcodeInfo& info, const Operand& operand, u8 value) noexcept;

    // -- decode + execute ----------------------------------------------------
    void execute(const OpcodeInfo& info, const Operand& operand) noexcept;
    void halt(u8 opcode) noexcept;

    /// Datasheet cycle count plus the data dependent penalties.
    [[nodiscard]] int cycle_cost(u8 opcode, const Operand& operand) const noexcept;

    /// The common part of NMI / IRQ / BRK: push the return address and the
    /// status, set I, and jump through `vector`.
    void enter_interrupt(u16 vector, bool is_break) noexcept;

    // -- small operation helpers --------------------------------------------
    void compare(u8 left, u8 right) noexcept;
    void branch(bool condition, const Operand& operand) noexcept;

    Bus* bus_;
    Registers reg_{};
    u64 cycles_ = 0;

    u16 instruction_pc_ = 0;
    u8  last_opcode_ = 0;

    int branch_extra_cycles_ = 0;

    bool nmi_pending_ = false;
    bool irq_line_ = false;

    bool halted_ = false;
    u8   unimplemented_opcode_ = 0;

    // Everything above is the machine's state. See src/core/state.cpp, which
    // reads and writes all of it and is the one place that has to be kept in
    // step with this list.
    friend struct fc::StateAccess;
};

} // namespace fc
