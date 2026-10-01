#include "core/nes/bus.hpp"

namespace fc::nes {

namespace {
// The boundaries of the memory map, in one place so the decoder reads like
// the table in the header.
constexpr u16 kRamEnd        = 0x2000;   // $0000-$1FFF
constexpr u16 kPpuEnd        = 0x4000;   // $2000-$3FFF
constexpr u16 kApuEnd        = 0x4018;   // $4000-$4017
constexpr u16 kDisabledEnd   = 0x4020;   // $4018-$401F
constexpr u16 kOamDma        = 0x4014;
constexpr u16 kController1   = 0x4016;
constexpr u16 kController2   = 0x4017;

/// One halt cycle, then 256 reads and 256 writes.
constexpr int kOamDmaCycles = 513;
} // namespace

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

NesBus::Region NesBus::region_of(u16 address) noexcept
{
    if (address < kRamEnd)      return Region::Ram;
    if (address < kPpuEnd)      return Region::PpuRegisters;
    if (address < kApuEnd)      return Region::ApuAndIo;
    if (address < kDisabledEnd) return Region::Disabled;
    return Region::Cartridge;
}

u8 NesBus::ppu_register_index(u16 address) noexcept
{
    // Only three address lines reach the PPU, so $2000 and $2008 and $2010
    // are all register 0.
    return static_cast<u8>(address & 0x0007);
}

// ---------------------------------------------------------------------------
// Read
// ---------------------------------------------------------------------------

u8 NesBus::read(u16 address)
{
    const u8 value = decode_read(address);

    // Whatever answered drove the data bus, so that is what an unmapped read
    // would see next.
    open_bus_ = value;
    return value;
}

u8 NesBus::peek(u16 address) const
{
    switch (region_of(address)) {
    case Region::Ram:
        return ram_.read(address);
    case Region::Cartridge:
        // Cartridge RAM, not cartridge ROM. Reading a ROM byte through the
        // mapper can bank switch on some boards, and a peek is a question,
        // not a bus cycle.
        if (address >= 0x6000 && cartridge_ != nullptr) {
            return cartridge_->read(address);
        }
        return 0;
    default:
        // A PPU register read has side effects; measuring must not change
        // what is being measured.
        return 0;
    }
}

u8 NesBus::decode_read(u16 address)
{
    switch (region_of(address)) {
    case Region::Ram:
        // The chip sees only 11 address lines, so the wrap happens in Ram.
        return ram_.read(address);

    case Region::PpuRegisters:
        // Full PPU register reads have side effects ($2002 clears vblank,
        // $2007 advances VRAM), which is why read() is not const anywhere
        // in this project.
        if (ppu_ != nullptr) {
            return ppu_->read(address);
        }
        return open_bus_;

    case Region::ApuAndIo:
        if (address == kController1) {
            return read_controller(0);
        }
        if (address == kController2) {
            return read_controller(1);
        }
        if (apu_ != nullptr) {
            return apu_->read(address);
        }
        return open_bus_;

    case Region::Disabled:
        return open_bus_;

    case Region::Cartridge:
        if (cartridge_ != nullptr) {
            return cartridge_->read(address);
        }
        return open_bus_;
    }
    return open_bus_;
}

// ---------------------------------------------------------------------------
// Write
// ---------------------------------------------------------------------------

void NesBus::write(u16 address, u8 value)
{
    // The value is on the data bus even if nothing listens, so open bus
    // tracks writes too.
    open_bus_ = value;

    switch (region_of(address)) {
    case Region::Ram:
        ram_.write(address, value);
        return;

    case Region::PpuRegisters:
        if (ppu_ != nullptr) {
            ppu_->write(address, value);
        }
        return;

    case Region::ApuAndIo:
        // $4014 is not the APU: it is the DMA controller. The bus owns it.
        if (address == kOamDma) {
            start_oam_dma(value);
            return;
        }
        // $4016's write is the strobe, and it reaches BOTH ports: they share
        // one wire. It does not go to the APU.
        if (address == kController1) {
            const bool high = (value & 0x01u) != 0;
            controllers_[0].strobe(high);
            controllers_[1].strobe(high);
            return;
        }
        // $4017's write IS the APU's frame counter, so it falls through.
        if (apu_ != nullptr) {
            apu_->write(address, value);
        }
        return;

    case Region::Disabled:
        // Writes here go nowhere on real hardware too.
        return;

    case Region::Cartridge:
        if (cartridge_ != nullptr) {
            cartridge_->write(address, value);
        }
        return;
    }
}

// ---------------------------------------------------------------------------
// Controllers
// ---------------------------------------------------------------------------

u8 NesBus::read_controller(int index) noexcept
{
    // Only bit 0 is wired from the controller port. The other seven bits are
    // whatever was last on the data bus, which is why a program must mask.
    const u8 bit = controllers_[static_cast<std::size_t>(index) & 1u].read();
    return static_cast<u8>((open_bus_ & 0xFEu) | (bit & 0x01u));
}

// ---------------------------------------------------------------------------
// OAM DMA
// ---------------------------------------------------------------------------

void NesBus::start_oam_dma(u8 page)
{
    ++oam_dma_count_;

    // `page` selects a 256 byte page of CPU memory: $00, $01, ... $FF.
    const u16 base = static_cast<u16>(static_cast<u16>(page) << 8);

    if (oam_target_ != nullptr) {
        for (u16 i = 0; i < 256; ++i) {
            // Note that this goes through the normal read path, so a DMA
            // sourced from $2000-$3FFF would really hit the PPU registers.
            // That is what the hardware does, and it is why games always DMA
            // from page $02.
            oam_target_->write_oam(static_cast<u8>(i),
                                   read(static_cast<u16>(base + i)));
        }
    }

    // The CPU is halted for one cycle, then 256 reads and 256 writes.
    // (On real hardware it is 514 when the write lands on an odd cycle. That
    // one cycle difference is not modelled yet.)
    pending_stalls_ += kOamDmaCycles;
}

int NesBus::take_stall_cycles()
{
    const int stalls = pending_stalls_;
    pending_stalls_ = 0;
    return stalls;
}

} // namespace fc::nes
