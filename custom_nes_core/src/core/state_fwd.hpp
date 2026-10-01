#pragma once

// ---------------------------------------------------------------------------
// A forward declaration of StateAccess, and nothing else.
//
// The classes that take part in a save state -- the CPU, the bus, the PPU, the
// APU and its channels, the cartridge, the controllers -- each declare
//
//     friend struct fc::StateAccess;
//
// so that src/core/state.cpp can read their private members. A friend
// declaration that names a qualified type needs that type to already exist, so
// this header is what makes the line above legal without pulling the whole
// serializer into every one of them.
//
// Including this is a small statement: "this class has state worth saving".
// ---------------------------------------------------------------------------

namespace fc {

struct StateAccess;

} // namespace fc
