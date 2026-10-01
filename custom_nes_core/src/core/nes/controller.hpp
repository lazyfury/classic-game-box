#pragma once

// ---------------------------------------------------------------------------
// The standard NES controller: eight buttons read one bit at a time.
//
// The console only has a few wires to the controller port, so the buttons are
// not read in parallel. Inside the controller there is a 4021 shift register
// holding all eight states at once, and the CPU clocks them out one bit per
// read:
//
//     write $4016 = 1     latch: the shift register is loaded from the buttons
//     write $4016 = 0     stop latching; the register keeps what it has
//
//     read  $4016 bit 0   A
//     read  $4016 bit 0   B
//     read  $4016 bit 0   Select
//     read  $4016 bit 0   Start
//     read  $4016 bit 0   Up
//     read  $4016 bit 0   Down
//     read  $4016 bit 0   Left
//     read  $4016 bit 0   Right
//     read  $4016 bit 0   1, and every read after that
//
// The ORDER IS FIXED BY THE WIRING. The buttons are soldered to specific
// inputs of the shift register; nothing can change which bit comes out when.
// Every NES program that reads a controller depends on this exact order.
//
// The trailing 1 is not padding. A program reads nine times and checks that
// the ninth is 1 to tell a standard controller from something else in the
// port. Reading it is how games like Duck Hunt decide whether the light gun
// is plugged in.
//
// Note what is NOT here: no interrupts, no timing, no "was it pressed this
// frame". The controller is a piece of wire with a latch on it. Everything
// about debouncing, repeat rates and button combinations happens in software,
// in the game, and that is why the same hardware feels different in different
// games.
// ---------------------------------------------------------------------------

#include "core/state_fwd.hpp"
#include "core/types.hpp"

namespace fc::nes {

class Controller {
public:
    enum class Button : u8 {
        A = 0,
        B = 1,
        Select = 2,
        Start = 3,
        Up = 4,
        Down = 5,
        Left = 6,
        Right = 7,
    };

    static constexpr int kButtonCount = 8;

    // -- what the player is doing, set from outside --------------------------

    void set_button(Button button, bool pressed) noexcept
    {
        const u8 mask = static_cast<u8>(1u << static_cast<u8>(button));
        buttons_ = pressed ? static_cast<u8>(buttons_ | mask)
                           : static_cast<u8>(buttons_ & static_cast<u8>(~mask));
    }

    [[nodiscard]] bool button(Button button) const noexcept
    {
        return ((buttons_ >> static_cast<u8>(button)) & 1u) != 0;
    }

    /// All eight at once, for scripted input. Bit N is `Button` N.
    void set_buttons(u8 mask) noexcept { buttons_ = mask; }
    [[nodiscard]] u8 buttons() const noexcept { return buttons_; }

    void release_all() noexcept { buttons_ = 0; }

    // -- the wire protocol ---------------------------------------------------

    /// Write to $4016 bit 0.
    void strobe(bool high) noexcept
    {
        strobing_ = high;
        if (high) {
            // While the latch is held the register is continuously reloaded,
            // so reading during a strobe always returns A.
            shift_ = buttons_;
        }
    }

    [[nodiscard]] bool is_strobing() const noexcept { return strobing_; }

    /// Read $4016/$4017 bit 0. The other seven bits are the bus's business.
    [[nodiscard]] u8 read() noexcept
    {
        if (strobing_) {
            return static_cast<u8>(buttons_ & 0x01u);
        }

        ++reads_;
        const u8 value = static_cast<u8>(shift_ & 0x01u);
        // Shift in a 1 at the top. After eight reads the register is all 1s,
        // which is what the ninth and later reads return.
        shift_ = static_cast<u8>((shift_ >> 1) | 0x80u);
        return value;
    }

    void reset() noexcept
    {
        shift_ = buttons_;
        strobing_ = false;
        reads_ = 0;
    }

    [[nodiscard]] u8 shift_register() const noexcept { return shift_; }

    /// How many times the port has been clocked since reset. A game that
    /// polls its controller every frame makes this grow steadily; a game that
    /// has stopped reading it makes it stop. That is a cheap way to tell
    /// whether input is being sampled at all.
    [[nodiscard]] u64 read_count() const noexcept { return reads_; }

private:
    u8 buttons_ = 0;
    u8 shift_ = 0;
    bool strobing_ = false;
    u64 reads_ = 0;

    friend struct fc::StateAccess;
};

[[nodiscard]] constexpr const char* button_name(Controller::Button button) noexcept
{
    switch (button) {
    case Controller::Button::A:      return "A";
    case Controller::Button::B:      return "B";
    case Controller::Button::Select: return "Select";
    case Controller::Button::Start:  return "Start";
    case Controller::Button::Up:     return "Up";
    case Controller::Button::Down:   return "Down";
    case Controller::Button::Left:   return "Left";
    case Controller::Button::Right:  return "Right";
    }
    return "?";
}

} // namespace fc::nes
