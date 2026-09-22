#include "core/cpu/cpu.hpp"
#include "core/nes/bus.hpp"
#include "core/nes/ram.hpp"
#include "core/nes/ram_cartridge.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

#include <array>
#include <cstddef>
#include <vector>

using namespace fc;

namespace {

/// Records which PPU register each access selected.
struct FakePpu : nes::Device {
    std::array<u8, 8> registers{};
    std::vector<u8> written_indices;
    std::vector<u8> read_indices;

    [[nodiscard]] u8 read(u16 address) override
    {
        const u8 index = nes::NesBus::ppu_register_index(address);
        read_indices.push_back(index);
        return registers[index];
    }

    void write(u16 address, u8 value) override
    {
        const u8 index = nes::NesBus::ppu_register_index(address);
        written_indices.push_back(index);
        registers[index] = value;
    }
};

/// Catches OAM DMA transfers.
struct FakeOam : nes::OamTarget {
    std::array<u8, 256> bytes{};
    int writes = 0;

    void write_oam(u8 index, u8 value) override
    {
        bytes[index] = value;
        ++writes;
    }
};

/// A NES with a RAM cartridge in the slot.
struct Machine {
    nes::RamCartridge cart;
    nes::NesBus bus;
    Cpu cpu{ bus };

    Machine() { bus.set_cartridge(&cart); }

    void load(std::vector<u8> program, u16 origin = 0x8000)
    {
        bus.ram().clear();
        cart.clear();
        cart.load(program, origin);
        cart.write(0xFFFC, static_cast<u8>(origin & 0x00FF));
        cart.write(0xFFFD, static_cast<u8>(origin >> 8));
        cpu.reset();
    }
};

} // namespace

// ===========================================================================
// Address decoding
// ===========================================================================

TEST(NesBus, RegionOfMatchesTheMemoryMap)
{
    using Region = nes::NesBus::Region;

    EXPECT_EQ(nes::NesBus::region_of(0x0000), Region::Ram);
    EXPECT_EQ(nes::NesBus::region_of(0x07FF), Region::Ram);
    EXPECT_EQ(nes::NesBus::region_of(0x0800), Region::Ram) << "still RAM, mirrored";
    EXPECT_EQ(nes::NesBus::region_of(0x1FFF), Region::Ram) << "the last mirror";

    EXPECT_EQ(nes::NesBus::region_of(0x2000), Region::PpuRegisters);
    EXPECT_EQ(nes::NesBus::region_of(0x2007), Region::PpuRegisters);
    EXPECT_EQ(nes::NesBus::region_of(0x2008), Region::PpuRegisters) << "mirrored";
    EXPECT_EQ(nes::NesBus::region_of(0x3FFF), Region::PpuRegisters);

    EXPECT_EQ(nes::NesBus::region_of(0x4000), Region::ApuAndIo);
    EXPECT_EQ(nes::NesBus::region_of(0x4014), Region::ApuAndIo) << "OAM DMA";
    EXPECT_EQ(nes::NesBus::region_of(0x4017), Region::ApuAndIo);

    EXPECT_EQ(nes::NesBus::region_of(0x4018), Region::Disabled);
    EXPECT_EQ(nes::NesBus::region_of(0x401F), Region::Disabled);

    EXPECT_EQ(nes::NesBus::region_of(0x4020), Region::Cartridge);
    EXPECT_EQ(nes::NesBus::region_of(0x8000), Region::Cartridge);
    EXPECT_EQ(nes::NesBus::region_of(0xFFFF), Region::Cartridge);
}

// ===========================================================================
// RAM mirroring: the Phase 2 completion criterion
// ===========================================================================

TEST(NesBus, AWriteIsVisibleThroughEveryMirror)
{
    // $0000-$1FFF is 8KB feeding a 2KB chip. Every cell is therefore
    // reachable at four different addresses.
    nes::NesBus bus;

    for (u16 offset = 0; offset < 0x0800; ++offset) {
        bus.ram().clear();
        bus.write(offset, 0xAA);

        EXPECT_EQ(bus.read(static_cast<u16>(0x0000 + offset)), 0xAA) << "mirror 0";
        EXPECT_EQ(bus.read(static_cast<u16>(0x0800 + offset)), 0xAA) << "mirror 1";
        EXPECT_EQ(bus.read(static_cast<u16>(0x1000 + offset)), 0xAA) << "mirror 2";
        EXPECT_EQ(bus.read(static_cast<u16>(0x1800 + offset)), 0xAA) << "mirror 3";
    }
}

TEST(NesBus, EveryByteInTheMirroredRangeAgreesWithTheRamChip)
{
    nes::NesBus bus;

    for (u16 i = 0; i < nes::Ram::kSize; ++i) {
        bus.write(i, static_cast<u8>(i * 7u + 3u));
    }

    for (u16 address = 0x0000; address < 0x2000; ++address) {
        const u8 expected = static_cast<u8>((address & nes::Ram::kMask) * 7u + 3u);
        EXPECT_EQ(bus.read(address), expected)
            << "address " << std::hex << address;
    }
}

TEST(NesBus, MirroringIsIndependentOfTheWrittenValue)
{
    // Write a different value at each of the four mirrors and check that the
    // last one wins, from every one of the four.
    nes::NesBus bus;

    bus.write(0x0010, 0x11);
    bus.write(0x0810, 0x22);
    bus.write(0x1010, 0x33);
    bus.write(0x1810, 0x44);

    EXPECT_EQ(bus.read(0x0010), 0x44);
    EXPECT_EQ(bus.read(0x0810), 0x44);
    EXPECT_EQ(bus.read(0x1010), 0x44);
    EXPECT_EQ(bus.read(0x1810), 0x44);
}

TEST(NesBus, TheCartridgeIsNotAliasedIntoRam)
{
    Machine m;
    m.bus.write(0x0010, 0xAA);
    m.bus.write(0x8010, 0xBB);

    EXPECT_EQ(m.bus.read(0x0010), 0xAA);
    EXPECT_EQ(m.bus.read(0x8010), 0xBB);
}

// ===========================================================================
// PPU register mirroring
// ===========================================================================

TEST(NesBus, OnlyThreeAddressLinesReachThePpu)
{
    EXPECT_EQ(nes::NesBus::ppu_register_index(0x2000), 0);
    EXPECT_EQ(nes::NesBus::ppu_register_index(0x2001), 1);
    EXPECT_EQ(nes::NesBus::ppu_register_index(0x2007), 7);
    EXPECT_EQ(nes::NesBus::ppu_register_index(0x2008), 0) << "wraps";
    EXPECT_EQ(nes::NesBus::ppu_register_index(0x200F), 7);
    EXPECT_EQ(nes::NesBus::ppu_register_index(0x3FFF), 7);
}

TEST(NesBus, PpuRegistersMirrorAcrossTheWholeEightKilobyteRange)
{
    nes::NesBus bus;
    FakePpu ppu;
    bus.set_ppu(&ppu);

    // 8KB of address space reaching 8 registers: each one answers 1024 times.
    for (u16 address = 0x2000; address < 0x4000; ++address) {
        bus.write(address, static_cast<u8>(address & 0x0007));
    }

    EXPECT_EQ(ppu.written_indices.size(), 0x2000u);

    for (std::size_t i = 0; i < ppu.written_indices.size(); ++i) {
        EXPECT_EQ(ppu.written_indices[i], static_cast<u8>(i & 0x0007));
    }

    for (u8 index = 0; index < 8; ++index) {
        EXPECT_EQ(ppu.registers[index], index);
    }
}

// ===========================================================================
// Open bus
// ===========================================================================

TEST(NesBus, UnmappedReadsReturnTheLastValueOnTheDataBus)
{
    nes::NesBus bus;

    bus.write(0x4018, 0x5A);   // disabled region: nothing stores it
    EXPECT_EQ(bus.read(0x4018), 0x5A) << "open bus, not zero";

    bus.write(0x0000, 0x11);   // a real write still drives the bus
    EXPECT_EQ(bus.read(0x4019), 0x11);

    bus.write(0x2000, 0x22);   // no PPU attached, so nothing latches it
    EXPECT_EQ(bus.read(0x401A), 0x22);
}

TEST(NesBus, OpenBusStartsAtZero)
{
    nes::NesBus bus;
    EXPECT_EQ(bus.open_bus(), 0);
    EXPECT_EQ(bus.read(0x4018), 0);
}

TEST(NesBus, ARealReadAlsoDrivesTheBus)
{
    nes::NesBus bus;
    bus.write(0x0000, 0x99);
    EXPECT_EQ(bus.read(0x0000), 0x99);
    EXPECT_EQ(bus.open_bus(), 0x99);
    EXPECT_EQ(bus.read(0x4018), 0x99);
}

// ===========================================================================
// OAM DMA
// ===========================================================================

TEST(NesBus, OamDmaCopiesAFullPage)
{
    nes::NesBus bus;
    FakeOam oam;
    bus.set_oam_target(&oam);

    for (u16 i = 0; i < 256; ++i) {
        bus.write(static_cast<u16>(0x0200 + i), static_cast<u8>(i ^ 0x5A));
    }

    bus.write(0x4014, 0x02);   // DMA from page $02

    EXPECT_EQ(bus.oam_dma_count(), 1);
    EXPECT_EQ(oam.writes, 256);

    for (int i = 0; i < 256; ++i) {
        EXPECT_EQ(oam.bytes[static_cast<std::size_t>(i)],
                  static_cast<u8>(i ^ 0x5A));
    }
}

TEST(NesBus, OamDmaStallsTheCpuForFiveHundredThirteenCycles)
{
    nes::NesBus bus;
    FakeOam oam;
    bus.set_oam_target(&oam);

    bus.write(0x4014, 0x02);

    EXPECT_EQ(bus.take_stall_cycles(), 513);
    EXPECT_EQ(bus.take_stall_cycles(), 0) << "the stalls are drained once";
}

TEST(NesBus, OamDmaReadsThroughTheNormalAddressPath)
{
    // Page $00 is real RAM. Page $20 would land on the PPU registers.
    // This proves the DMA engine uses the memory map, not a private copy.
    nes::NesBus bus;
    FakeOam oam;
    FakePpu ppu;
    bus.set_oam_target(&oam);
    bus.set_ppu(&ppu);

    ppu.registers[0] = 0x77;

    bus.write(0x4014, 0x20);   // source page $20

    EXPECT_EQ(oam.writes, 256);
    EXPECT_EQ(oam.bytes[0], 0x77) << "read from $2000, which is PPU register 0";
    EXPECT_EQ(ppu.read_indices.size(), 256u);
}

TEST(NesBus, OamDmaWithoutATargetStillStalls)
{
    // The stall is a property of the bus, not of the PPU being present.
    nes::NesBus bus;
    bus.write(0x4014, 0x02);
    EXPECT_EQ(bus.take_stall_cycles(), 513);
}

// ===========================================================================
// The CPU on the real bus
// ===========================================================================

TEST(NesBus, TheCpuWaitsForOamDma)
{
    Machine m;

    // LDA #$02 ; STA $4014
    m.load({ 0xA9, 0x02, 0x8D, 0x14, 0x40 });

    FakeOam oam;
    m.bus.set_oam_target(&oam);

    m.cpu.step();   // LDA #$02
    EXPECT_EQ(m.cpu.total_cycles(), 2u);

    const int cycles = m.cpu.step();   // STA $4014
    EXPECT_EQ(cycles, 4 + 513) << "the store, plus the DMA stall the bus asked for";
    EXPECT_EQ(m.cpu.total_cycles(), 2u + 4u + 513u);
    EXPECT_EQ(oam.writes, 256);
}

TEST(NesBus, RunsAProgramThroughTheRealMemoryMap)
{
    Machine m;

    // The program lives in cartridge space. Its variables live in real RAM.
    // Its stack lives in page 1. All three go through the decoder.
    m.load({
        0xA9, 0x2A,        // $8000  LDA #$2A
        0x85, 0x10,        // $8002  STA $10       zero page
        0xA2, 0x05,        // $8004  LDX #$05
        0x95, 0x20,        // $8006  STA $20,X     -> $0025
        0x48,              // $8008  PHA           stack, page 1
        0x4C, 0x00, 0x80,  // $8009  JMP $8000
    });

    m.cpu.run(5);

    EXPECT_EQ(m.bus.ram().read(0x0010), 0x2A);
    EXPECT_EQ(m.bus.ram().read(0x0025), 0x2A);
    EXPECT_EQ(m.bus.read(0x01FD), 0x2A) << "page 1, the stack";
    EXPECT_EQ(m.bus.read(0x0810), 0x2A) << "the same cell through its mirror";
    EXPECT_EQ(m.bus.read(0x1010), 0x2A);
}

TEST(NesBus, TheResetVectorComesThroughTheCartridgeSlot)
{
    Machine m;
    m.cart.write(0xFFFC, 0x34);
    m.cart.write(0xFFFD, 0x12);
    m.cpu.reset();

    EXPECT_EQ(m.cpu.registers().pc, 0x1234);
}

TEST(NesBus, TheStackStillWorksThroughTheDecoder)
{
    Machine m;
    m.load({ 0xEA });

    m.cpu.push(0x11);
    m.cpu.push(0x22);

    EXPECT_EQ(m.cpu.pop(), 0x22);
    EXPECT_EQ(m.cpu.pop(), 0x11);

    // SP starts at $FD, so the two pushes landed at $01FD and $01FC.
    EXPECT_EQ(m.bus.ram().read(0x01FD), 0x11) << "page 1 is inside the RAM chip";
    EXPECT_EQ(m.bus.ram().read(0x01FC), 0x22);
}
