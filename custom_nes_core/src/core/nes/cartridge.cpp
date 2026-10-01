#include "core/nes/cartridge.hpp"

#include "core/nes/mapper0.hpp"
#include "core/nes/mapper1.hpp"
#include "core/nes/mapper2.hpp"
#include "core/nes/mapper3.hpp"
#include "core/nes/mapper4.hpp"
#include "core/nes/mapper74.hpp"
#include "core/nes/mapper241.hpp"
#include "core/nes/mapper245.hpp"
#include "core/nes/mapper7.hpp"
#include "core/nes/mapper9.hpp"
#include "core/nes/mapper10.hpp"
#include "core/nes/mapper11.hpp"
#include "core/nes/mapper121.hpp"
#include "core/nes/mapper13.hpp"
#include "core/nes/mapper15.hpp"
#include "core/nes/mapper18.hpp"
#include "core/nes/mapper19.hpp"
#include "core/nes/mapper199.hpp"
#include "core/nes/mapper21.hpp"
#include "core/nes/mapper32.hpp"
#include "core/nes/mapper33.hpp"
#include "core/nes/mapper66.hpp"
#include "core/nes/mapper68.hpp"
#include "core/nes/mapper71.hpp"
#include "core/nes/mapper78.hpp"
#include "core/nes/mapper87.hpp"
#include "core/nes/mapper162.hpp"
#include "core/nes/mapper163.hpp"
#include "core/nes/mapper165.hpp"
#include "core/nes/mapper164.hpp"
#include "core/nes/mapper178.hpp"
#include "core/nes/mapper177.hpp"
#include "core/nes/mapper190.hpp"
#include "core/nes/mapper226.hpp"
#include "core/nes/mapper227.hpp"
#include "core/nes/mapper242.hpp"
#include "core/nes/mapper246.hpp"
#include "core/nes/mapper249.hpp"

namespace fc::nes {

namespace {

/// Copy a slice of the file into its own vector.
std::vector<u8> slice(std::span<const u8> rom, std::size_t offset, std::size_t size)
{
    if (offset >= rom.size()) {
        return {};
    }
    const std::size_t available = rom.size() - offset;
    const std::size_t take = (size < available) ? size : available;
    return std::vector<u8>(rom.begin() + static_cast<std::ptrdiff_t>(offset),
                           rom.begin() + static_cast<std::ptrdiff_t>(offset + take));
}

} // namespace

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

std::optional<Cartridge> Cartridge::from_bytes(std::span<const u8> rom, std::string& error)
{
    const InesParseResult parsed = parse_ines_header(rom);
    if (!parsed.ok()) {
        error = parsed.error;
        return std::nullopt;
    }
    const InesHeader header = *parsed.header;

    Cartridge cart;
    cart.header_ = header;

    // 1. the header is 16 bytes, then optionally a trainer.
    std::size_t offset = 16;
    if (header.has_trainer) {
        cart.trainer_ = slice(rom, offset, 512);
        offset += 512;
    }

    // 2. PRG ROM.
    cart.prg_rom_ = slice(rom, offset, header.prg_rom_size());
    offset += header.prg_rom_size();

    // 3. CHR ROM. Zero pages means the cartridge has CHR RAM instead, which
    //    games use to rewrite the pattern tables while they run.
    cart.chr_rom_ = slice(rom, offset, header.chr_rom_size());

    // 4. the mapper.
    switch (header.mapper) {
    case 0: {
        auto mapper = std::make_unique<Mapper0>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 1: {
        auto mapper = std::make_unique<Mapper1>(cart.prg_rom_, cart.chr_rom_);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 2: {   // UxROM: the CHR is always RAM, no matter what the header says.
        auto mapper = std::make_unique<Mapper2>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        mapper->make_chr_ram();
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 3: {   // CNROM
        cart.mapper_ = std::make_unique<Mapper3>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 4: {   // MMC3
        // The MMC3 can only wire the nametables one way or the other, so a
        // four-screen header is meaningless here. Start from vertical and let
        // the game's first $A000 write decide.
        const Mirroring start = (header.mirroring == Mirroring::FourScreen)
                                    ? Mirroring::Vertical
                                    : header.mirroring;
        auto mapper = std::make_unique<Mapper4>(cart.prg_rom_, cart.chr_rom_, start);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 7: {   // AxROM: CHR RAM, and the header mirroring is overridden anyway.
        auto mapper = std::make_unique<Mapper7>(cart.prg_rom_, cart.chr_rom_);
        mapper->make_chr_ram();
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 9: {   // MMC2
        cart.mapper_ = std::make_unique<Mapper9>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 10: {  // MMC4
        cart.mapper_ = std::make_unique<Mapper10>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 11: {  // Color Dreams
        cart.mapper_ = std::make_unique<Mapper11>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 13: {  // CPROM: the mapper owns its own 16KB of CHR RAM.
        cart.mapper_ = std::make_unique<Mapper13>(cart.prg_rom_, header.mirroring);
        break;
    }

    case 15: {  // 100-in-1 multicart: 8KB CHR RAM when the header has no CHR.
        auto mapper = std::make_unique<Mapper15>(cart.prg_rom_, cart.chr_rom_);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 18: {   // Jaleco SS88006
        cart.mapper_ = std::make_unique<Mapper18>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 19: {   // Namco 163
        auto mapper = std::make_unique<Mapper19>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 21:
    case 22:
    case 23:
    case 25: {   // Konami VRC2 / VRC4
        cart.mapper_ = std::make_unique<Mapper21>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring, header.mapper);
        break;
    }

    case 32: {   // IREM G-101
        cart.mapper_ = std::make_unique<Mapper32>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 33: {   // Taito TC0190
        cart.mapper_ = std::make_unique<Mapper33>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 66: {   // GxROM
        auto mapper = std::make_unique<Mapper66>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 68: {   // Sunsoft-4
        cart.mapper_ = std::make_unique<Mapper68>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 71: {   // Codemasters BF909x
        auto mapper = std::make_unique<Mapper71>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 74: {   // Waixing MMC3 with a 2KB CHR RAM window.
        auto mapper = std::make_unique<Mapper74>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 78: {   // Jaleco JF-16
        cart.mapper_ = std::make_unique<Mapper78>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 87: {   // Jaleco JF-13
        cart.mapper_ = std::make_unique<Mapper87>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 121: {  // MMC3 with a protection latch.
        auto mapper = std::make_unique<Mapper121>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 162: {  // Waixing
        auto mapper = std::make_unique<Mapper162>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 163: {   // Nanjing FC-001, the Chinese RPG board. CHR is always RAM.
        auto mapper = std::make_unique<Mapper163>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 164: {  // Waixing
        auto mapper = std::make_unique<Mapper164>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 178: {  // Waixing
        auto mapper = std::make_unique<Mapper178>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 165: {  // MMC2-style CHR latch on an MMC3.
        auto mapper = std::make_unique<Mapper165>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 177: {  // Henggedianzi
        auto mapper = std::make_unique<Mapper177>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 190: {  // Magic Kid Goo Goo
        auto mapper = std::make_unique<Mapper190>(cart.prg_rom_, cart.chr_rom_);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 199: {  // MMC3 with four board registers and CHR RAM by page.
        auto mapper = std::make_unique<Mapper199>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 226: {   // 76-in-1 multicart.
        auto mapper = std::make_unique<Mapper226>(cart.prg_rom_, cart.chr_rom_);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 227: {  // multicart
        auto mapper = std::make_unique<Mapper227>(cart.prg_rom_, cart.chr_rom_);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 241: {  // 32KB PRG window and 8KB of CHR RAM.
        cart.mapper_ = std::make_unique<Mapper241>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        break;
    }

    case 242: {  // Waixing
        auto mapper = std::make_unique<Mapper242>(cart.prg_rom_, cart.chr_rom_);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 245: {  // Waixing MMC3, PRG high bit from CHR register 0.
        auto mapper = std::make_unique<Mapper245>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 246: {  // Chinese multicart
        auto mapper = std::make_unique<Mapper246>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    case 249: {  // MMC3 clone with scrambled banks
        auto mapper = std::make_unique<Mapper249>(
            cart.prg_rom_, cart.chr_rom_, header.mirroring);
        if (header.chr_rom_pages == 0) {
            mapper->make_chr_ram();
        }
        cart.mapper_ = std::move(mapper);
        break;
    }

    default:
        error = "mapper " + std::to_string(header.mapper) +
                " is not implemented yet (0 NROM, 1 MMC1, 2 UxROM, 3 CNROM, "
                "4 MMC3, 7 AxROM, 9 MMC2, 10 MMC4, 11 Color Dreams, "
                "13 CPROM, 15 100-in-1, 18 SS88006, 19 Namco 163, "
                "21/22/23/25 VRC2/4, 32 IREM, 33 Taito, 66 GxROM, "
                "68 Sunsoft-4, 71 Codemasters, 78/87 Jaleco, "
                "162/164/178/242 Waixing, 163 Nanjing, 177 Henggedianzi, "
                "190 Magic Kid Goo Goo, 226 76-in-1, 227/246/249 multicart are)";
        return std::nullopt;
    }

    return cart;
}

// ---------------------------------------------------------------------------
// The CPU's side
// ---------------------------------------------------------------------------

u8 Cartridge::read(u16 address)
{
    if (address <= kExpansionEnd) {
        // The expansion area is where unlicensed boards and carts with extra
        // audio hardware put their registers (the Nanjing board's banks live
        // at $5000). A mapper with nothing there answers 0, which is the
        // same value the old fixed return produced. On real hardware an
        // unused expansion address returns open bus; that subtlety is still
        // left to the bus.
        return mapper_->read_expansion(address);
    }

    if (address <= kPrgRamEnd) {
        if (!mapper_->has_work_ram()) {
            return mapper_->read_expansion(address);
        }
        if (!prg_ram_enabled_) {
            return 0;
        }
        return prg_ram_[static_cast<std::size_t>(address - kPrgRamBase) % prg_ram_.size()];
    }

    u8 value = mapper_->read_prg(address);

    // Game Genie patches act here, on the byte the cartridge returned, at the
    // CPU address -- after banking. An eight letter code only applies when
    // the byte already matches its compare value, which is how two banks that
    // share an address are told apart.
    for (const PrgPatch& patch : prg_patches_) {
        if (patch.address != address) {
            continue;
        }
        if (patch.compare >= 0 && static_cast<u8>(patch.compare) != value) {
            continue;
        }
        value = patch.value;
    }
    return value;
}

void Cartridge::write(u16 address, u8 value)
{
    if (address <= kExpansionEnd) {
        mapper_->write_expansion(address, value);
        return;
    }

    if (address <= kPrgRamEnd) {
        if (!mapper_->has_work_ram()) {
            mapper_->write_expansion(address, value);
            return;
        }
        if (!prg_ram_enabled_) {
            return;
        }
        prg_ram_[static_cast<std::size_t>(address - kPrgRamBase) % prg_ram_.size()] = value;
        return;
    }

    // On NROM this goes nowhere. On a banking cartridge this is where the
    // mapper listens: the address and value are the bank switch command.
    mapper_->write_prg(address, value);
}

// ---------------------------------------------------------------------------
// The PPU's side
// ---------------------------------------------------------------------------

u8 Cartridge::read_chr(u16 address)
{
    return mapper_->read_chr(address);
}

void Cartridge::write_chr(u16 address, u8 value)
{
    mapper_->write_chr(address, value);
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

std::string Cartridge::summary() const
{
    std::string out;
    out += "mapper " + std::to_string(header_.mapper);
    out += ", ";
    out += std::to_string(header_.prg_rom_pages) + "x16KB PRG";
    out += " (" + std::to_string(prg_rom_.size()) + " bytes)";

    if (header_.chr_rom_pages == 0) {
        out += ", 8KB CHR RAM";
    } else {
        out += ", " + std::to_string(header_.chr_rom_pages) + "x8KB CHR";
        out += " (" + std::to_string(chr_rom_.size()) + " bytes)";
    }

    out += ", ";
    out += mirroring_name(header_.mirroring);

    if (header_.has_battery) {
        out += ", battery";
    }
    if (header_.has_trainer) {
        out += ", trainer";
    }
    return out;
}

} // namespace fc::nes
