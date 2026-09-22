// ---------------------------------------------------------------------------
// Everywhere else: no pads.
//
// This exists so that the portable half of the C++ helper -- the protocol, the
// slot bookkeeping, the change detection -- can be built, run and tested on a
// machine that is not Windows. It reports nothing at 60Hz, which is the honest
// answer when the reading is done by another program.
//
// macOS does not use this build at all: it builds the Swift helper, which
// talks to the GameController framework (see native/build.sh and
// native/gamepad/README.md). Linux has no backend yet; a `.js`/event-device
// reader would be the next one, and it would slot in here without touching
// anything above.
// ---------------------------------------------------------------------------

#ifndef _WIN32

#include "backend.h"

namespace fc {
namespace {

class EmptyBackend final : public Backend {
public:
    std::vector<PadSample> poll() override { return {}; }
};

}  // namespace

std::unique_ptr<Backend> makeBackend() {
    return std::make_unique<EmptyBackend>();
}

}  // namespace fc

#endif  // !_WIN32
