#include "core/nes/ines.hpp"

namespace fc::nes {

InesParseResult parse_ines_header(std::span<const u8> rom)
{
    InesParseResult result;

    if (rom.size() < 16) {
        result.error = "file is shorter than the 16 byte iNES header";
        return result;
    }

    if (rom[0] != 'N' || rom[1] != 'E' || rom[2] != 'S' || rom[3] != 0x1A) {
        result.error = "missing the \"NES\\x1A\" signature";
        return result;
    }

    InesHeader header{};
    header.prg_rom_pages = rom[4];
    header.chr_rom_pages = rom[5];

    const u8 flags6 = rom[6];
    const u8 flags7 = rom[7];
    header.flags6 = flags6;
    header.flags7 = flags7;

    // bit 3 of flags 6 says "four screen", and it overrides bit 0.
    if ((flags6 & 0x08) != 0) {
        header.mirroring = Mirroring::FourScreen;
    } else if ((flags6 & 0x01) != 0) {
        header.mirroring = Mirroring::Vertical;
    } else {
        header.mirroring = Mirroring::Horizontal;
    }

    header.four_screen = (flags6 & 0x08) != 0;
    header.has_trainer = (flags6 & 0x04) != 0;
    header.has_battery = (flags6 & 0x02) != 0;

    // The mapper number is split across two bytes, four bits each.
    header.mapper = static_cast<u8>((flags6 >> 4) | (flags7 & 0xF0u));

    // NES 2.0 marks itself with bits 3..2 of flags 7 equal to binary 10.
    header.nes2 = (flags7 & 0x0Cu) == 0x08u;

    if (header.prg_rom_pages == 0) {
        result.error = "the header claims zero PRG ROM pages";
        return result;
    }

    if (rom.size() < header.total_size()) {
        result.error = "the file is shorter than the header claims: expected " +
                       std::to_string(header.total_size()) + " bytes, found " +
                       std::to_string(rom.size());
        return result;
    }

    result.header = header;
    return result;
}

} // namespace fc::nes
