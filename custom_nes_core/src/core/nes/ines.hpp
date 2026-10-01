#pragma once

// ---------------------------------------------------------------------------
// The iNES file format.
//
// A .nes file is a 16 byte header followed by the ROM data:
//
//     offset 0    "NES" 0x1A         the signature
//     offset 4    PRG ROM pages      each page is 16KB
//     offset 5    CHR ROM pages      each page is 8KB
//     offset 6    flags 6            mapper low nibble, mirroring, battery...
//     offset 7    flags 7            mapper high nibble, NES 2.0 marker
//     offset 8-15 usually zero
//     ------------------------------------------------
//     offset 16   trainer            512 bytes, if flags 6 bit 2 is set
//     then        PRG ROM            the program
//     then        CHR ROM            the pattern tables
//
// PRG ROM is what the CPU executes and reads. CHR ROM is the graphics: it is
// connected to the PPU, not the CPU, and it holds 8x8 pixel tiles.
//
// Two fields are packed into flags 6 and 7 in a way that is worth seeing:
//
//     flags 6:  NNNN BBMM     N = mapper high nibble, B = battery, M = mirror
//     flags 7:  NNNN xxxx     N = mapper low nibble
//
// so the mapper number is
//
//     mapper = (flags6 >> 4) | (flags7 & 0xF0)
//
// which looks odd until you remember that field 7 was added later and had to
// slot into bits nobody was using.
// ---------------------------------------------------------------------------

#include "core/types.hpp"

#include <cstddef>
#include <optional>
#include <span>
#include <string>

namespace fc::nes {

/// How the PPU's two nametables are wired.
///
/// The cartridge decides this, because the cartridge contains the extra RAM
/// (or does not). A 4KB VRAM chip can be wired for horizontal or vertical
/// arrangement; a game with its own extra RAM gets four screens.
enum class Mirroring : u8 {
    Horizontal,
    Vertical,
    FourScreen,
    SingleScreenLower,
    SingleScreenUpper,
};

[[nodiscard]] constexpr const char* mirroring_name(Mirroring mirroring) noexcept
{
    switch (mirroring) {
    case Mirroring::Horizontal:        return "horizontal";
    case Mirroring::Vertical:          return "vertical";
    case Mirroring::FourScreen:        return "four screen";
    case Mirroring::SingleScreenLower: return "single screen, lower";
    case Mirroring::SingleScreenUpper: return "single screen, upper";
    }
    return "?";
}

struct InesHeader {
    u8 prg_rom_pages = 0;   // 16KB units
    u8 chr_rom_pages = 0;   // 8KB units, 0 means CHR RAM

    u8 mapper = 0;

    Mirroring mirroring = Mirroring::Horizontal;

    /// The raw flag bytes, kept so tooling can show them without
    /// reconstructing the packing.
    u8 flags6 = 0;
    u8 flags7 = 0;

    bool has_trainer = false;
    bool has_battery = false;
    bool four_screen = false;
    bool nes2 = false;

    [[nodiscard]] std::size_t prg_rom_size() const noexcept
    {
        return static_cast<std::size_t>(prg_rom_pages) * 16384u;
    }
    [[nodiscard]] std::size_t chr_rom_size() const noexcept
    {
        return static_cast<std::size_t>(chr_rom_pages) * 8192u;
    }
    [[nodiscard]] std::size_t trainer_size() const noexcept
    {
        return has_trainer ? 512u : 0u;
    }
    [[nodiscard]] std::size_t total_size() const noexcept
    {
        return 16u + trainer_size() + prg_rom_size() + chr_rom_size();
    }
};

/// Parsing can fail, and when it does the reason matters - a bad dump, a
/// zip file, or a header-only file are all different problems.
struct InesParseResult {
    std::optional<InesHeader> header;
    std::string error;

    [[nodiscard]] bool ok() const noexcept { return header.has_value(); }
};

/// Parse the 16 byte header. Does not read the ROM data itself.
[[nodiscard]] InesParseResult parse_ines_header(std::span<const u8> rom);

} // namespace fc::nes
