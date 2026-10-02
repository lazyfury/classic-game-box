/* This target exists only to expose the Rust header to Swift and to satisfy
 * SwiftPM's "a C target needs a source file" rule. The implementation is the
 * Rust library (`libcgb_mac.a`) linked into the executable. */
#include "cgb_host.h"
