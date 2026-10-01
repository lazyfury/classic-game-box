#include "core/cpu/cpu.hpp"
#include "core/nes/bus.hpp"
#include "core/nes/controller.hpp"
#include "core/nes/ines.hpp"
#include "core/nes/ram_cartridge.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

#include <array>
#include <cstddef>
#include <initializer_list>
#include <string>
#include <vector>

using namespace fc;

namespace {

using Button = nes::Controller::Button;

struct Machine {
    nes::RamCartridge cart;
    nes::NesBus bus;
    Cpu cpu{ bus };

    Machine() { bus.set_cartridge(&cart); }

    void load(std::initializer_list<u8> program, u16 origin = 0x8000)
    {
        const std::vector<u8> bytes(program);
        bus.ram().clear();
        cart.clear();
        cart.load(bytes, origin);
        cart.write(0xFFFC, static_cast<u8>(origin & 0x00FF));
        cart.write(0xFFFD, static_cast<u8>(origin >> 8));
        cpu.reset();
    }
};

} // namespace

// ===========================================================================
// The serial protocol
// ===========================================================================

TEST(Controller, NothingPressedReadsAllZeroesThenOnes)
{
    nes::Controller c;
    c.release_all();

    c.strobe(true);
    c.strobe(false);

    for (int i = 0; i < 8; ++i) {
        EXPECT_EQ(c.read(), 0) << "button " << i << " is not pressed";
    }
    for (int i = 0; i < 4; ++i) {
        EXPECT_EQ(c.read(), 1) << "after the eighth read the line is pulled high";
    }
}

TEST(Controller, TheButtonsComeOutInTheWiredOrder)
{
    // Press A and Right, which are the first and last bits out.
    nes::Controller c;
    c.set_button(Button::A, true);
    c.set_button(Button::Right, true);

    c.strobe(true);
    c.strobe(false);

    const bool expected[nes::Controller::kButtonCount] = {
        true, false, false, false, false, false, false, true,
    };

    for (int i = 0; i < nes::Controller::kButtonCount; ++i) {
        EXPECT_EQ(c.read() != 0, expected[i])
            << "read " << i << " should be " << button_name(static_cast<Button>(i));
    }
}

TEST(Controller, EveryButtonAppearsExactlyOnce)
{
    // Press each button on its own and confirm it lands on the right clock.
    for (int pressed = 0; pressed < nes::Controller::kButtonCount; ++pressed) {
        nes::Controller c;
        c.set_button(static_cast<Button>(pressed), true);

        c.strobe(true);
        c.strobe(false);

        for (int clock = 0; clock < nes::Controller::kButtonCount; ++clock) {
            const u8 bit = c.read();
            EXPECT_EQ(bit, clock == pressed ? 1 : 0)
                << button_name(static_cast<Button>(pressed))
                << " on clock " << clock;
        }
    }
}

TEST(Controller, HoldingTheStrobeReturnsTheFirstButtonRepeatedly)
{
    // While the latch is held the shift register reloads every time, so the
    // line never advances. This is how a game can wait for a button without
    // having to strobe again.
    nes::Controller c;
    c.set_button(Button::Start, true);
    c.set_button(Button::B, true);

    c.strobe(true);

    for (int i = 0; i < 5; ++i) {
        EXPECT_EQ(c.read(), 0) << "A is not pressed, and the latch has not moved";
    }

    c.set_button(Button::A, true);
    for (int i = 0; i < 5; ++i) {
        EXPECT_EQ(c.read(), 1) << "A is pressed now and the latch is still loading";
    }
}

TEST(Controller, TheStrobeTakesASnapshot)
{
    // The buttons are latched, not followed. Changing them mid-read does not
    // change the eight bits already waiting.
    nes::Controller c;
    c.set_button(Button::A, true);

    c.strobe(true);
    c.strobe(false);

    c.set_button(Button::A, false);
    c.set_button(Button::Right, true);

    EXPECT_EQ(c.read(), 1) << "still the latched A";
    for (int i = 1; i < 7; ++i) {
        EXPECT_EQ(c.read(), 0) << "clock " << i;
    }
    EXPECT_EQ(c.read(), 0) << "the Right press happened after the latch";
}

TEST(Controller, RestrobingRecapturesTheButtons)
{
    nes::Controller c;
    c.set_button(Button::A, true);
    c.strobe(true);
    c.strobe(false);
    (void)c.read();

    // Change the buttons, then latch again.
    c.set_button(Button::A, false);
    c.set_button(Button::B, true);
    c.strobe(true);
    c.strobe(false);

    EXPECT_EQ(c.read(), 0) << "A was released before the second latch";
    EXPECT_EQ(c.read(), 1) << "B was pressed before the second latch";
}

TEST(Controller, ResetKeepsTheButtonsButClearsTheLatch)
{
    nes::Controller c;
    c.set_button(Button::Start, true);
    c.strobe(true);
    c.strobe(false);
    (void)c.read();

    c.reset();

    EXPECT_TRUE(c.button(Button::Start)) << "the player is still holding it";
    EXPECT_FALSE(c.is_strobing());
}

TEST(Controller, AllEightAtOnce)
{
    nes::Controller c;
    c.set_buttons(0xFF);
    c.strobe(true);
    c.strobe(false);
    for (int i = 0; i < 8; ++i) {
        EXPECT_EQ(c.read(), 1) << "clock " << i;
    }
}

// ===========================================================================
// The ports on the bus
// ===========================================================================

TEST(ControllerBus, PortOneIsReadAt4016)
{
    nes::NesBus bus;
    bus.controller(0).set_button(Button::A, true);

    bus.write(0x4016, 0x01);   // latch
    bus.write(0x4016, 0x00);   // start shifting

    EXPECT_EQ(bus.read(0x4016) & 0x01, 1) << "A";
    EXPECT_EQ(bus.read(0x4016) & 0x01, 0) << "B";
}

TEST(ControllerBus, PortTwoIsReadAt4017)
{
    nes::NesBus bus;
    bus.controller(1).set_button(Button::B, true);

    bus.write(0x4016, 0x01);
    bus.write(0x4016, 0x00);

    EXPECT_EQ(bus.read(0x4017) & 0x01, 0) << "A";
    EXPECT_EQ(bus.read(0x4017) & 0x01, 1) << "B";
}

TEST(ControllerBus, TheStrobeReachesBothPorts)
{
    // The two ports share one latch wire, so a write to $4016 latches both.
    nes::NesBus bus;
    bus.controller(0).set_button(Button::Right, true);
    bus.controller(1).set_button(Button::Right, true);

    bus.write(0x4016, 0x01);
    bus.write(0x4016, 0x00);

    for (int i = 0; i < 7; ++i) {
        (void)bus.read(0x4016);
        (void)bus.read(0x4017);
    }
    EXPECT_EQ(bus.read(0x4016) & 0x01, 1) << "port 1, Right";
    EXPECT_EQ(bus.read(0x4017) & 0x01, 1) << "port 2, Right";
}

TEST(ControllerBus, PortTwoWritesDoNotStrobe)
{
    // $4017's WRITE is the APU frame counter, not a controller latch. Only
    // $4016 has the strobe.
    nes::NesBus bus;
    bus.controller(0).set_button(Button::A, true);

    bus.write(0x4017, 0x01);
    bus.write(0x4017, 0x00);

    EXPECT_FALSE(bus.controller(0).is_strobing());
}

TEST(ControllerBus, ThePortsShiftIndependently)
{
    nes::NesBus bus;
    bus.controller(0).set_button(Button::A, true);
    bus.controller(1).set_button(Button::Right, true);

    bus.write(0x4016, 0x01);
    bus.write(0x4016, 0x00);

    EXPECT_EQ(bus.read(0x4016) & 0x01, 1) << "port 1 first bit is A";
    EXPECT_EQ(bus.read(0x4017) & 0x01, 0) << "port 2 first bit is A, not pressed";
}

TEST(ControllerBus, TheUpperBitsAreOpenBus)
{
    // Only bit 0 is wired from the controller port. Programs must mask, and
    // a program that does not will see whatever was on the bus.
    nes::NesBus bus;
    bus.write(0x0000, 0xA0);   // put a recognisable byte on the data bus
    (void)bus.read(0x0000);

    const u8 value = bus.read(0x4016);
    EXPECT_EQ(value & 0xFE, 0xA0) << "the high seven bits come from the bus";
}

TEST(ControllerBus, OpenBusStartsAtZeroAndNothingIsPressed)
{
    nes::NesBus bus;
    EXPECT_EQ(bus.read(0x4016), 0x00);
    EXPECT_EQ(bus.read(0x4017), 0x00);
}

// ===========================================================================
// A program that actually reads a controller
// ===========================================================================

TEST(ControllerCpu, AProgramCanReadAllEightButtonsIntoAZeroPageByte)
{
    // The classic NES idiom, the same shape Super Mario Bros uses:
    //
    //      LDA #$01
    //      STA $4016     ; latch
    //      LDA #$00
    //      STA $4016     ; start shifting
    //      LDX #$08
    // loop LDA $4016
    //      LSR A         ; bit 0 into the carry
    //      ROL $10       ; carry into the bottom of $10, everything up one
    //      DEX
    //      BNE loop
    //
    // The first button read ends up in bit 7 and the last in bit 0.
    Machine m;
    m.load({
        0xA9, 0x01,   // $8000  LDA #$01
        0x8D, 0x16, 0x40,   // $8002  STA $4016
        0xA9, 0x00,   // $8005  LDA #$00
        0x8D, 0x16, 0x40,   // $8007  STA $4016
        0xA2, 0x08,   // $800A  LDX #$08
        0xAD, 0x16, 0x40,   // $800C  LDA $4016   <- loop
        0x4A,         // $800F  LSR A
        0x26, 0x10,   // $8010  ROL $10
        0xCA,         // $8012  DEX
        0xD0, 0xF7,   // $8013  BNE $800C
    });

    m.bus.controller(0).set_button(Button::Start, true);

    m.cpu.run(200);

    // A is bit 7, B bit 6, Select 5, Start 4, Up 3, Down 2, Left 1, Right 0.
    EXPECT_EQ(m.bus.ram().read(0x0010), 0x10)
        << "only Start, which is bit 4 after the rotate";
}

TEST(ControllerCpu, TheProgramSeesWhateverIsHeldAtTheLatch)
{
    Machine m;
    m.load({
        0xA9, 0x01, 0x8D, 0x16, 0x40,
        0xA9, 0x00, 0x8D, 0x16, 0x40,
        0xA2, 0x08,
        0xAD, 0x16, 0x40,
        0x4A,
        0x26, 0x10,
        0xCA,
        0xD0, 0xF7,
    });

    // A, B, Select and Start all pressed at once.
    m.bus.controller(0).set_button(Button::A, true);
    m.bus.controller(0).set_button(Button::B, true);
    m.bus.controller(0).set_button(Button::Select, true);
    m.bus.controller(0).set_button(Button::Start, true);

    m.cpu.run(200);

    EXPECT_EQ(m.bus.ram().read(0x0010), 0xF0) << "the top four bits";
}

TEST(ControllerCpu, TheDirectionPadBitsAreInTheRightOrder)
{
    Machine m;
    m.load({
        0xA9, 0x01, 0x8D, 0x16, 0x40,
        0xA9, 0x00, 0x8D, 0x16, 0x40,
        0xA2, 0x08,
        0xAD, 0x16, 0x40,
        0x4A,
        0x26, 0x10,
        0xCA,
        0xD0, 0xF7,
    });

    // Up and Right: the two ends of the direction pad.
    m.bus.controller(0).set_button(Button::Up, true);
    m.bus.controller(0).set_button(Button::Right, true);

    m.cpu.run(200);

    // Up is clock 4 -> bit 3, Right is clock 7 -> bit 0.
    EXPECT_EQ(m.bus.ram().read(0x0010), 0x09);
}

TEST(ControllerCpu, HoldingAButtonAcrossFramesKeepsReadingIt)
{
    // The controller has no notion of a frame. A program that strobes and
    // reads every frame sees the button every frame for as long as it is held.
    Machine m;
    m.load({
        0xA9, 0x01, 0x8D, 0x16, 0x40,
        0xA9, 0x00, 0x8D, 0x16, 0x40,
        0xA2, 0x08,
        0xAD, 0x16, 0x40,
        0x4A,
        0x26, 0x10,
        0xCA,
        0xD0, 0xF7,
        0x4C, 0x00, 0x80,   // JMP $8000, read it again forever
    });

    m.bus.controller(0).set_button(Button::A, true);

    // One full pass is 5 setup instructions, 8 iterations of 5, and the JMP.
    constexpr int kOnePass = 5 + 8 * 5 + 1;

    for (int i = 0; i < 5; ++i) {
        m.bus.ram().write(0x0010, 0x00);
        m.cpu.run(kOnePass);
        EXPECT_EQ(m.bus.ram().read(0x0010), 0x80)
            << "A is still held on pass " << i;
    }
}
