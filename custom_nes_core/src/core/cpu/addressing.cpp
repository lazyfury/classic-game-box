#include "core/cpu/addressing.hpp"

#include "core/bit.hpp"

namespace fc {
namespace {

/// absolute + index, with the wrap at 64K that a 16 bit address bus gives you.
[[nodiscard]] Operand indexed_absolute(u16 base, u8 index) noexcept
{
    const u16 address = static_cast<u16>(base + index);

    Operand out{};
    out.kind = OperandKind::Address;
    out.address = address;
    out.page_crossed = bit::hi_byte(base) != bit::hi_byte(address);
    return out;
}

[[nodiscard]] Operand address_operand(u16 address) noexcept
{
    Operand out{};
    out.kind = OperandKind::Address;
    out.address = address;
    return out;
}

} // namespace

// ---------------------------------------------------------------------------
// Pointer reads
// ---------------------------------------------------------------------------

u16 read_pointer_zero_page(Bus& bus, u8 address) noexcept
{
    const u8 low = bus.read(address);

    // The high byte stays inside page 0. bit::lo_byte() of an 8 bit value is
    // the value itself, so the addition simply wraps within the byte.
    const u8 next = static_cast<u8>(address + 1u);
    const u8 high = bus.read(next);

    return bit::make_u16(low, high);
}

u16 read_pointer_indirect(Bus& bus, u16 address) noexcept
{
    const u8 low = bus.read(address);

    // 6502 bug: the high byte is read from the same 256 byte page as the low
    // byte. $10FF -> low from $10FF, high from $1000.
    //
    //   page of address      = address & 0xFF00
    //   low byte of address  = address & 0x00FF
    //   next byte in page    = (low byte + 1) & 0xFF
    const u16 high_address = static_cast<u16>((address & 0xFF00) |
                                              static_cast<u16>((address + 1u) & 0x00FF));
    const u8 high = bus.read(high_address);

    return bit::make_u16(low, high);
}

// ---------------------------------------------------------------------------
// resolve
// ---------------------------------------------------------------------------

Operand resolve(const AddressingRequest& request, Bus& bus) noexcept
{
    const u16 absolute = bit::make_u16(request.operand_lo, request.operand_hi);

    switch (request.mode) {

    // -- no operand at all ---------------------------------------------------
    case AddressingMode::Implied:
    case AddressingMode::Accumulator:
        return Operand{ OperandKind::None, 0, 0, false };

    // -- the operand is the data ---------------------------------------------
    case AddressingMode::Immediate: {
        Operand out{};
        out.kind = OperandKind::Value;
        out.value = request.operand_lo;
        return out;
    }

    // -- zero page: the high byte is hard wired to zero ----------------------
    case AddressingMode::ZeroPage:
        return address_operand(request.operand_lo);

    case AddressingMode::ZeroPageX:
        // The addition wraps INSIDE page 0. The cast to u8 throws away the
        // carry, which is exactly what the hardware does.
        return address_operand(static_cast<u8>(request.operand_lo + request.x));

    case AddressingMode::ZeroPageY:
        return address_operand(static_cast<u8>(request.operand_lo + request.y));

    // -- absolute ------------------------------------------------------------
    case AddressingMode::Absolute:
        return address_operand(absolute);

    case AddressingMode::AbsoluteX:
        return indexed_absolute(absolute, request.x);

    case AddressingMode::AbsoluteY:
        return indexed_absolute(absolute, request.y);

    // -- indirect ------------------------------------------------------------
    case AddressingMode::Indirect:
        return address_operand(read_pointer_indirect(bus, absolute));

    case AddressingMode::IndirectX: {
        // Index the POINTER first, then dereference.
        // The pointer lives in page 0 and wraps there.
        const u8 pointer = static_cast<u8>(request.operand_lo + request.x);
        return address_operand(read_pointer_zero_page(bus, pointer));
    }

    case AddressingMode::IndirectY: {
        // Dereference FIRST, then index the result.
        const u16 base = read_pointer_zero_page(bus, request.operand_lo);
        return indexed_absolute(base, request.y);
    }

    // -- relative: a signed offset from the next instruction -----------------
    case AddressingMode::Relative: {
        // Branch instructions are always 2 bytes, so the base is pc + 2.
        const int offset = static_cast<int>(bit::as_signed(request.operand_lo));
        const int next_instruction = static_cast<int>(request.instruction_pc) + 2;

        Operand out{};
        out.kind = OperandKind::Target;
        out.address = static_cast<u16>(next_instruction + offset);
        return out;
    }

    case AddressingMode::Unknown:
        return Operand{};
    }

    return Operand{};
}

} // namespace fc
