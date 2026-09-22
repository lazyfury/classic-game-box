#pragma once

// ---------------------------------------------------------------------------
// Cheats: a byte, at an address, put back when the game overwrites it.
//
// This is the whole of the mechanism, and it is deliberately this small. A
// cheat in the sense a player means ("more lives", "full health") is not code
// that runs; it is one byte of console RAM that the game keeps rewriting, and
// the only thing worth doing about it is writing it again.
//
//   poke    write the value once, now
//   freeze  write the value at the start of every frame
//
// Which byte, and what value, is per game and is the player's business. The
// emulator does not know what lives are, and giving it a way to find out would
// mean a table of games and addresses -- a database, not an emulator. Finding
// the address is what a cheat *search* is for, and that is a front end job on
// top of the `peek` this layer provides.
//
// Why through the bus
// -------------------
// `apply` writes through the same Bus the CPU uses, so address decoding is not
// duplicated here: $075A and $0F5A are the same byte on a real NES, and a
// cheat written against one of them has to land on the other. Writing into the
// RAM array directly would quietly break that, and it would be the kind of bug
// that only shows up in one game.
// ---------------------------------------------------------------------------

#include "core/bus.hpp"
#include "core/types.hpp"

#include <cstddef>
#include <span>
#include <vector>

namespace fc::nes {

/** One cheat: an address, a value, and whether to keep putting it back. */
struct Cheat {
    u16 address = 0;
    u8 value = 0;

    /**
     * Write it at the start of every frame.
     *
     * A one-shot poke is enough for a value the game sets once and reads once.
     * It is not enough for one the game rewrites -- which is most of them, and
     * all of the interesting ones -- so this is what a player almost always
     * wants and what the panel sets by default.
     */
    bool freeze = false;

    /** A cheat that is remembered but switched off. */
    bool enabled = true;
};

/**
 * The list, and the one operation that matters: put it back.
 *
 * Deliberately dumb, and deliberately not serialized into a save state: a
 * cheat describes what the player asked for, not what the machine is doing,
 * and a state loaded from disk should not silently turn somebody's cheats off
 * or on.
 */
class CheatSet {
public:
    void set(std::span<const Cheat> cheats) { cheats_.assign(cheats.begin(), cheats.end()); }
    void clear() noexcept { cheats_.clear(); }

    [[nodiscard]] std::size_t size() const noexcept { return cheats_.size(); }
    [[nodiscard]] bool empty() const noexcept { return cheats_.empty(); }
    [[nodiscard]] const Cheat& at(std::size_t index) const { return cheats_[index]; }
    [[nodiscard]] std::span<const Cheat> all() const noexcept { return cheats_; }

    /**
     * Write every enabled freeze cheat, through `bus`.
     *
     * Called at the start of a frame. Writing once per frame rather than once
     * per instruction is the trade every emulator makes: a cheat that needs
     * to survive within a single frame is rare, and a write on every
     * instruction would be a write the game can see between two of its own.
     */
    void apply(Bus& bus) const
    {
        for (const Cheat& cheat : cheats_) {
            if (cheat.enabled && cheat.freeze) {
                bus.write(cheat.address, cheat.value);
            }
        }
    }

private:
    std::vector<Cheat> cheats_;
};

} // namespace fc::nes
