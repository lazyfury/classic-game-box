// ---------------------------------------------------------------------------
// The half of the helper that can be tested without a pad.
//
// A fake backend hands the reporter scripted readings; a string writer
// collects the lines it would have written. What is checked is the part with
// decisions in it: slot assignment, slot reuse, and the promise that a
// 60Hz poll of a steady pad produces no output at all.
//
//   cmake -S native/gamepad-cpp -B native/gamepad-cpp/build
//   cmake --build native/gamepad-cpp/build --config Release
//   ctest --test-dir native/gamepad-cpp/build -C Release --output-on-failure
// ---------------------------------------------------------------------------

#include <iostream>
#include <string>
#include <vector>

#include "backend.h"
#include "protocol.h"
#include "reporter.h"

namespace {

int failures = 0;

void check(bool condition, const std::string& what) {
    if (!condition) {
        std::cerr << "FAIL: " << what << "\n";
        ++failures;
    }
}

void checkEqual(const std::string& actual, const std::string& expected, const std::string& what) {
    if (actual != expected) {
        std::cerr << "FAIL: " << what << "\n"
                  << "  expected: " << expected << "\n"
                  << "  actual:   " << actual << "\n";
        ++failures;
    }
}

class FakeBackend final : public fc::Backend {
public:
    std::vector<fc::PadSample> next;
    std::vector<fc::PadSample> poll() override { return next; }
};

class StringWriter final : public fc::LineWriter {
public:
    std::vector<std::string> lines;
    void writeLine(const std::string& line) override { lines.push_back(line); }
};

fc::PadSample pad(const std::string& key, const std::string& id) {
    fc::PadSample sample;
    sample.key = key;
    sample.id = id;
    return sample;
}

}  // namespace

int main() {
    // --- the wire format -------------------------------------------------
    checkEqual(
        fc::helloMessage(42),
        R"({"pid":42,"type":"hello","version":2})",
        "the hello line");

    checkEqual(
        fc::jsonString("plain"),
        R"("plain")",
        "a plain string");
    checkEqual(
        fc::jsonString("a\"b\\c"),
        R"("a\"b\\c")",
        "quotes and backslashes are escaped");
    checkEqual(
        fc::jsonString("line\nbreak"),
        R"("line\nbreak")",
        "a newline never splits a line");

    // A whole message, exactly, so that the order of the keys and the eight
    // names are pinned down. The reader does not care about key order; a
    // person reading two helpers' output side by side does.
    {
        std::vector<fc::PadReading> pads;
        fc::PadReading reading;
        reading.index = 0;
        reading.id = "Xbox Controller";
        reading.buttons.A = true;
        pads.push_back(reading);
        checkEqual(
            fc::padsMessage(pads),
            "{\"type\":\"pads\",\"pads\":[{\"index\":0,\"id\":\"Xbox Controller\","
            "\"buttons\":{\"A\":true,\"B\":false,\"SELECT\":false,\"START\":false,"
            "\"UP\":false,\"DOWN\":false,\"LEFT\":false,\"RIGHT\":false,"
            "\"L1\":false,\"R1\":false,\"L2\":false,\"R2\":false,"
            "\"L3\":false,\"R3\":false,\"FACE_X\":false,\"FACE_Y\":false,"
            "\"GUIDE\":false}}]}",
            "a whole pads message");
    }

    // --- the reporter ----------------------------------------------------
    FakeBackend backend;
    StringWriter writer;
    fc::Reporter reporter(backend, writer);

    // Nothing connected: nothing worth saying.
    backend.next = {};
    reporter.poll();
    check(writer.lines.empty(), "no pads means no line");

    // The first pad takes slot 0.
    backend.next = {pad("xinput:0", "Xbox Controller")};
    reporter.poll();
    check(writer.lines.size() == 1, "connecting a pad writes one line");
    check(writer.lines[0].find("\"index\":0") != std::string::npos, "the first pad is slot 0");

    // The same pad, twice more: silence.
    reporter.poll();
    reporter.poll();
    check(writer.lines.size() == 1, "a steady pad says nothing");

    // A button: one line, and only the button.
    backend.next[0].buttons.A = true;
    reporter.poll();
    check(writer.lines.size() == 2, "a press writes one line");
    check(writer.lines[1].find("\"A\":true") != std::string::npos, "the press is reported");

    // A second pad takes the next slot.
    backend.next.push_back(pad("xinput:1", "Xbox Controller"));
    reporter.poll();
    check(writer.lines.size() == 3, "a second pad writes one line");
    check(writer.lines[2].find("\"index\":0") != std::string::npos, "pad 0 is still first");
    check(writer.lines[2].find("\"index\":1") != std::string::npos, "pad 1 is listed too");

    // The first pad goes. Its slot is released; the second keeps its number,
    // because moving it would reassign it in the settings screen.
    backend.next = {pad("xinput:1", "Xbox Controller")};
    reporter.poll();
    check(writer.lines.size() == 4, "a disconnect writes one line");
    check(writer.lines[3].find("\"index\":0") == std::string::npos, "the gone pad is not listed");
    check(writer.lines[3].find("\"index\":1") != std::string::npos, "the remaining pad keeps slot 1");

    // A new device reuses the free slot 0 rather than growing the list.
    backend.next.push_back(pad("xinput:2", "Xbox Controller"));
    reporter.poll();
    check(writer.lines.size() == 5, "a new pad writes one line");
    check(writer.lines[4].find("\"index\":0") != std::string::npos, "the free slot is reused");

    // Same key, same slot: reconnect is not a new device.
    backend.next = {pad("xinput:1", "Xbox Controller")};
    reporter.poll();
    check(writer.lines.size() == 6, "a disconnect writes one line");
    check(writer.lines[5].find("\"index\":1") != std::string::npos, "slot 1 survived");

    if (failures != 0) {
        std::cerr << failures << " check(s) failed\n";
        return 1;
    }
    std::cout << "fc-gamepad: all checks passed\n";
    return 0;
}
