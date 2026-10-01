#pragma once

// ---------------------------------------------------------------------------
// Cartridge - a .nes file turned into something the bus can talk to.
//
// It plugs into the bus's cartridge slot ($4020-$FFFF) and splits that range:
//
//     $4020-$5FFF   expansion area          (nothing on NROM carts)
//     $6000-$7FFF   PRG RAM                 (8KB of save/work memory)
//     $8000-$FFFF   PRG ROM, via the mapper
//
// The CHR ROM is not reachable from the CPU at all. It answers on the PPU's
// own bus, which is why read_chr/write_chr are separate functions rather than
// part of Device.
//
// Loading a ROM is a five step job, and each step is a place a real dump can
// go wrong:
//
//   1. parse the 16 byte header
//   2. skip the 512 byte trainer if the header says there is one
//   3. copy PRG ROM
//   4. copy CHR ROM, or allocate CHR RAM when the header says zero pages
//   5. pick a mapper from the header's mapper number
// ---------------------------------------------------------------------------

#include "core/state_fwd.hpp"
#include "core/nes/device.hpp"
#include "core/nes/ines.hpp"
#include "core/nes/mapper.hpp"
#include "core/types.hpp"

#include <cstddef>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <vector>

namespace fc::nes {

class Cartridge : public Device {
public:
    static constexpr u16 kExpansionEnd = 0x5FFF;   // $4020-$5FFF
    static constexpr u16 kPrgRamBase   = 0x6000;   // $6000-$7FFF
    static constexpr u16 kPrgRamEnd    = 0x7FFF;
    static constexpr std::size_t kPrgRamSize = 0x2000;   // 8KB

    /// Build a cartridge from the bytes of a .nes file.
    ///
    /// Returns nothing on failure and fills `error` with the reason. Failures
    /// are normal here - the file might be a zip, a truncated dump, or use a
    /// mapper we have not written yet - so they are values, not exceptions.
    [[nodiscard]] static std::optional<Cartridge> from_bytes(std::span<const u8> rom,
                                                             std::string& error);

    // -- Device, the CPU's side ----------------------------------------------

    [[nodiscard]] u8 read(u16 address) override;
    void write(u16 address, u8 value) override;

    // -- the PPU's side ------------------------------------------------------

    [[nodiscard]] u8 read_chr(u16 address);
    void write_chr(u16 address, u8 value);

    // -- inspection ----------------------------------------------------------

    [[nodiscard]] const InesHeader& header() const noexcept { return header_; }
    [[nodiscard]] Mapper& mapper() noexcept { return *mapper_; }
    [[nodiscard]] const Mapper& mapper() const noexcept { return *mapper_; }

    [[nodiscard]] const std::vector<u8>& prg_rom() const noexcept { return prg_rom_; }
    [[nodiscard]] const std::vector<u8>& chr_rom() const noexcept { return chr_rom_; }
    [[nodiscard]] const std::vector<u8>& trainer() const noexcept { return trainer_; }

    [[nodiscard]] bool has_prg_ram() const noexcept { return prg_ram_enabled_; }
    void set_prg_ram_enabled(bool enabled) noexcept { prg_ram_enabled_ = enabled; }

    /// The 8KB of work RAM at $6000, as the front end sees it.
    ///
    /// A front end persists this as the cartridge's battery save, so the
    /// pointer has to be the bytes the CPU actually reads and writes. On a
    /// board that answers $6000 with its own registers instead of a RAM chip
    /// (mapper 87, mapper 246, VRC2a) the cartridge's copy is unused, which is
    /// what battery_backed() is careful about.
    [[nodiscard]] std::span<u8> prg_ram() noexcept { return prg_ram_; }
    [[nodiscard]] std::span<const u8> prg_ram() const noexcept { return prg_ram_; }

    /// Whether that RAM is the game's save and should outlive the emulator.
    ///
    /// The battery bit lives in the iNES header (flags 6, bit 1). A cartridge
    /// without it has RAM the game clears on power up and nobody misses; one
    /// with it has the save file. The has_work_ram() half keeps the promise
    /// honest: a board that does not answer $6000 with this buffer must not
    /// hand a front end a buffer the game never wrote.
    [[nodiscard]] bool battery_backed() const noexcept
    {
        return header_.has_battery && mapper_ != nullptr && mapper_->has_work_ram();
    }

    // -- Game Genie patches --------------------------------------------------
    //
    // A Game Genie does not modify the ROM. It sits between the cartridge and
    // the console and rewrites the byte on the data bus, which means it acts
    // on the CPU address ($8000-$FFFF), after the mapper has done its banking.
    // That is why the patch lives here and not in the mapper: two different
    // banks of the same address get the patch separately, exactly as on the
    // real hardware.
    //
    // `compare` is the value the cartridge must already be returning for the
    // patch to apply, or -1 to apply unconditionally. It is what an eight
    // letter code carries and a six letter code does not.

    /// One patch applied to PRG ROM reads.
    struct PrgPatch {
        u16 address = 0;
        u8 value = 0;
        int compare = -1;
    };

    void set_prg_patches(std::span<const PrgPatch> patches)
    {
        prg_patches_.assign(patches.begin(), patches.end());
    }

    void clear_prg_patches() noexcept { prg_patches_.clear(); }

    [[nodiscard]] std::span<const PrgPatch> prg_patches() const noexcept
    {
        return prg_patches_;
    }

    /// A one line summary, for tooling.
    [[nodiscard]] std::string summary() const;

private:
    Cartridge() = default;

    InesHeader header_{};
    std::vector<u8> trainer_;
    std::vector<u8> prg_rom_;
    std::vector<u8> chr_rom_;
    std::vector<u8> prg_ram_ = std::vector<u8>(kPrgRamSize, 0);
    std::unique_ptr<Mapper> mapper_;
    bool prg_ram_enabled_ = true;
    std::vector<PrgPatch> prg_patches_;

    friend struct fc::StateAccess;
};

} // namespace fc::nes
