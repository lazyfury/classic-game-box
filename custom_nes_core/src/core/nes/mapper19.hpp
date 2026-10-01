#pragma once

// ---------------------------------------------------------------------------
// Mapper 19, the Namco 163 (also sold as Namcot 163).
//
//     PRG:  4 x 8KB windows; $8000/$A000/$C000 switchable, $E000 fixed last
//     CHR:  8 x 1KB windows
//     $4800-$4FFF  audio RAM data port (128 bytes, auto-increment)
//     $5000-$57FF  IRQ counter, low byte
//     $5800-$5FFF  IRQ counter, high byte (bit 7 enables it)
//     $E000  PRG bank 0, and bit 6 mutes the sound
//     $E800  PRG bank 1
//     $F000  PRG bank 2
//     $F800  audio RAM address port (bit 7 = auto-increment)
//     Mirroring: hard wired on the board
//
// The interesting part is that $4800-$5FFF is not a set of registers but a
// *window into the sound chip's own 128 bytes of RAM*. To change a channel
// the game writes the byte address to $F800 and then reads or writes $4800,
// which can auto-increment. That is why the registers look like memory: they
// are.
//
// The IRQ is another CPU-clocked counter, like the VRC4 and SS88006, but with
// a difference worth noticing: it counts *up*, bit 15 is the enable, and the
// interrupt comes when the low 15 bits roll over to $7FFF rather than to
// zero. Games use it as a timebase, not a scanline counter.
//
// The wavetable synthesis itself is not emulated yet; the RAM is kept so the
// game's writes and reads behave, but nothing is mixed into the audio output.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper19 : public Mapper {
public:
    Mapper19(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
    {
    }

    void make_chr_ram(std::size_t size = 8192)
    {
        chr_.assign(size, 0);
        chr_ram_ = true;
    }

    // -- the CPU's view, $8000-$FFFF -----------------------------------------

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t banks = prg_bank_count();
        const std::size_t slot = static_cast<std::size_t>(address >> 13) & 0x03u;
        const std::size_t bank = (slot < 3u)
                                     ? (static_cast<std::size_t>(prg_bank_[slot]) % banks)
                                     : (banks - 1u);
        return prg_[bank * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        switch (address & 0xF800u) {
        case 0x8000: case 0x8800: case 0x9000: case 0x9800:
            chr_bank_[(address - 0x8000u) >> 11] = value;
            break;

        case 0xA000: case 0xA800: case 0xB000: case 0xB800:
            chr_bank_[4u + ((address - 0xA000u) >> 11)] = value;
            break;

        case 0xC000: case 0xC800: case 0xD000: case 0xD800:
            // CHR-ROM nametables. Accepted and remembered, but the PPU still
            // reads its own VRAM: this feature is not emulated.
            nametable_bank_[(address - 0xC000u) >> 11] = value;
            break;

        case 0xE000:
            prg_bank_[0] = static_cast<u8>(value & 0x3Fu);
            audio_muted_ = (value & 0x40u) != 0;
            break;

        case 0xE800:
            prg_bank_[1] = static_cast<u8>(value & 0x3Fu);
            break;

        case 0xF000:
            prg_bank_[2] = static_cast<u8>(value & 0x3Fu);
            break;

        default:   // 0xF800
            audio_address_ = static_cast<u8>(value & 0x7Fu);
            audio_auto_increment_ = (value & 0x80u) != 0;
            break;
        }
    }

    // -- the PPU's view, $0000-$1FFF -----------------------------------------

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        return chr_[chr_offset(address)];
    }

    void write_chr(u16 address, u8 value) override
    {
        if (chr_ram_) {
            chr_[chr_offset(address)] = value;
        }
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    // -- the expansion area: the sound RAM and the IRQ ------------------------

    [[nodiscard]] u8 read_expansion(u16 address) override
    {
        switch (address & 0xF800u) {
        case 0x4800: {
            const u8 value = audio_ram_[audio_address_];
            advance_audio_address();
            return value;
        }
        case 0x5000:
            return static_cast<u8>(irq_ & 0x00FFu);
        case 0x5800:
            return static_cast<u8>((irq_ >> 8) & 0x00FFu);
        default:
            return 0;
        }
    }

    void write_expansion(u16 address, u8 value) override
    {
        switch (address & 0xF800u) {
        case 0x4800:
            audio_ram_[audio_address_] = value;
            advance_audio_address();
            break;

        case 0x5000:
            irq_ = static_cast<u16>((irq_ & 0xFF00u) | value);
            irq_pending_ = false;
            break;

        case 0x5800:
            irq_ = static_cast<u16>((irq_ & 0x00FFu) |
                                    (static_cast<u16>(value) << 8));
            irq_pending_ = false;
            break;

        default:
            break;
        }
    }

    // -- the CPU-clocked IRQ counter -----------------------------------------

    [[nodiscard]] bool clocks_on_cpu_cycles() const noexcept override { return true; }

    void on_cpu_cycle() noexcept override
    {
        // Counts up, only while enabled, and stops on $7FFF (the IRQ).
        if ((irq_ & 0x8000u) != 0 && (irq_ & 0x7FFFu) != 0x7FFFu) {
            ++irq_;
            if ((irq_ & 0x7FFFu) == 0x7FFFu) {
                irq_pending_ = true;
            }
        }
    }

    [[nodiscard]] bool irq_asserted() const noexcept override { return irq_pending_; }

    // -- inspection, for tests ----------------------------------------------

    [[nodiscard]] u16 irq_counter() const noexcept { return irq_; }
    [[nodiscard]] u8 chr_bank(int slot) const noexcept
    {
        return (slot >= 0 && slot < 8) ? chr_bank_[slot] : 0;
    }
    [[nodiscard]] u8 audio_address() const noexcept { return audio_address_; }
    [[nodiscard]] u8 audio_ram(int index) const noexcept
    {
        return (index >= 0 && index < 0x80) ? audio_ram_[index] : 0;
    }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x2000u;
        return (count == 0) ? 1 : count;
    }

    [[nodiscard]] std::size_t chr_offset(u16 address) const noexcept
    {
        if (chr_.empty()) {
            return 0;
        }
        const std::size_t banks = chr_.size() / 0x400u;
        const std::size_t slot = static_cast<std::size_t>(address >> 10) & 0x07u;
        const std::size_t bank =
            static_cast<std::size_t>(chr_bank_[slot]) % ((banks == 0) ? 1 : banks);
        return bank * 0x400u + static_cast<std::size_t>(address & 0x3FFu);
    }

    void advance_audio_address() noexcept
    {
        if (audio_auto_increment_) {
            audio_address_ = static_cast<u8>((audio_address_ + 1u) & 0x7Fu);
        }
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    bool chr_ram_ = false;

    u8 prg_bank_[3] = { 0, 0, 0 };
    u8 chr_bank_[8] = { 0, 0, 0, 0, 0, 0, 0, 0 };
    u8 nametable_bank_[4] = { 0, 0, 0, 0 };

    u8 audio_ram_[0x80] = {};
    u8 audio_address_ = 0;
    bool audio_auto_increment_ = false;
    bool audio_muted_ = false;

    u16 irq_ = 0;
    bool irq_pending_ = false;

    // -- save states ---------------------------------------------------------
    //
    // This board has the most state of any mapper here, and one piece of it is
    // not a bank register at all: `audio_ram_` is 128 bytes of wavetable the
    // game writes for the Namco 163's own sound chip. It is memory, not
    // wiring, and the game reads it back through $4800 -- so leaving it out
    // would give back a working machine with the wrong instrument loaded.
    //
    // The expansion audio itself is not mixed yet, which does not change what
    // has to be saved: the RAM is the game's, and the game can tell whether it
    // came back.

public:
    void serialize(StateWriter& out) const override
    {
        for (const u8 bank : prg_bank_) {
            out.put_u8(bank);
        }
        for (const u8 bank : chr_bank_) {
            out.put_u8(bank);
        }
        for (const u8 bank : nametable_bank_) {
            out.put_u8(bank);
        }

        out.raw(audio_ram_, sizeof(audio_ram_));
        out.put_u8(audio_address_);
        out.put_flag(audio_auto_increment_);
        out.put_flag(audio_muted_);

        out.put_u16(irq_);
        out.put_flag(irq_pending_);
        out.put_u8(static_cast<u8>(mirroring_));
        out.put_flag(chr_ram_);
        if (chr_ram_) {
            out.sized_bytes(chr_);
        }
    }

    bool deserialize(StateReader& in) override
    {
        for (auto& bank : prg_bank_) {
            in.get_u8(bank);
        }
        for (auto& bank : chr_bank_) {
            in.get_u8(bank);
        }
        for (auto& bank : nametable_bank_) {
            in.get_u8(bank);
        }

        in.raw(audio_ram_, sizeof(audio_ram_));
        in.get_u8(audio_address_);
        in.get_flag(audio_auto_increment_);
        in.get_flag(audio_muted_);

        in.get_u16(irq_);
        in.get_flag(irq_pending_);

        u8 mirroring = 0;
        in.get_u8(mirroring);
        mirroring_ = static_cast<Mirroring>(mirroring);

        in.get_flag(chr_ram_);
        if (chr_ram_) {
            in.sized_bytes(chr_);
        }
        return in.ok();
    }

    [[nodiscard]] bool saves_state() const noexcept override { return true; }

private:
};

} // namespace fc::nes
