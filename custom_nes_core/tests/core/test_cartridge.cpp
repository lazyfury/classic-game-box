#include "core/cpu/cpu.hpp"
#include "core/cpu/disassembler.hpp"
#include "core/nes/bus.hpp"
#include "core/nes/cartridge.hpp"
#include "core/nes/ines.hpp"
#include "core/nes/mapper0.hpp"
#include "core/nes/mapper1.hpp"
#include "core/nes/mapper4.hpp"
#include "core/nes/mapper163.hpp"
#include "core/nes/mapper226.hpp"
#include "core/nes/ppu.hpp"
#include "core/nes/ram_cartridge.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <vector>

using namespace fc;

namespace {

/// Build a synthetic iNES image: header + PRG + CHR.
/// PRG byte i is (i & 0xFF), CHR byte i is ((i * 3) & 0xFF), so every byte is
/// predictable and a copy that is off by one is immediately visible.
std::vector<u8> make_ines(u8 prg_pages, u8 chr_pages, u8 flags6 = 0, u8 flags7 = 0,
                          std::span<const u8> trainer = {})
{
    std::vector<u8> rom;
    rom.reserve(16 + trainer.size() + std::size_t(prg_pages) * 16384u
                + std::size_t(chr_pages) * 8192u);

    rom.push_back('N');
    rom.push_back('E');
    rom.push_back('S');
    rom.push_back(0x1A);
    rom.push_back(prg_pages);
    rom.push_back(chr_pages);
    rom.push_back(flags6);
    rom.push_back(flags7);
    for (int i = 0; i < 8; ++i) {
        rom.push_back(0);
    }

    rom.insert(rom.end(), trainer.begin(), trainer.end());

    for (std::size_t i = 0; i < std::size_t(prg_pages) * 16384u; ++i) {
        rom.push_back(static_cast<u8>(i & 0xFFu));
    }
    for (std::size_t i = 0; i < std::size_t(chr_pages) * 8192u; ++i) {
        rom.push_back(static_cast<u8>((i * 3u) & 0xFFu));
    }
    return rom;
}

/// A 32KB PRG ROM holding `program` at `origin`, with the vectors pointed at it.
std::vector<u8> make_program_rom(std::span<const u8> program, u16 origin = 0x8000)
{
    std::vector<u8> rom = make_ines(2, 1);

    for (std::size_t i = 0; i < program.size(); ++i) {
        rom[16 + (origin - 0x8000) + i] = program[i];
    }
    rom[16 + 0x7FFC] = static_cast<u8>(origin & 0x00FF);
    rom[16 + 0x7FFD] = static_cast<u8>(origin >> 8);
    return rom;
}

std::optional<nes::Cartridge> load(std::span<const u8> rom, std::string& error)
{
    return nes::Cartridge::from_bytes(rom, error);
}

/// An iNES image whose banks are numbered so a test can tell them apart.
/// Every byte of 16KB PRG bank N is N, and every byte of 4KB CHR bank N is N.
std::vector<u8> make_banked_ines(u8 prg_pages, u8 chr_pages, u8 flags6)
{
    std::vector<u8> rom = make_ines(prg_pages, chr_pages, flags6);

    for (std::size_t i = 0; i < std::size_t(prg_pages) * 16384u; ++i) {
        rom[16 + i] = static_cast<u8>(i / 0x4000u);
    }
    const std::size_t chr_base = 16 + std::size_t(prg_pages) * 16384u;
    for (std::size_t i = 0; i < std::size_t(chr_pages) * 8192u; ++i) {
        rom[chr_base + i] = static_cast<u8>(i / 0x1000u);
    }
    return rom;
}

/// Numbers every 16KB PRG bank and every 4KB CHR bank, and sets both iNES
/// mapper nibbles so the 100+ mappers can be built the same way.
std::vector<u8> make_banked_ines_with(u8 prg_pages, u8 chr_pages, u8 flags6, u8 flags7)
{
    std::vector<u8> rom = make_ines(prg_pages, chr_pages, flags6, flags7);

    for (std::size_t i = 0; i < std::size_t(prg_pages) * 16384u; ++i) {
        rom[16 + i] = static_cast<u8>(i / 0x4000u);
    }
    const std::size_t chr_base = 16 + std::size_t(prg_pages) * 16384u;
    for (std::size_t i = 0; i < std::size_t(chr_pages) * 8192u; ++i) {
        rom[chr_base + i] = static_cast<u8>(i / 0x1000u);
    }
    return rom;
}

/// The Nanjing board: mapper 163, 32KB PRG banks, CHR RAM.
std::vector<u8> make_nanjing_ines(u8 prg_pages)
{
    return make_banked_ines_with(prg_pages, 0, 0x30, 0xA0);
}

/// Build an image for any mapper number: the two iNES nibbles pack it.
std::vector<u8> make_mapper_ines(u8 prg_pages, u8 chr_pages, int mapper,
                                 bool vertical = false)
{
    const u8 flags6 = static_cast<u8>(((mapper & 0x0F) << 4) | (vertical ? 0x01 : 0x00));
    const u8 flags7 = static_cast<u8>(mapper & 0xF0);
    return make_banked_ines_with(prg_pages, chr_pages, flags6, flags7);
}

/// Same, but numbered at 8KB granularity, for the mappers whose PRG windows
/// are that small so a test can name every window.
std::vector<u8> make_mapper_ines_8k(u8 prg_pages, u8 chr_pages, int mapper)
{
    std::vector<u8> rom = make_mapper_ines(prg_pages, chr_pages, mapper);
    for (std::size_t i = 0; i < std::size_t(prg_pages) * 16384u; ++i) {
        rom[16 + i] = static_cast<u8>(i / 0x2000u);
    }
    return rom;
}

/// Numbered at 8KB for PRG and 1KB for CHR, for the MMC3-shaped boards
/// (mapper 19, 249) whose CHR registers count 1KB pages.
std::vector<u8> make_8k_1k_ines(u8 prg_pages, u8 chr_pages, int mapper)
{
    std::vector<u8> rom = make_mapper_ines(prg_pages, chr_pages, mapper);
    for (std::size_t i = 0; i < std::size_t(prg_pages) * 16384u; ++i) {
        rom[16 + i] = static_cast<u8>(i / 0x2000u);
    }
    const std::size_t chr_base = 16 + std::size_t(prg_pages) * 16384u;
    for (std::size_t i = 0; i < std::size_t(chr_pages) * 8192u; ++i) {
        rom[chr_base + i] = static_cast<u8>(i / 0x400u);
    }
    return rom;
}

/// Reach a loaded cartridge's mapper as the concrete Nanjing chip, so a test
/// can look at state the Mapper interface does not expose.
nes::Mapper163& nanjing(nes::Cartridge& cart)
{
    return static_cast<nes::Mapper163&>(cart.mapper());
}

/// Write a five bit value through the MMC1's serial port, LSB first.
void mmc1_write(nes::Cartridge& cart, u16 address, u8 value)
{
    for (int bit = 0; bit < 5; ++bit) {
        cart.write(address, static_cast<u8>((value >> bit) & 1u));
    }
}

/// An iNES image numbered at MMC3's granularity: 8KB PRG banks and 1KB CHR
/// banks, so a test can tell every bank from every other one.
std::vector<u8> make_mmc3_ines(u8 prg_pages, u8 chr_pages)
{
    std::vector<u8> rom = make_ines(prg_pages, chr_pages, 0x40);

    for (std::size_t i = 0; i < std::size_t(prg_pages) * 16384u; ++i) {
        rom[16 + i] = static_cast<u8>(i / 0x2000u);
    }
    const std::size_t chr_base = 16 + std::size_t(prg_pages) * 16384u;
    for (std::size_t i = 0; i < std::size_t(chr_pages) * 8192u; ++i) {
        rom[chr_base + i] = static_cast<u8>(i / 0x400u);
    }
    return rom;
}

/// One MMC3 scanline: A12 drops then rises, which is what clocks the counter.
void mmc3_scanline(nes::Cartridge& cart)
{
    cart.mapper().on_ppu_address(0x0000);
    cart.mapper().on_ppu_address(0x1000);
}

/// An iNES image numbered at 8KB PRG / 4KB CHR granularity, for MMC2/MMC4.
std::vector<u8> make_8k_prg_ines(u8 prg_pages, u8 chr_pages, u8 flags6)
{
    std::vector<u8> rom = make_ines(prg_pages, chr_pages, flags6);

    for (std::size_t i = 0; i < std::size_t(prg_pages) * 16384u; ++i) {
        rom[16 + i] = static_cast<u8>(i / 0x2000u);
    }
    const std::size_t chr_base = 16 + std::size_t(prg_pages) * 16384u;
    for (std::size_t i = 0; i < std::size_t(chr_pages) * 8192u; ++i) {
        rom[chr_base + i] = static_cast<u8>(i / 0x1000u);
    }
    return rom;
}

} // namespace

// ===========================================================================
// Header parsing
// ===========================================================================

TEST(Ines, ReadsTheBasicFields)
{
    const std::vector<u8> rom = make_ines(2, 1);
    const auto result = nes::parse_ines_header(rom);

    ASSERT_TRUE(result.ok()) << result.error;
    const auto& h = *result.header;

    EXPECT_EQ(h.prg_rom_pages, 2);
    EXPECT_EQ(h.chr_rom_pages, 1);
    EXPECT_EQ(h.prg_rom_size(), 32768u);
    EXPECT_EQ(h.chr_rom_size(), 8192u);
    EXPECT_EQ(h.total_size(), rom.size());
    EXPECT_EQ(h.mapper, 0);
}

TEST(Ines, RejectsAFileThatIsTooShortForAHeader)
{
    const std::vector<u8> tiny = { 'N', 'E', 'S' };
    const auto result = nes::parse_ines_header(tiny);

    EXPECT_FALSE(result.ok());
    EXPECT_NE(result.error.find("shorter"), std::string::npos);
}

TEST(Ines, RejectsAWrongSignature)
{
    std::vector<u8> rom = make_ines(2, 1);
    rom[3] = 0x00;   // break the 0x1A

    const auto result = nes::parse_ines_header(rom);
    EXPECT_FALSE(result.ok());
    EXPECT_NE(result.error.find("signature"), std::string::npos);
}

TEST(Ines, RejectsAZipFileThatHappensToBeBigEnough)
{
    // A PK zip starts with "PK\x03\x04", which is not "NES\x1A".
    std::vector<u8> rom(64, 0);
    rom[0] = 'P';
    rom[1] = 'K';
    rom[2] = 0x03;
    rom[3] = 0x04;

    const auto result = nes::parse_ines_header(rom);
    EXPECT_FALSE(result.ok());
}

TEST(Ines, RejectsAHeaderClaimingZeroPrgPages)
{
    std::vector<u8> rom = make_ines(2, 1);
    rom[4] = 0;

    const auto result = nes::parse_ines_header(rom);
    EXPECT_FALSE(result.ok());
    EXPECT_NE(result.error.find("PRG"), std::string::npos);
}

TEST(Ines, RejectsATruncatedDump)
{
    std::vector<u8> rom = make_ines(2, 1);
    rom.resize(rom.size() - 100);   // the header still claims the full size

    const auto result = nes::parse_ines_header(rom);
    EXPECT_FALSE(result.ok());
    EXPECT_NE(result.error.find("shorter than"), std::string::npos);
}

TEST(Ines, MirroringComesFromBit0OfFlags6)
{
    EXPECT_EQ(nes::parse_ines_header(make_ines(1, 1, 0x00)).header->mirroring,
              nes::Mirroring::Horizontal);
    EXPECT_EQ(nes::parse_ines_header(make_ines(1, 1, 0x01)).header->mirroring,
              nes::Mirroring::Vertical);
    // Bit 3 overrides bit 0.
    EXPECT_EQ(nes::parse_ines_header(make_ines(1, 1, 0x09)).header->mirroring,
              nes::Mirroring::FourScreen);
}

TEST(Ines, MapperNumberIsSplitAcrossTwoBytes)
{
    // mapper 3   -> flags6 high nibble 3, flags7 high nibble 0
    EXPECT_EQ(nes::parse_ines_header(make_ines(1, 1, 0x30, 0x00)).header->mapper, 3);
    // mapper 4   -> flags6 high nibble 4
    EXPECT_EQ(nes::parse_ines_header(make_ines(1, 1, 0x40, 0x00)).header->mapper, 4);
    // mapper 0x21 -> low nibble 1 in flags6, high nibble 2 in flags7
    EXPECT_EQ(nes::parse_ines_header(make_ines(1, 1, 0x10, 0x20)).header->mapper, 0x21);
    // mapper 0xF0 -> high nibble F in flags7
    EXPECT_EQ(nes::parse_ines_header(make_ines(1, 1, 0x00, 0xF0)).header->mapper, 0xF0);
}

TEST(Ines, BatteryAndTrainerFlags)
{
    // Battery: no extra data, so the size still adds up.
    const auto battery = nes::parse_ines_header(make_ines(1, 1, 0x02));
    ASSERT_TRUE(battery.ok()) << battery.error;
    EXPECT_TRUE(battery.header->has_battery);
    EXPECT_FALSE(battery.header->has_trainer);

    // Trainer: the flag promises 512 extra bytes after the header, so the
    // file has to actually contain them or the size check rejects it.
    const std::vector<u8> trainer(512, 0x00);
    const auto trained = nes::parse_ines_header(make_ines(1, 1, 0x04, 0x00, trainer));
    ASSERT_TRUE(trained.ok()) << trained.error;
    EXPECT_TRUE(trained.header->has_trainer);
    EXPECT_FALSE(trained.header->has_battery);

    // With the flag set but the bytes missing, parsing must fail rather than
    // silently reading the PRG ROM out of position.
    const auto broken = nes::parse_ines_header(make_ines(1, 1, 0x04));
    EXPECT_FALSE(broken.ok()) << "a trainer flag without the bytes is a bad dump";
}

TEST(Ines, TrainerShiftsTheRomDataBy512Bytes)
{
    // A trainer is 512 bytes between the header and the PRG ROM.
    std::vector<u8> trainer(512, 0xEE);
    const std::vector<u8> rom = make_ines(1, 1, 0x04, 0x00, trainer);

    const auto header = nes::parse_ines_header(rom);
    ASSERT_TRUE(header.ok()) << header.error;
    EXPECT_TRUE(header.header->has_trainer);
    EXPECT_EQ(header.header->trainer_size(), 512u);
    EXPECT_EQ(header.header->total_size(), rom.size());

    std::string error;
    auto cart = load(rom, error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->trainer().size(), 512u);
    EXPECT_EQ(cart->trainer()[0], 0xEE);

    // PRG byte 0 must be 0, not the first trainer byte.
    EXPECT_EQ(cart->prg_rom()[0], 0x00);
    EXPECT_EQ(cart->prg_rom()[1], 0x01);
}

// ===========================================================================
// Mapper 0: NROM
// ===========================================================================

TEST(Mapper0, ThirtyTwoKilobytesMapsStraightThrough)
{
    std::string error;
    auto cart = load(make_ines(2, 1), error);
    ASSERT_TRUE(cart.has_value()) << error;

    int mismatches = 0;
    for (std::size_t i = 0; i < 32768; ++i) {
        const u16 address = static_cast<u16>(0x8000 + i);
        if (cart->read(address) != static_cast<u8>(i & 0xFFu)) {
            ++mismatches;
        }
    }
    EXPECT_EQ(mismatches, 0);
}

TEST(Mapper0, SixteenKilobytesMirrorsIntoBothHalves)
{
    std::string error;
    auto cart = load(make_ines(1, 1), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // $8000-$BFFF and $C000-$FFFF are the same 16KB. This is a third kind of
    // mirroring, and like the others it comes from an address line nobody
    // bothered to decode.
    int mismatches = 0;
    for (u16 offset = 0; offset < 0x4000; ++offset) {
        const u8 low = cart->read(static_cast<u16>(0x8000 + offset));
        const u8 high = cart->read(static_cast<u16>(0xC000 + offset));
        if (low != high) {
            ++mismatches;
        }
        if (low != static_cast<u8>(offset & 0xFF)) {
            ++mismatches;
        }
    }
    EXPECT_EQ(mismatches, 0);
}

TEST(Mapper0, WritesToPrgRomGoNowhere)
{
    std::string error;
    auto cart = load(make_ines(2, 1), error);
    ASSERT_TRUE(cart.has_value()) << error;

    const u8 before = cart->read(0x8000);
    cart->write(0x8000, 0x55);

    EXPECT_EQ(cart->read(0x8000), before) << "NROM has no bank register";
}

TEST(Mapper0, ChrRomIsReadOnly)
{
    std::string error;
    auto cart = load(make_ines(1, 1), error);
    ASSERT_TRUE(cart.has_value()) << error;

    const u8 before = cart->read_chr(0x0010);
    cart->write_chr(0x0010, 0x99);

    EXPECT_EQ(cart->read_chr(0x0010), before) << "CHR ROM, not RAM";
    EXPECT_EQ(cart->header().chr_rom_pages, 1);
}

TEST(Mapper0, ZeroChrPagesMeansChrRam)
{
    std::string error;
    auto cart = load(make_ines(1, 0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->header().chr_rom_pages, 0);
    EXPECT_EQ(cart->chr_rom().size(), 0u);

    // With CHR RAM the pattern tables start empty and can be written.
    EXPECT_EQ(cart->read_chr(0x0010), 0x00);
    cart->write_chr(0x0010, 0x99);
    EXPECT_EQ(cart->read_chr(0x0010), 0x99);
}

// ===========================================================================
// Mapper 1: MMC1
//
// The trick to testing this mapper is that the CPU writes ONE BIT at a time,
// and the register that receives the completed value is chosen by the
// address, not by the value. So a test helper clocks five bits in, and the
// assertions are about which bank answers which half of the address space.
// ===========================================================================

TEST(Mapper1, LoadsAndKeepsTheHeaderFields)
{
    std::string error;
    // flags6 0x12: mapper 1, horizontal, battery.
    auto cart = load(make_banked_ines(8, 16, 0x12), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->header().mapper, 1);
    EXPECT_EQ(cart->prg_rom().size(), 128u * 1024u);
    EXPECT_EQ(cart->chr_rom().size(), 128u * 1024u);
    EXPECT_TRUE(cart->header().has_battery);
}

TEST(Mapper1, PowerOnStartsInPrgModeThree)
{
    std::string error;
    auto cart = load(make_banked_ines(8, 16, 0x10), error);
    ASSERT_TRUE(cart.has_value()) << error;

    auto* mapper = dynamic_cast<nes::Mapper1*>(&cart->mapper());
    ASSERT_NE(mapper, nullptr);

    EXPECT_EQ(mapper->control(), 0x0C) << "reset forces PRG mode 3";
    EXPECT_EQ(mapper->shift_register(), 0x10) << "empty, with its sentinel";

    // Mode 3: bank 0 at $8000, the last bank fixed at $C000.
    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xC000), 7);
}

TEST(Mapper1, FiveSerialWritesSetTheControlRegister)
{
    std::string error;
    auto cart = load(make_banked_ines(8, 16, 0x10), error);
    ASSERT_TRUE(cart.has_value()) << error;
    auto* mapper = dynamic_cast<nes::Mapper1*>(&cart->mapper());
    ASSERT_NE(mapper, nullptr);

    // 0x1F = 11111: 4KB CHR (bit 4), PRG mode 3 (bits 2-3), horizontal
    // mirroring (bits 0-1 = 11).
    mmc1_write(*cart, 0x8000, 0x1F);

    EXPECT_EQ(mapper->control(), 0x1F);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Horizontal);
}

TEST(Mapper1, AHighBitResetsTheShiftRegister)
{
    std::string error;
    auto cart = load(make_banked_ines(8, 16, 0x10), error);
    ASSERT_TRUE(cart.has_value()) << error;
    auto* mapper = dynamic_cast<nes::Mapper1*>(&cart->mapper());
    ASSERT_NE(mapper, nullptr);

    // Clock four bits of a control value, then reset instead of finishing.
    cart->write(0x8000, 1);
    cart->write(0x8000, 0);
    cart->write(0x8000, 1);
    cart->write(0x8000, 0);
    cart->write(0x8000, 0x80);

    EXPECT_EQ(mapper->shift_register(), 0x10) << "back to empty";
    EXPECT_EQ(mapper->control(), 0x0C) << "and the reset value is forced";
}

TEST(Mapper1, PrgModeThreeSwitchesTheLowHalfAndFixesTheLastBank)
{
    std::string error;
    auto cart = load(make_banked_ines(8, 16, 0x10), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // Control stays at its reset value, which is mode 3. Select PRG bank 3.
    mmc1_write(*cart, 0xE000, 3);

    EXPECT_EQ(cart->read(0x8000), 3) << "the low half is switchable";
    EXPECT_EQ(cart->read(0xBFFF), 3) << "all of it";
    EXPECT_EQ(cart->read(0xC000), 7) << "the last bank is fixed at $C000";
    EXPECT_EQ(cart->read(0xFFFF), 7);
}

TEST(Mapper1, PrgModeTwoFixesTheFirstBankAndSwitchesTheHighHalf)
{
    std::string error;
    auto cart = load(make_banked_ines(8, 16, 0x10), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // Control 0x08: PRG mode 2, 8KB CHR, single-screen lower mirroring.
    mmc1_write(*cart, 0x8000, 0x08);
    mmc1_write(*cart, 0xE000, 5);

    EXPECT_EQ(cart->read(0x8000), 0) << "bank 0 is fixed at $8000";
    EXPECT_EQ(cart->read(0xC000), 5) << "the high half is switchable";
}

TEST(Mapper1, PrgModeZeroSwitchesA32KiloByteBank)
{
    std::string error;
    auto cart = load(make_banked_ines(8, 16, 0x10), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // Control 0x00: PRG mode 0. The bank register's bit 0 is ignored, so a
    // value of 4 selects the 32KB bank made of 16KB banks 4 and 5.
    mmc1_write(*cart, 0x8000, 0x00);
    mmc1_write(*cart, 0xE000, 4);

    EXPECT_EQ(cart->read(0x8000), 4);
    EXPECT_EQ(cart->read(0xBFFF), 4);
    EXPECT_EQ(cart->read(0xC000), 5);
    EXPECT_EQ(cart->read(0xFFFF), 5);
}

TEST(Mapper1, ChrFourKiloByteModeSwitchesTheHalvesSeparately)
{
    std::string error;
    auto cart = load(make_banked_ines(8, 16, 0x10), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // Control 0x1C: 4KB CHR (bit 4), PRG mode 3, single-screen lower.
    mmc1_write(*cart, 0x8000, 0x1C);
    mmc1_write(*cart, 0xA000, 3);   // CHR bank 0
    mmc1_write(*cart, 0xC000, 7);   // CHR bank 1

    EXPECT_EQ(cart->read_chr(0x0000), 3);
    EXPECT_EQ(cart->read_chr(0x0FFF), 3);
    EXPECT_EQ(cart->read_chr(0x1000), 7);
    EXPECT_EQ(cart->read_chr(0x1FFF), 7);
}

TEST(Mapper1, ChrEightKiloByteModeUsesBankZeroShiftedRight)
{
    std::string error;
    auto cart = load(make_banked_ines(8, 16, 0x10), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // Control 0x0C: 8KB CHR. The register holds the bank number DOUBLED,
    // because bit 0 selects the 4KB half inside the bank and is ignored in
    // this mode. Writing 2 therefore selects 8KB bank 1 (4KB banks 2 and 3).
    mmc1_write(*cart, 0x8000, 0x0C);
    mmc1_write(*cart, 0xA000, 2);

    EXPECT_EQ(cart->read_chr(0x0000), 2);
    EXPECT_EQ(cart->read_chr(0x1FFF), 3);

    // $10 has bit 4 set, so it selects 8KB bank 8: 4KB banks 16 and 17.
    mmc1_write(*cart, 0xA000, 0x10);
    EXPECT_EQ(cart->read_chr(0x0000), 16);
    EXPECT_EQ(cart->read_chr(0x1FFF), 17);
}

TEST(Mapper1, ControlBitsChooseTheMirroring)
{
    std::string error;
    auto cart = load(make_banked_ines(8, 16, 0x10), error);
    ASSERT_TRUE(cart.has_value()) << error;

    mmc1_write(*cart, 0x8000, 0x0C | 0);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::SingleScreenLower);

    mmc1_write(*cart, 0x8000, 0x0C | 1);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::SingleScreenUpper);

    mmc1_write(*cart, 0x8000, 0x0C | 2);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Vertical);

    mmc1_write(*cart, 0x8000, 0x0C | 3);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Horizontal);
}

TEST(Mapper1, ZeroChrPagesMeansChrRam)
{
    std::string error;
    auto cart = load(make_banked_ines(8, 0, 0x10), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->chr_rom().size(), 0u);
    EXPECT_EQ(cart->read_chr(0x0100), 0x00);
    cart->write_chr(0x0100, 0x5A);
    EXPECT_EQ(cart->read_chr(0x0100), 0x5A);
}

// ===========================================================================
// Mapper 2: UxROM
// ===========================================================================

TEST(Mapper2, SwitchesTheLowHalfAndFixesTheLastBank)
{
    std::string error;
    auto cart = load(make_banked_ines(4, 0, 0x20), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // Bank 0 at $8000, the last bank pinned at $C000.
    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xC000), 3);

    cart->write(0x8000, 2);
    EXPECT_EQ(cart->read(0x8000), 2);
    EXPECT_EQ(cart->read(0xBFFF), 2);
    EXPECT_EQ(cart->read(0xC000), 3) << "the top bank never moves";
}

TEST(Mapper2, ChrIsRamAndWritable)
{
    std::string error;
    auto cart = load(make_banked_ines(4, 0, 0x20), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read_chr(0x0100), 0x00);
    cart->write_chr(0x0100, 0x5A);
    EXPECT_EQ(cart->read_chr(0x0100), 0x5A);
}

// ===========================================================================
// Mapper 3: CNROM
// ===========================================================================

TEST(Mapper3, SwitchesChrAndLeavesPrgAlone)
{
    std::string error;
    auto cart = load(make_banked_ines(2, 4, 0x30), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // The named 4KB banks make an 8KB bank pair (0,1), (2,3), ...
    EXPECT_EQ(cart->read_chr(0x0000), 0);
    EXPECT_EQ(cart->read_chr(0x1000), 1);

    cart->write(0x8000, 2);
    EXPECT_EQ(cart->read_chr(0x0000), 4);
    EXPECT_EQ(cart->read_chr(0x1000), 5);

    // PRG is fixed: a write does not move it.
    EXPECT_EQ(cart->read(0x8000), 0);
}

// ===========================================================================
// Mapper 7: AxROM
// ===========================================================================

TEST(Mapper7, SwitchesWhole32KiloByteBanks)
{
    std::string error;
    auto cart = load(make_banked_ines(4, 0, 0x70), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // The named 16KB banks make a 32KB bank out of (0,1) and (2,3).
    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xC000), 1);

    cart->write(0x8000, 1);
    EXPECT_EQ(cart->read(0x8000), 2);
    EXPECT_EQ(cart->read(0xC000), 3);
}

TEST(Mapper7, BitFourChoosesTheSingleScreenMirroring)
{
    std::string error;
    auto cart = load(make_banked_ines(4, 0, 0x70), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x8000, 0x00);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::SingleScreenLower);

    cart->write(0x8000, 0x10);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::SingleScreenUpper);
}

// ===========================================================================
// Mapper 11: Color Dreams
// ===========================================================================

TEST(Mapper11, SwitchesChrBanks)
{
    std::string error;
    auto cart = load(make_banked_ines(2, 4, 0xB0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x8000, 2);
    EXPECT_EQ(cart->read_chr(0x0000), 4);
    EXPECT_EQ(cart->read_chr(0x1000), 5);
}

// ===========================================================================
// Mapper 4: MMC3
// ===========================================================================

TEST(Mapper4, PrgModeZeroSwitchesR6AndFixesTheLastTwoBanks)
{
    std::string error;
    auto cart = load(make_mmc3_ines(8, 4), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // Select register 6, then set it to 8KB bank 2.
    cart->write(0x8000, 6);
    cart->write(0x8001, 2);
    // Register 7 defaults to 0.
    cart->write(0x8000, 7);
    cart->write(0x8001, 3);

    EXPECT_EQ(cart->read(0x8000), 2) << "R6 at $8000 in PRG mode 0";
    EXPECT_EQ(cart->read(0xA000), 3) << "R7 is always at $A000";
    EXPECT_EQ(cart->read(0xC000), 14) << "second to last bank fixed";
    EXPECT_EQ(cart->read(0xE000), 15) << "last bank fixed";
}

TEST(Mapper4, PrgModeOnePutsR6AtC000)
{
    std::string error;
    auto cart = load(make_mmc3_ines(8, 4), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x8000, 6);
    cart->write(0x8001, 5);
    cart->write(0x8000, 0x40);   // PRG mode 1

    EXPECT_EQ(cart->read(0x8000), 14) << "second to last bank fixed at $8000";
    EXPECT_EQ(cart->read(0xC000), 5) << "R6 moved to $C000";
}

TEST(Mapper4, ChrTwoKiloByteMode)
{
    std::string error;
    auto cart = load(make_mmc3_ines(8, 4), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // CHR mode 0 (the reset value): R0 is a 2KB bank at $0000.
    cart->write(0x8000, 0);
    cart->write(0x8001, 2);

    EXPECT_EQ(cart->read_chr(0x0000), 2) << "1KB bank 2";
    EXPECT_EQ(cart->read_chr(0x0400), 3) << "1KB bank 3";
}

TEST(Mapper4, ChrOneKiloByteModeSwapsTheHalves)
{
    std::string error;
    auto cart = load(make_mmc3_ines(8, 4), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // CHR mode 1: R2-R5 move to the bottom and R0/R1 to the top.
    cart->write(0x8000, 0x80);   // select R0 with CHR mode 1
    cart->write(0x8001, 5);      // R0 = 5
    cart->write(0x8000, 0x82);   // select R2 with CHR mode 1
    cart->write(0x8001, 9);      // R2 = 9

    EXPECT_EQ(cart->read_chr(0x1000), 5) << "R0 at $1000";
    EXPECT_EQ(cart->read_chr(0x1400), 6) << "R0 + 1 at $1400";
    EXPECT_EQ(cart->read_chr(0x0000), 9) << "R2 moved to $0000";
}

TEST(Mapper4, BitZeroOfA000ChoosesTheMirroring)
{
    std::string error;
    auto cart = load(make_mmc3_ines(8, 4), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0xA000, 0);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Vertical);

    cart->write(0xA000, 1);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Horizontal);
}

TEST(Mapper4, TheScanlineCounterFiresTheIrq)
{
    std::string error;
    auto cart = load(make_mmc3_ines(8, 4), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0xC000, 3);   // latch = 3
    cart->write(0xC001, 0);   // reload on the next edge
    cart->write(0xE001, 0);   // enable

    // The reload edge loads the counter; the IRQ comes after three more.
    mmc3_scanline(*cart);
    EXPECT_FALSE(cart->mapper().irq_asserted()) << "the reload edge does not fire";
    mmc3_scanline(*cart);
    mmc3_scanline(*cart);
    EXPECT_FALSE(cart->mapper().irq_asserted());
    mmc3_scanline(*cart);
    EXPECT_TRUE(cart->mapper().irq_asserted());

    // $E000 disables AND acknowledges in one write.
    cart->write(0xE000, 0);
    EXPECT_FALSE(cart->mapper().irq_asserted());
}

TEST(Mapper4, TheIrqStaysQuietWhenDisabled)
{
    std::string error;
    auto cart = load(make_mmc3_ines(8, 4), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0xC000, 0);
    cart->write(0xC001, 0);
    // Not enabled.
    for (int i = 0; i < 20; ++i) {
        mmc3_scanline(*cart);
    }
    EXPECT_FALSE(cart->mapper().irq_asserted());
}

TEST(Mapper4, TheCounterIsClockedOncePerScanlineEvenWithNoSprites)
{
    // Regression for the Super Mario Bros. 3 status bar. The counter is
    // clocked by PPU A12, which only pulses when the PPU fetches a sprite
    // pattern. The real PPU performs eight sprite fetches on *every*
    // scanline, filling empty slots with dummy reads, so A12 pulses even on
    // a line with nothing on it. If the emulator only fetched patterns for
    // sprites that are actually visible, a quiet line would leave A12 low,
    // the counter would never reach zero, and the IRQ-driven split that
    // draws the map screen's status bar would never fire.
    std::string error;
    auto cart = load(make_mmc3_ines(8, 4), error);
    ASSERT_TRUE(cart.has_value()) << error;

    nes::Ppu ppu;
    ppu.set_cartridge(&*cart);

    // 8x8 sprites from $1000, background from $0000, background on.
    ppu.write(0x2000, 0x08);
    ppu.write(0x2001, 0x08);

    // Park every sprite well below the screen so no line has one.
    ppu.write(0x2003, 0x00);
    for (int i = 0; i < 256; ++i) {
        ppu.write(0x2004, 0xFF);
    }

    auto& mapper = static_cast<nes::Mapper4&>(cart->mapper());
    const long before = mapper.irq_clock_count();
    ppu.tick(341 * 262);   // one whole frame
    const long clocks = mapper.irq_clock_count() - before;

    EXPECT_GT(clocks, 200) << "one clock per visible scanline";
    EXPECT_LT(clocks, 300) << "and no more than one each";
}

// ===========================================================================
// Mapper 9: MMC2
// ===========================================================================

TEST(Mapper9, PrgHasTwoSwitchableAndTwoPinnedBanks)
{
    std::string error;
    auto cart = load(make_8k_prg_ines(4, 2, 0x90), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // 64KB PRG = eight 8KB banks.
    EXPECT_EQ(cart->read(0x8000), 0) << "R0 starts at 0";
    EXPECT_EQ(cart->read(0xA000), 6) << "second to last is pinned";
    EXPECT_EQ(cart->read(0xC000), 0) << "R1 starts at 0";
    EXPECT_EQ(cart->read(0xE000), 7) << "last is pinned";

    cart->write(0xA000, 3);   // R0 = 3
    cart->write(0xC000, 4);   // R1 = 4
    EXPECT_EQ(cart->read(0x8000), 3);
    EXPECT_EQ(cart->read(0xC000), 4);
}

TEST(Mapper9, ThePpuFlipsTheChrLatches)
{
    std::string error;
    auto cart = load(make_8k_prg_ines(4, 2, 0x90), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // R0 = 1, R1 = 2. The latch starts clear, so the low half uses R0.
    cart->write(0xB000, 1);
    cart->write(0xD000, 2);
    EXPECT_EQ(cart->read_chr(0x0000), 1);

    // Fetching tile $FD sets the latch; fetching $FE clears it again.
    cart->mapper().on_ppu_address(0x0FD0);
    EXPECT_EQ(cart->read_chr(0x0000), 2);
    cart->mapper().on_ppu_address(0x0FE0);
    EXPECT_EQ(cart->read_chr(0x0000), 1);

    // The high half has its own latch, driven by the high tiles.
    cart->write(0xE000, 0);
    cart->write(0xF000, 3);
    EXPECT_EQ(cart->read_chr(0x1000), 0);
    cart->mapper().on_ppu_address(0x1FD0);
    EXPECT_EQ(cart->read_chr(0x1000), 3);
}

// ===========================================================================
// Mapper 10: MMC4
// ===========================================================================

TEST(Mapper10, PrgIsOneSwitchable16KiloByteBankAtTheTop)
{
    std::string error;
    auto cart = load(make_8k_prg_ines(8, 2, 0xA0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // 128KB PRG = eight 16KB banks. $8000 is pinned to the second to last.
    EXPECT_EQ(cart->read(0x8000), 12) << "16KB bank 6 starts at byte 12";
    EXPECT_EQ(cart->read(0xC000), 0) << "R0 starts at 0";

    cart->write(0xA000, 2);
    EXPECT_EQ(cart->read(0xC000), 4) << "16KB bank 2 starts at byte 4";
}

TEST(Mapper10, UsesTheSameChrLatchesAsMmc2)
{
    std::string error;
    auto cart = load(make_8k_prg_ines(8, 2, 0xA0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0xB000, 1);
    cart->write(0xC000, 2);
    EXPECT_EQ(cart->read_chr(0x0000), 1);
    cart->mapper().on_ppu_address(0x0FD0);
    EXPECT_EQ(cart->read_chr(0x0000), 2);
}

// ===========================================================================
// Mapper 13: CPROM
// ===========================================================================

TEST(Mapper13, SwitchesA4KiloByteWindowOverItsOwnChrRam)
{
    std::string error;
    auto cart = load(make_ines(2, 0, 0xD0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 0) << "PRG is fixed";

    // The mapper owns 16KB of CHR RAM; the register picks the low 4KB.
    cart->write_chr(0x0000, 0xAA);
    cart->write_chr(0x1000, 0xBB);
    EXPECT_EQ(cart->read_chr(0x0000), 0xAA);
    EXPECT_EQ(cart->read_chr(0x1000), 0xBB);

    // Bank 1 writes to a different place than bank 0 did.
    cart->write(0x8000, 1);
    EXPECT_EQ(cart->read_chr(0x0000), 0x00);
    cart->write_chr(0x0000, 0xCC);
    EXPECT_EQ(cart->read_chr(0x0000), 0xCC);

    cart->write(0x8000, 0);
    EXPECT_EQ(cart->read_chr(0x0000), 0xAA) << "bank 0 came back";
}

// ===========================================================================
// Mapper 15: 100-in-1
// ===========================================================================

TEST(Mapper15, SwitchesTheLow16KiloBytesAndFixesTheTopOnes)
{
    std::string error;
    auto cart = load(make_banked_ines(4, 1, 0xF0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // 64KB PRG = four 16KB banks, named 0..3. $8000 switches, $C000 is the
    // last bank and never moves, so the vectors stay put.
    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xC000), 3);

    cart->write(0x8000, 1);
    EXPECT_EQ(cart->read(0x8000), 1);
    EXPECT_EQ(cart->read(0xC000), 3) << "the top bank is pinned";

    // Bit 6 is not a bank bit: it picks the single-screen half.
    cart->write(0x8000, 0x40);
    EXPECT_EQ(cart->read(0x8000), 0) << "bank 0 again";
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::SingleScreenUpper);
}

// ===========================================================================
// Mapper 163: Nanjing FC-001
// ===========================================================================

TEST(Mapper163, PowerOnBootsInBankThree)
{
    std::string error;
    auto cart = load(make_nanjing_ines(8), error);   // 128KB = four 32KB banks
    ASSERT_TRUE(cart.has_value()) << error;

    // All registers start at zero, so mode bit 2 is clear and PRG A15/A16
    // are forced to 11. The boot bank is 3, not 0. A 32KB bank 3 starts at
    // 16KB bank 6 in the numbered test image.
    EXPECT_EQ(nanjing(*cart).prg_bank(), 3);
    EXPECT_EQ(cart->read(0x8000), 6);
    EXPECT_EQ(cart->read(0xC000), 7) << "the same 32KB bank continues at $C000";
}

TEST(Mapper163, ModeBitTwoChoosesWhereTheLowBankBitsComeFrom)
{
    std::string error;
    auto cart = load(make_nanjing_ines(64), error);   // 1MB = thirty-two banks
    ASSERT_TRUE(cart.has_value()) << error;

    // A=1: A15/A16 come from $5000, so bank 1 is reachable.
    cart->write(0x5300, 0x04);
    cart->write(0x5000, 0x01);
    EXPECT_EQ(cart->read(0x8000), 2) << "32KB bank 1";

    // A=0 again: the low two bits snap back to 11, so it is bank 3.
    cart->write(0x5300, 0x00);
    EXPECT_EQ(cart->read(0x8000), 6) << "32KB bank 3";
}

TEST(Mapper163, TheHighBankBitsComeFromFiveThousandTwoHundred)
{
    std::string error;
    auto cart = load(make_nanjing_ines(64), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x5300, 0x04);   // A=1
    cart->write(0x5000, 0x05);   // low nibble = 5
    cart->write(0x5200, 0x01);   // high bits = 1, so the bank is $15 = 21

    EXPECT_EQ(nanjing(*cart).prg_bank(), 0x15);
    EXPECT_EQ(cart->read(0x8000), 42) << "16KB bank 42 is the start of 32KB bank 21";
}

TEST(Mapper163, ModeBitZeroSwapsDataBitsZeroAndOne)
{
    std::string error;
    auto cart = load(make_nanjing_ines(64), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x5300, 0x04);   // A=1, B=0
    cart->write(0x5000, 0x01);
    EXPECT_EQ(cart->read(0x8000), 2) << "bank 1, no swap";

    cart->write(0x5300, 0x05);   // A=1, B=1: writes now swap bits 0 and 1
    cart->write(0x5000, 0x01);   // lands as 0x02, i.e. bank 2
    EXPECT_EQ(cart->read(0x8000), 4) << "bank 2 after the swap";
}

TEST(Mapper163, TheFeedbackBitReadsBackInverted)
{
    std::string error;
    auto cart = load(make_nanjing_ines(8), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // D2 is F, D0 is E. Latching F=1 makes the read give the inverted bit,
    // which is what the anti-piracy check looks for.
    cart->write(0x5100, 0x04);   // F=1, E=0
    EXPECT_EQ(nanjing(*cart).feedback_bit(), 1);
    EXPECT_EQ(cart->read(0x5500) & 0x04, 0x00);

    cart->write(0x5100, 0x00);   // F=0
    EXPECT_EQ(cart->read(0x5500) & 0x04, 0x04);
}

TEST(Mapper163, TheStrobeFlipsTheFeedbackBitOnAFallingEdge)
{
    std::string error;
    auto cart = load(make_nanjing_ines(8), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x5100, 0x05);   // F=1, E=1
    ASSERT_EQ(nanjing(*cart).feedback_bit(), 1);

    cart->write(0x5101, 0x00);   // E falls: 1 -> 0, so F flips
    EXPECT_EQ(nanjing(*cart).feedback_bit(), 0);
    EXPECT_EQ(cart->read(0x5500) & 0x04, 0x04);

    cart->write(0x5101, 0x01);   // E rises: no edge, no flip
    EXPECT_EQ(nanjing(*cart).feedback_bit(), 0);
    cart->write(0x5101, 0x00);   // falls again
    EXPECT_EQ(nanjing(*cart).feedback_bit(), 1);
}

TEST(Mapper163, ChrRamIsWritable)
{
    std::string error;
    auto cart = load(make_nanjing_ines(8), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write_chr(0x0123, 0xA5);
    EXPECT_EQ(cart->read_chr(0x0123), 0xA5);
}

TEST(Mapper163, TheAutomaticSwitchMovesBothPatternTables)
{
    std::string error;
    auto cart = load(make_nanjing_ines(8), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write_chr(0x0000, 0x11);
    cart->write_chr(0x1000, 0x22);

    // With the switch off the two halves read normally.
    EXPECT_EQ(cart->read_chr(0x0000), 0x11);
    EXPECT_EQ(cart->read_chr(0x1000), 0x22);

    // $5000 bit 7 turns it on. Both pattern tables then come from the same
    // 4KB half, switched by the beam position.
    cart->write(0x5300, 0x04);
    cart->write(0x5000, 0x80);
    EXPECT_TRUE(nanjing(*cart).auto_switch());

    EXPECT_EQ(cart->read_chr(0x0000), 0x11);   // top half, page 0
    cart->mapper().on_scanline(127);
    EXPECT_EQ(cart->read_chr(0x0000), 0x22) << "bottom half uses page 1";
    EXPECT_EQ(cart->read_chr(0x1000), 0x22);
    cart->mapper().on_scanline(239);
    EXPECT_EQ(cart->read_chr(0x0000), 0x11) << "back to page 0";
}

// ===========================================================================
// Mapper 226: 76-in-1
// ===========================================================================

TEST(Mapper226, ModeZeroShowsA32KiloByteBank)
{
    std::string error;
    auto cart = load(make_banked_ines_with(32, 0, 0x20, 0xE0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // Power on: register 0, mode 0, so $8000-$FFFF is 32KB bank 0 and the
    // low bit of the bank number is dropped.
    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xC000), 1);

    cart->write(0x8000, 0x04);
    EXPECT_EQ(cart->read(0x8000), 4);
    EXPECT_EQ(cart->read(0xC000), 5);

    cart->write(0x8000, 0x05);   // odd: the low bit is not connected
    EXPECT_EQ(cart->read(0x8000), 4);
}

TEST(Mapper226, ModeOneRepeatsTheSameBankInBothHalves)
{
    std::string error;
    auto cart = load(make_banked_ines_with(32, 0, 0x20, 0xE0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x8000, 0x24);   // bit 5 = mode 1, bank low = 4
    EXPECT_EQ(cart->read(0x8000), 4);
    EXPECT_EQ(cart->read(0xC000), 4) << "the same 16KB bank is wired to both halves";
}

TEST(Mapper226, TheSixthAndSeventhBankBitsLiveInOddPlaces)
{
    std::string error;
    auto cart = load(make_banked_ines_with(128, 0, 0x20, 0xE0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    // Bank bit 6 is $8001 bit 0.
    cart->write(0x8001, 0x01);
    EXPECT_EQ(cart->read(0x8000), 64);

    // Bank bit 5 is $8000 bit 7, because bit 6 of $8000 is the mirroring bit.
    cart->write(0x8000, 0x80);
    EXPECT_EQ(cart->read(0x8000), 96) << "$20 | $40 = 96";
}

TEST(Mapper226, BitSixOfTheRegisterChoosesMirroring)
{
    std::string error;
    auto cart = load(make_banked_ines_with(32, 0, 0x20, 0xE0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Horizontal);
    cart->write(0x8000, 0x40);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Vertical);
}

TEST(Mapper226, OneAndAHalfMegabyteCartsUseAScrambledBankOrder)
{
    std::string error;
    // 96 x 16KB is 1.5MB, which is the size that wires the top two bank bits
    // through the { 0, 0, 1, 2 } table.
    auto cart = load(make_banked_ines_with(96, 0, 0x20, 0xE0), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x8001, 0x01);   // base bits = 2
    EXPECT_EQ(cart->read(0x8000), 32) << "reordered base 2 means bank 1";
}

TEST(Mapper226, RunsAProgramThroughThePinnedBank)
{
    // Build a 2MB image and put a small program in 16KB bank 1. With the
    // register at zero, mode 0 makes $C000-$FFFF that bank, so the reset
    // vector at $FFFC must point at it and the CPU must execute there.
    std::vector<u8> rom = make_banked_ines_with(128, 0, 0x20, 0xE0);

    const std::vector<u8> program = {
        0xA9, 0x42,        // LDA #$42
        0x85, 0x10,        // STA $10
        0x4C, 0x00, 0xC0,  // JMP $C000
    };
    const std::size_t bank1 = 16 + 1u * 0x4000u;
    for (std::size_t i = 0; i < program.size(); ++i) {
        rom[bank1 + i] = program[i];
    }
    rom[bank1 + 0x3FFC] = 0x00;   // reset vector low
    rom[bank1 + 0x3FFD] = 0xC0;   // reset vector high

    std::string error;
    auto cart = load(rom, error);
    ASSERT_TRUE(cart.has_value()) << error;

    nes::NesBus bus;
    bus.set_cartridge(&*cart);

    Cpu cpu{ bus };
    cpu.reset();
    EXPECT_EQ(cpu.registers().pc, 0xC000);

    cpu.run(3);
    EXPECT_EQ(bus.ram().read(0x0010), 0x42);
    EXPECT_FALSE(cpu.is_halted());
}

// ===========================================================================
// The 2024/2025 batch: mappers 18, 21-25, 32, 33, 66, 68, 71, 78, 87,
// 162, 164, 178, 190, 227, 242, 246
// ===========================================================================

TEST(Mapper66, HighNibbleIsPrgAndLowNibbleIsChr)
{
    std::string error;
    auto cart = load(make_mapper_ines(8, 4, 66), error);   // 4 x 32KB, 4 x 8KB
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read_chr(0x0000), 0);

    cart->write(0x8000, 0x31);   // PRG bank 3, CHR bank 1
    EXPECT_EQ(cart->read(0x8000), 6) << "32KB bank 3 starts at 16KB bank 6";
    EXPECT_EQ(cart->read_chr(0x0000), 2) << "8KB bank 1 starts at 4KB bank 2";
}

TEST(Mapper71, LowHalfSwitchesAndTopStays)
{
    std::string error;
    auto cart = load(make_mapper_ines(8, 0, 71), error);   // 8 x 16KB
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xC000), 7) << "top 16KB is pinned to the last bank";

    cart->write(0xC000, 0x03);
    EXPECT_EQ(cart->read(0x8000), 3);
    EXPECT_EQ(cart->read(0xC000), 7);
}

TEST(Mapper71, WritingNineThousandTurnsOnTheBf9097MirroringBit)
{
    std::string error;
    auto cart = load(make_mapper_ines(8, 0, 71), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0xC000, 0x05);
    cart->write(0x9000, 0x00);          // declares itself a BF9097
    cart->write(0x8000, 0x13);          // bit 4 = 1
    EXPECT_EQ(cart->read(0x8000), 5) << "the low write did not change the bank";
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::SingleScreenLower);

    cart->write(0x8000, 0x03);          // bit 4 = 0
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::SingleScreenUpper);
}

TEST(Mapper78, SwitchesBothRoms)
{
    std::string error;
    auto cart = load(make_mapper_ines(8, 8, 78), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x8000, 0x23);   // PRG 3, CHR 2, mirroring bit 3 = 0
    EXPECT_EQ(cart->read(0x8000), 3);
    EXPECT_EQ(cart->read(0xC000), 7);
    EXPECT_EQ(cart->read_chr(0x0000), 4) << "8KB CHR bank 2 starts at 4KB bank 4";
}

TEST(Mapper87, TheRegisterLivesAtSixThousandAndThereIsNoPrgRam)
{
    std::string error;
    auto cart = load(make_mapper_ines(4, 8, 87), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_FALSE(cart->mapper().has_work_ram());

    cart->write(0x6000, 0x12);   // PRG bank 1, CHR bank 2
    EXPECT_EQ(cart->read(0x8000), 2) << "32KB bank 1 starts at 16KB bank 2";
    EXPECT_EQ(cart->read_chr(0x0000), 4);

    cart->write(0x7000, 0x0F);   // audio, must not be a bank switch
    EXPECT_EQ(cart->read(0x8000), 2);
}

TEST(Mapper32, TwoPrgLayouts)
{
    std::string error;
    auto cart = load(make_mapper_ines_8k(8, 4, 32), error);   // 16 x 8KB
    ASSERT_TRUE(cart.has_value()) << error;

    // Mode 0, everything zero: [0][0][-2][-1] = [0][0][14][15].
    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xC000), 14);
    EXPECT_EQ(cart->read(0xE000), 15);

    cart->write(0x8000, 0x02);   // slot 0 = bank 2
    cart->write(0xA000, 0x03);   // slot 1 = bank 3
    EXPECT_EQ(cart->read(0x8000), 2);
    EXPECT_EQ(cart->read(0xA000), 3);

    cart->write(0x9000, 0x03);   // mode 1, horizontal mirroring
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Horizontal);
    EXPECT_EQ(cart->read(0x8000), 14) << "slot 0 is pinned to the second-last bank";
    EXPECT_EQ(cart->read(0xA000), 3) << "slot 1 keeps its register";
    EXPECT_EQ(cart->read(0xC000), 2) << "the register moved to slot 2";
    EXPECT_EQ(cart->read(0xE000), 15);

    cart->write(0xB000, 0x04);   // 1KB CHR bank 4
    EXPECT_EQ(cart->read_chr(0x0000), 1) << "1KB bank 4 is in 4KB bank 1";
}

TEST(Mapper33, PrgAndChrBanks)
{
    std::string error;
    auto cart = load(make_mapper_ines_8k(8, 4, 33), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xC000), 14);
    EXPECT_EQ(cart->read(0xE000), 15);

    cart->write(0x8000, 0x41);   // PRG 1, horizontal
    EXPECT_EQ(cart->read(0x8000), 1);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Horizontal);

    cart->write(0x8002, 0x02);   // CHR 4 and 5
    EXPECT_EQ(cart->read_chr(0x0000), 1) << "1KB bank 4 is in 4KB bank 1";
    EXPECT_EQ(cart->read_chr(0x0400), 1) << "and bank 5 is in the same 4KB page";
}

TEST(Mapper68, FourChrBanksAndRuntimeMirroring)
{
    std::string error;
    auto cart = load(make_mapper_ines(8, 8, 68), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xC000), 7);

    cart->write(0xF000, 0x03);
    EXPECT_EQ(cart->read(0x8000), 3);

    cart->write(0xE000, 0x02);   // one-screen lower
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::SingleScreenLower);

    cart->write(0x8000, 0x02);   // CHR 2KB bank 2
    EXPECT_EQ(cart->read_chr(0x0000), 1) << "2KB bank 2 is in 4KB bank 1";
}

TEST(Mapper18, BankNumbersArriveOneNibbleAtATime)
{
    std::string error;
    auto cart = load(make_mapper_ines_8k(16, 4, 18), error);   // 32 x 8KB
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xE000), 31) << "the top 8KB is pinned to the last bank";

    cart->write(0x8000, 0x05);   // low nibble
    cart->write(0x8001, 0x01);   // high nibble -> bank $15 = 21
    EXPECT_EQ(cart->read(0x8000), 21);
}

TEST(Mapper18, TheIrqCounterIsClockedByCpuCycles)
{
    std::string error;
    auto cart = load(make_mapper_ines(16, 4, 18), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_TRUE(cart->mapper().clocks_on_cpu_cycles());

    cart->write(0xE000, 0x01);   // reload value = 1
    cart->write(0xF000, 0x00);   // reload the counter
    cart->write(0xF001, 0x09);   // enable, 4-bit counter

    EXPECT_FALSE(cart->mapper().irq_asserted());
    cart->mapper().on_cpu_cycle();
    EXPECT_TRUE(cart->mapper().irq_asserted()) << "2 -> 1 -> 0 fires";
}

TEST(Mapper21, Vrc4PrgBanksAndMirroring)
{
    std::string error;
    auto cart = load(make_mapper_ines_8k(8, 4, 21), error);   // 16 x 8KB
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x8000, 0x03);   // slot 0 = bank 3
    cart->write(0xA000, 0x02);   // slot 1 = bank 2

    EXPECT_EQ(cart->read(0x8000), 3);
    EXPECT_EQ(cart->read(0xA000), 2);
    EXPECT_EQ(cart->read(0xC000), 14) << "second-last bank pinned";
    EXPECT_EQ(cart->read(0xE000), 15) << "last bank pinned";

    cart->write(0x9000, 0x01);   // horizontal
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Horizontal);
}

TEST(Mapper21, TheIrqCounterCountsCpuCyclesThroughAPrescaler)
{
    std::string error;
    auto cart = load(make_mapper_ines(8, 4, 21), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_TRUE(cart->mapper().clocks_on_cpu_cycles());

    cart->write(0xF000, 0x0E);   // reload low nibble = $E
    cart->write(0xF002, 0x0F);   // reload high nibble = $F (VRC4a wiring)
    cart->write(0xF004, 0x02);   // enable

    EXPECT_FALSE(cart->mapper().irq_asserted());
    for (int i = 0; i < 1000; ++i) {
        cart->mapper().on_cpu_cycle();
    }
    EXPECT_TRUE(cart->mapper().irq_asserted()) << "$FE wraps after two increments";

    cart->write(0xF006, 0x00);   // acknowledge
    EXPECT_FALSE(cart->mapper().irq_asserted());
}

TEST(Mapper21, Vrc2aShiftsTheChrBankAndOwnsSixThousand)
{
    std::string error;
    auto cart = load(make_mapper_ines(8, 4, 22), error);   // mapper 22 = VRC2a
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_FALSE(cart->mapper().has_work_ram()) << "the EEPROM port replaces work RAM";
    EXPECT_FALSE(cart->mapper().clocks_on_cpu_cycles()) << "VRC2 has no IRQ";

    cart->write(0x6000, 0x01);
    EXPECT_EQ(cart->read(0x6000), 0x01) << "the serial latch reads back";
}

TEST(Mapper162, RegistersAtFiveThousandPickTheBankFormula)
{
    std::string error;
    auto cart = load(make_mapper_ines(64, 0, 162), error);   // 32 x 32KB
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 6) << "reset mode picks bank 3";

    cart->write(0x5300, 0x00);   // formula 0
    cart->write(0x5000, 0x03);
    cart->write(0x5100, 0x01);
    cart->write(0x5200, 0x01);
    EXPECT_EQ(cart->read(0x8000), 32) << "bank 16 = (reg2 << 4)";
}

TEST(Mapper164, TwoNibblesMakeTheBank)
{
    std::string error;
    auto cart = load(make_mapper_ines(64, 0, 164), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 30) << "reset bank $0F starts at 16KB bank 30";

    cart->write(0x5000, 0x02);   // low nibble
    EXPECT_EQ(cart->read(0x8000), 4);
    cart->write(0x5100, 0x01);   // high nibble -> bank $12
    EXPECT_EQ(cart->read(0x8000), 36);
}

TEST(Mapper178, TheBankIsSplitInto16KiloByteHalves)
{
    std::string error;
    auto cart = load(make_mapper_ines(32, 0, 178), error);   // 32 x 16KB
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xC000), 1);

    cart->write(0x4801, 0x05);   // low bank bits
    EXPECT_EQ(cart->read(0x8000), 4) << "a 32KB pair drops the low bit";
    EXPECT_EQ(cart->read(0xC000), 5);

    cart->write(0x4800, 0x02);   // switch to the 16+16 split
    EXPECT_EQ(cart->read(0x8000), 5);
    EXPECT_EQ(cart->read(0xC000), 7) << "the high half takes bank 7";
}

TEST(Mapper190, OnlyTheLowWindowMoves)
{
    std::string error;
    auto cart = load(make_mapper_ines(16, 0, 190), error);   // 16 x 16KB
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Vertical);

    cart->write(0x8000, 0x03);
    EXPECT_EQ(cart->read(0x8000), 3);
    EXPECT_EQ(cart->read(0xC000), 0) << "the top 16KB stays on bank 0";

    // CHR is RAM here, so bank numbers only mean something once the
    // program has written different bytes to different banks.
    cart->write_chr(0x0000, 0xAA);   // slot 0, bank 0
    cart->write(0xA000, 0x02);        // slot 0 now bank 2
    cart->write_chr(0x0000, 0xBB);
    cart->write(0xA000, 0x00);        // back to bank 0
    EXPECT_EQ(cart->read_chr(0x0000), 0xAA);
}

TEST(Mapper227, TheAddressItselfIsTheRegister)
{
    std::string error;
    auto cart = load(make_mapper_ines(32, 0, 227), error);   // 32 x 16KB
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x8004, 0x00);   // A2 set -> bank bit 0
    EXPECT_EQ(cart->read(0x8000), 1);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Vertical);

    cart->write(0x8006, 0x00);   // A1 set -> horizontal, same bank
    EXPECT_EQ(cart->read(0x8000), 1);
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Horizontal);
}

TEST(Mapper242, AddressBitsChooseTheBank)
{
    std::string error;
    auto cart = load(make_mapper_ines(32, 0, 242), error);   // 16 x 32KB
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x8008, 0x00);   // (A3..A6) = 1
    EXPECT_EQ(cart->read(0x8000), 2) << "32KB bank 1 is 16KB bank 2";
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Vertical);

    cart->write(0x800A, 0x00);   // A1 set
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Horizontal);
}

TEST(Mapper246, FourPrgAndFourChrWindowsAtSixThousand)
{
    std::string error;
    auto cart = load(make_mapper_ines_8k(16, 0, 246), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_FALSE(cart->mapper().has_work_ram());
    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xE000), 31) << "slot 3 starts on bank $FF, wrapped";

    cart->write(0x6000, 0x03);   // PRG slot 0 = 3
    EXPECT_EQ(cart->read(0x8000), 3);

    // CHR is RAM, so use bytes rather than bank numbers.
    cart->write_chr(0x0000, 0xAA);   // slot 0, bank 0
    cart->write(0x6004, 0x02);        // CHR slot 0 = 2KB bank 2
    cart->write_chr(0x0000, 0xBB);
    cart->write(0x6004, 0x00);
    EXPECT_EQ(cart->read_chr(0x0000), 0xAA);
}

// ===========================================================================
// Mapper 19 (Namco 163), 177 (Henggedianzi), 249 (scrambled MMC3)
// ===========================================================================

TEST(Mapper177, OneRegisterSwitchesTheBankAndTheMirroring)
{
    std::string error;
    auto cart = load(make_mapper_ines(8, 1, 177), error);   // 4 x 32KB
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 0);

    cart->write(0x8000, 0x03);
    EXPECT_EQ(cart->read(0x8000), 6) << "32KB bank 3 starts at 16KB bank 6";
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Vertical);

    cart->write(0x8000, 0x20);
    EXPECT_EQ(cart->read(0x8000), 0) << "bit 5 is not a bank bit";
    EXPECT_EQ(cart->mapper().mirroring(), nes::Mirroring::Horizontal);
}

TEST(Mapper19, PrgAndChrBanks)
{
    std::string error;
    auto cart = load(make_8k_1k_ines(8, 1, 19), error);   // 16 x 8KB PRG
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8000), 0);
    EXPECT_EQ(cart->read(0xE000), 15) << "the top 8KB is pinned to the last bank";

    cart->write(0xE000, 0x02);   // PRG slot 0
    cart->write(0xE800, 0x03);   // PRG slot 1
    cart->write(0xF000, 0x04);   // PRG slot 2
    EXPECT_EQ(cart->read(0x8000), 2);
    EXPECT_EQ(cart->read(0xA000), 3);
    EXPECT_EQ(cart->read(0xC000), 4);
    EXPECT_EQ(cart->read(0xE000), 15);

    cart->write(0x8000, 0x02);   // CHR slot 0
    cart->write(0xA000, 0x05);   // CHR slot 4
    EXPECT_EQ(cart->read_chr(0x0000), 2);
    EXPECT_EQ(cart->read_chr(0x1000), 5);
}

TEST(Mapper19, TheAudioRamIsAWindowWithAutoIncrement)
{
    std::string error;
    auto cart = load(make_8k_1k_ines(8, 0, 19), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0xF800, 0x00);   // address 0, no auto-increment
    cart->write(0x4800, 0xAB);
    EXPECT_EQ(cart->read(0x4800), 0xAB);

    cart->write(0xF800, 0x80);   // address 0, auto-increment on
    cart->write(0x4800, 0x11);
    cart->write(0x4800, 0x22);

    cart->write(0xF800, 0x80);   // back to address 0, auto-increment on
    EXPECT_EQ(cart->read(0x4800), 0x11);
    EXPECT_EQ(cart->read(0x4800), 0x22);
}

TEST(Mapper19, TheIrqCountsUpToSevenFfff)
{
    std::string error;
    auto cart = load(make_8k_1k_ines(8, 0, 19), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_TRUE(cart->mapper().clocks_on_cpu_cycles());

    cart->write(0x5000, 0xFE);   // low = $7FFE
    cart->write(0x5800, 0xFF);   // high = $FF: enables, and the IRQ is armed
    EXPECT_EQ(cart->read(0x5000), 0xFE);
    EXPECT_EQ(cart->read(0x5800), 0xFF);
    EXPECT_FALSE(cart->mapper().irq_asserted());

    cart->mapper().on_cpu_cycle();   // $7FFE -> $7FFF
    EXPECT_TRUE(cart->mapper().irq_asserted());

    cart->write(0x5000, 0x00);       // writing the low byte acknowledges
    EXPECT_FALSE(cart->mapper().irq_asserted());
}

TEST(Mapper249, TheBoardPermutesPrgAndChrBanks)
{
    // A mapper 249 file is stored in the order the board produces when
    // $5000=00, which is what this board's bank wiring has to undo. The
    // mapping is fixed for the whole run.
    std::string error;
    auto cart = load(make_8k_1k_ines(8, 4, 249), error);
    ASSERT_TRUE(cart.has_value()) << error;

    cart->write(0x8000, 6);      // select R6
    cart->write(0x8001, 3);
    EXPECT_EQ(cart->read(0x8000), 9) << "register 3 lands on physical bank 9";

    cart->write(0x8000, 0x82);   // CHR mode 1, select R2 for 1KB slot 0
    cart->write(0x8001, 8);
    EXPECT_EQ(cart->read_chr(0x0000), 4) << "CHR page 8 lands on page 4";

    // The fixed windows at the top are not permuted: they have to stay at
    // the end of the ROM, because that is where the vectors live.
    EXPECT_EQ(cart->read(0xE000), 15);
}

// ===========================================================================
// Cartridge as a device on the bus
// ===========================================================================

TEST(Cartridge, PrgRamAtSixThousandIsWritable)
{
    std::string error;
    auto cart = load(make_ines(1, 1), error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_TRUE(cart->has_prg_ram());

    cart->write(0x6000, 0x11);
    cart->write(0x7FFF, 0x22);

    EXPECT_EQ(cart->read(0x6000), 0x11);
    EXPECT_EQ(cart->read(0x7FFF), 0x22);
    EXPECT_EQ(cart->read(0x6001), 0x00);
}

TEST(Cartridge, RejectsAnUnimplementedMapper)
{
    std::string error;
    // mapper 5 (MMC5) is the one big one still missing.
    auto cart = load(make_ines(2, 1, 0x50), error);

    EXPECT_FALSE(cart.has_value());
    EXPECT_NE(error.find("mapper 5"), std::string::npos);
    EXPECT_NE(error.find("not implemented"), std::string::npos);
}

TEST(Cartridge, PlugsIntoTheBusAndAnswersTheResetVector)
{
    const std::vector<u8> program = { 0xEA, 0xEA, 0xEA, 0xEA };
    const std::vector<u8> rom = make_program_rom(program, 0x8000);

    std::string error;
    auto cart = load(rom, error);
    ASSERT_TRUE(cart.has_value()) << error;

    nes::NesBus bus;
    bus.set_cartridge(&*cart);

    EXPECT_EQ(bus.read(0xFFFC), 0x00);
    EXPECT_EQ(bus.read(0xFFFD), 0x80);

    Cpu cpu{ bus };
    cpu.reset();
    EXPECT_EQ(cpu.registers().pc, 0x8000);
}

TEST(Cartridge, RunsAProgramThroughTheBus)
{
    // LDA #$2A ; STA $10 ; LDX #$05 ; STA $20,X ; JMP self
    const std::vector<u8> program = {
        0xA9, 0x2A,
        0x85, 0x10,
        0xA2, 0x05,
        0x95, 0x20,
        0x4C, 0x08, 0x80,
    };
    const std::vector<u8> rom = make_program_rom(program, 0x8000);

    std::string error;
    auto cart = load(rom, error);
    ASSERT_TRUE(cart.has_value()) << error;

    nes::NesBus bus;
    bus.set_cartridge(&*cart);

    Cpu cpu{ bus };
    cpu.reset();
    cpu.run(4);

    EXPECT_EQ(bus.ram().read(0x0010), 0x2A);
    EXPECT_EQ(bus.ram().read(0x0025), 0x2A);
    EXPECT_FALSE(cpu.is_halted());
}

TEST(Cartridge, SummaryDescribesTheRom)
{
    std::string error;
    auto cart = load(make_ines(2, 1, 0x01), error);
    ASSERT_TRUE(cart.has_value()) << error;

    const std::string text = cart->summary();
    EXPECT_NE(text.find("mapper 0"), std::string::npos);
    EXPECT_NE(text.find("2x16KB PRG"), std::string::npos);
    EXPECT_NE(text.find("1x8KB CHR"), std::string::npos);
    EXPECT_NE(text.find("vertical"), std::string::npos);
}

// ===========================================================================
// The Cartridge must be a drop-in replacement for RamCartridge
// ===========================================================================

TEST(Cartridge, InterchangesWithTheRamCartridgePlaceholder)
{
    const std::vector<u8> program = { 0xA9, 0x7F };
    const std::vector<u8> rom = make_program_rom(program, 0x8000);

    std::string error;
    auto cart = load(rom, error);
    ASSERT_TRUE(cart.has_value()) << error;

    nes::NesBus bus;
    bus.set_cartridge(&*cart);   // the same slot RamCartridge used

    Cpu cpu{ bus };
    cpu.reset();
    cpu.step();

    EXPECT_EQ(cpu.registers().a, 0x7F);
}

// ===========================================================================
// Save RAM and the battery bit
//
// A front end persists the bytes prg_ram() points at, so the two things that
// matter are that it is the same memory the CPU reads at $6000 and that a
// cartridge without the battery bit does not claim to have a save at all.
// ===========================================================================

TEST(Cartridge, WithoutTheBatteryBitThereIsNoSaveRam)
{
    const std::vector<u8> rom = make_ines(2, 1, /*flags6=*/0x00);
    std::string error;
    auto cart = load(rom, error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_FALSE(cart->battery_backed());
}

TEST(Cartridge, TheBatteryBitIsReportedAndTheRamIsTheCpusRam)
{
    // flags 6 bit 1 is the battery. Bit 0 is the mirroring, so 0x02 is
    // horizontal with a battery.
    const std::vector<u8> rom = make_ines(2, 1, /*flags6=*/0x02);
    std::string error;
    auto cart = load(rom, error);
    ASSERT_TRUE(cart.has_value()) << error;

    ASSERT_TRUE(cart->battery_backed());
    ASSERT_EQ(cart->prg_ram().size(), 0x2000u);

    // The exposed buffer is the cartridge's own memory: a write through the
    // bus is the byte the front end would save, and a byte written into the
    // buffer is what the CPU reads back.
    cart->write(0x6000, 0x5A);
    EXPECT_EQ(cart->prg_ram()[0], 0x5A);
    cart->prg_ram()[1] = 0xA5;
    EXPECT_EQ(cart->read(0x6001), 0xA5);
}

TEST(Cartridge, ABoardThatDoesNotAnswer6000WithRamHasNoSave)
{
    // Mapper 87 (Jaleco JF-13) puts registers at $6000, not a RAM chip, so
    // even with the battery bit set there is nothing a front end could save.
    // The mapper number packs as (flags7 & 0xF0) | (flags6 >> 4): 87 is 0x57,
    // so flags6 carries 0x70 (plus the battery) and flags7 carries 0x50.
    std::vector<u8> rom = make_ines(2, 1, /*flags6=*/0x02 | 0x70);
    rom[7] = 0x50;

    std::string error;
    auto cart = load(rom, error);
    ASSERT_TRUE(cart.has_value()) << error;
    ASSERT_EQ(cart->header().mapper, 87);

    EXPECT_FALSE(cart->battery_backed());
}

// ===========================================================================
// Game Genie patches
//
// The patch acts on the byte the cartridge returned, at the CPU address, after
// the mapper has banked. A compare byte lets a code apply to one bank and not
// another, which is the only reason eight letter codes exist.
// ===========================================================================

TEST(Cartridge, APrgPatchReplacesTheByteTheMapperReturned)
{
    // make_ines fills PRG so that byte i is (i & 0xFF). NROM maps $8000 to
    // PRG offset 0, so $8002 reads 0x02 before any patch.
    const std::vector<u8> rom = make_ines(2, 1);
    std::string error;
    auto cart = load(rom, error);
    ASSERT_TRUE(cart.has_value()) << error;

    EXPECT_EQ(cart->read(0x8002), 0x02);

    const nes::Cartridge::PrgPatch patch{ 0x8002, 0x55, -1 };
    cart->set_prg_patches(std::span<const nes::Cartridge::PrgPatch>(&patch, 1));
    EXPECT_EQ(cart->read(0x8002), 0x55);
    // Only the named address moved.
    EXPECT_EQ(cart->read(0x8003), 0x03);

    cart->clear_prg_patches();
    EXPECT_EQ(cart->read(0x8002), 0x02);
}

TEST(Cartridge, ACompareByteLetsAPatchApplyToOnlyOneValue)
{
    const std::vector<u8> rom = make_ines(2, 1);
    std::string error;
    auto cart = load(rom, error);
    ASSERT_TRUE(cart.has_value()) << error;

    // $8002 is 0x02. A patch that expects 0x02 applies; one that expects
    // anything else leaves the byte alone.
    const nes::Cartridge::PrgPatch matching{ 0x8002, 0x55, 0x02 };
    cart->set_prg_patches(std::span<const nes::Cartridge::PrgPatch>(&matching, 1));
    EXPECT_EQ(cart->read(0x8002), 0x55);

    const nes::Cartridge::PrgPatch missing{ 0x8002, 0x66, 0x03 };
    cart->set_prg_patches(std::span<const nes::Cartridge::PrgPatch>(&missing, 1));
    EXPECT_EQ(cart->read(0x8002), 0x02);
}
