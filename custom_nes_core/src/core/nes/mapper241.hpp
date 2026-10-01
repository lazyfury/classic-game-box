#pragma once

// ---------------------------------------------------------------------------
// Mapper 241.
//
// One 16KB PRG window and 8KB of CHR RAM, and a write anywhere in
// $8000-$FFFF chooses the bank for the *upper* window:
//
//     $8000-$BFFF   fixed to the first 16KB of the ROM
//     $C000-$FFFF   16KB bank, chosen by the write
//
// The whole game's boot code lives in the fixed half and switches the upper
// half out from under itself -- which is why the fixed window has to be the
// first 16KB and not the last: the reset vector itself is read from the
// switchable window, and it points back into the fixed one.
//
// There is no IRQ, no mirroring control and no CHR banking, because there is
// only one 8KB pattern table and it is writable. This is what a late, cheap
// Chinese board looks like: 盟军赶死队 is a 512KB cartridge that uses the
// window to bring in a third of the game at a time.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper241 : public Mapper {
public:
    Mapper241(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
    {
        // A header with no CHR pages means the pattern table is RAM, which is
        // the usual case for this board. 8KB, cleared.
        if (chr_.empty()) {
            chr_.assign(0x2000, 0);
        }
    }

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        if (address < 0xC000u) {
            // The fixed window is the first 16KB, masked rather than checked,
            // so a 16KB cartridge (if one existed) still reads something.
            return prg_[static_cast<std::size_t>(address & 0x3FFFu) % prg_.size()];
        }
        const std::size_t pages = prg_.size() / 0x4000u;
        const std::size_t bank = (pages == 0) ? 0 : prg_bank_ % pages;
        return prg_[bank * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
    }

    /// Any write in the window is the bank select. The address is not part of
    /// the command, which is why it is unnamed here.
    void write_prg(u16 /*address*/, u8 value) override { prg_bank_ = value; }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        return chr_[static_cast<std::size_t>(address) & 0x1FFFu];
    }

    void write_chr(u16 address, u8 value) override
    {
        chr_[static_cast<std::size_t>(address) & 0x1FFFu] = value;
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    void serialize(StateWriter& out) const override
    {
        out.put_u8(prg_bank_);
        out.sized_bytes(chr_);
    }

    bool deserialize(StateReader& in) override
    {
        in.get_u8(prg_bank_);
        in.sized_bytes(chr_);
        return in.ok();
    }

    [[nodiscard]] bool saves_state() const noexcept override { return true; }

private:
    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    u8 prg_bank_ = 0;
};

} // namespace fc::nes
