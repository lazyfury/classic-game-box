// ---------------------------------------------------------------------------
// Slots, and the decision to speak.
//
// The backend reports pads; this turns them into the messages the renderer
// understands. It is deliberately the same state machine as the Swift
// helper's PadReporter, because the main process and the renderer should not
// be able to tell which one is on the other end of the pipe.
//
// Two jobs:
//
//   1. Give each device a slot and keep it there. `index` is what the
//      settings screen calls "player 1"; two identical pads report the same
//      name, so the slot is the only name a pad has. A pad keeps its slot
//      while it stays connected and the lowest free one is reused after it
//      goes, so the list does not grow a permanent hole per unplugging.
//
//   2. Say nothing when nothing changed. Polling is 60Hz; the pipe is only
//      worth writing to when the answer is different. The main process
//      filters once more against the last reading, so the renderer hears each
//      change exactly once.
// ---------------------------------------------------------------------------

#pragma once

#include <map>
#include <string>
#include <vector>

#include "backend.h"
#include "protocol.h"

namespace fc {

/// Somewhere a line can go. An interface, not std::cout, so the reporter can
/// be tested without a pipe -- which is the only way to test it on a machine
/// with no gamepad attached.
class LineWriter {
public:
    virtual ~LineWriter() = default;
    virtual void writeLine(const std::string& line) = 0;
};

/// Writes to stdout and flushes every line.
///
/// The flush is the point. The pipe the parent reads is not a terminal, so
/// the C runtime buffers it; a line that sits in a 4KB buffer until the
/// buffer fills is a button press that arrives seconds late, or never if the
/// player is standing still.
class StdoutLineWriter : public LineWriter {
public:
    void writeLine(const std::string& line) override;
};

/// Turns polls into `pads` messages.
class Reporter {
public:
    Reporter(Backend& backend, LineWriter& writer);

    /// Poll the backend once and write a line if, and only if, something
    /// changed.
    void poll();

    /// The last thing written, for tests and for a caller that wants to know.
    const std::vector<PadReading>& last() const { return last_; }

private:
    /// The slot for a device, assigning and remembering one if this is the
    /// first time it has been seen.
    int slotFor(const std::string& key);

    Backend& backend_;
    LineWriter& writer_;
    std::map<std::string, int> slots_;
    std::vector<PadReading> last_;
};

}  // namespace fc
