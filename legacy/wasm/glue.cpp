// ---------------------------------------------------------------------------
// The WebAssembly build's one C++ file.
//
// Everything the emulator does is already in packages/fc-core/src/core, behind the C interface
// in packages/fc-core/src/ffi/emulator_api.h. This file adds nothing to the emulator. It exists
// for two smaller reasons:
//
//   1. A linker needs at least one input, and it gives us a place to hang the
//      exported function list.
//
//   2. A stale .wasm and a newer JavaScript wrapper must not silently
//      disagree. A browser will happily load a two month old module and then
//      call into it with the wrong assumptions, and the failure shows up as a
//      garbled picture rather than an error. So the wrapper asks this module
//      what it is before it trusts it.
//
// Note what is *not* here: no file loading, no audio, no window. Those belong
// to the front end. The core does not know they exist -- that rule is what
// lets the same source tree compile to a native binary and to wasm without a
// single #ifdef.
// ---------------------------------------------------------------------------

#include "ffi/emulator_api.h"

extern "C" {

/// Bumped whenever the meaning of anything below changes.
int fc_wasm_abi(void)
{
    return 2;
}

/// The screen size, so the wrapper never hardcodes 256x240 in two places.
int fc_wasm_screen_width(void)
{
    return FC_SCREEN_WIDTH;
}

int fc_wasm_screen_height(void)
{
    return FC_SCREEN_HEIGHT;
}

/// Samples per second the APU produces. The front end needs it to build its
/// audio pipeline, and again it should come from the core, not from a comment.
int fc_wasm_sample_rate(void)
{
    return fc_sample_rate();
}

} // extern "C"
