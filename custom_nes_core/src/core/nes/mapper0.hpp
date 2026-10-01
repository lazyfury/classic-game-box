#pragma once

// ---------------------------------------------------------------------------
// Mapper 0, also called NROM.
//
//     PRG ROM:  16KB or 32KB, no banking at all
//     CHR ROM:  8KB, no banking at all
//
// This is the cartridge with no extra logic on it. There is nothing to
// switch, so writes to the PRG area go nowhere.
//
// The only interesting part is the 16KB case. A 16KB PRG ROM is too small to
// fill $8000-$FFFF, so the cartridge wires it to BOTH halves:
//
//     $8000-$BFFF  ->  ROM[0x0000-0x3FFF]
//     $C000-$FFFF  ->  ROM[0x0000-0x3FFF]   again
//
// That is a third kind of mirroring, and like the other two it comes from
// not spending gates: there is no address line left to distinguish the two
// halves, so the same chip answers both.
//
// NROM is the first target because it is what most launch titles used, and
// because Super Mario Bros is one of them.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper0 : public Mapper {
public:
    Mapper0(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
    {
    }

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t offset = static_cast<std::size_t>(address - 0x8000u);

        // A 16KB ROM mirrors: $C000 reads the same byte as $8000.
        // A 32KB ROM does not. One modulo covers both, because both sizes are
        // powers of two, and it also keeps a malformed size from running off
        // the end of the vector.
        return prg_[offset % prg_.size()];
    }

    void write_prg(u16 /*address*/, u8 /*value*/) override
    {
        // NROM has no bank register, so there is nothing to write to.
        // On real hardware this is simply an open circuit.
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        if (chr_.empty()) {
            return 0;
        }
        return chr_[static_cast<std::size_t>(address) % chr_.size()];
    }

    void write_chr(u16 address, u8 value) override
    {
        // With CHR ROM the writes go nowhere. With CHR RAM (a header that
        // says zero CHR pages) they are stored, and that is how games animate
        // tiles by rewriting the pattern tables.
        if (chr_ram_) {
            chr_[static_cast<std::size_t>(address) % chr_.size()] = value;
        }
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] std::size_t prg_size() const noexcept { return prg_.size(); }
    [[nodiscard]] std::size_t chr_size() const noexcept { return chr_.size(); }

    /// Turn this into CHR RAM. Used when the header says zero CHR pages.
    void make_chr_ram(std::size_t size = 8192)
    {
        chr_.assign(size, 0);
        chr_ram_ = true;
    }

    // -- save states ---------------------------------------------------------
    //
    // NROM has no bank registers, so there is almost nothing to save. The one
    // thing that can change is CHR RAM, and only on a board whose header said
    // zero CHR pages: the writes then go into the vector that would otherwise
    // have held ROM. That vector is the only writable thing on the board.

    void serialize(StateWriter& out) const override
    {
        out.put_flag(chr_ram_);
        if (chr_ram_) {
            out.sized_bytes(chr_);
        }
    }

    bool deserialize(StateReader& in) override
    {
        in.get_flag(chr_ram_);
        if (chr_ram_) {
            in.sized_bytes(chr_);
        }
        return in.ok();
    }

    [[nodiscard]] bool saves_state() const noexcept override { return true; }

private:
    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    bool chr_ram_ = false;
};

} // namespace fc::nes
