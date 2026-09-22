#include "reporter.h"

#include <algorithm>
#include <cstdio>
#include <set>

namespace fc {

void StdoutLineWriter::writeLine(const std::string& line) {
    std::fwrite(line.data(), 1, line.size(), stdout);
    std::fputc('\n', stdout);
    std::fflush(stdout);
}

Reporter::Reporter(Backend& backend, LineWriter& writer)
    : backend_(backend), writer_(writer) {}

int Reporter::slotFor(const std::string& key) {
    const auto found = slots_.find(key);
    if (found != slots_.end()) {
        return found->second;
    }

    // The lowest number nobody is using. Not "the number of pads so far":
    // that would hand a reconnecting pad a new slot every time and move it in
    // the settings screen while the player is looking at it.
    std::set<int> used;
    for (const auto& entry : slots_) {
        used.insert(entry.second);
    }
    int candidate = 0;
    while (used.count(candidate) != 0) {
        ++candidate;
    }
    slots_[key] = candidate;
    return candidate;
}

void Reporter::poll() {
    const std::vector<PadSample> samples = backend_.poll();

    // Slots are handed out before absent ones are released, so that a device
    // that is still here never has its number taken by a device that just
    // arrived in the same frame.
    std::set<std::string> present;
    std::vector<PadReading> readings;
    readings.reserve(samples.size());
    for (const PadSample& sample : samples) {
        present.insert(sample.key);
        readings.push_back(PadReading{slotFor(sample.key), sample.id, sample.buttons});
    }

    // A pad that has gone releases its slot, so the number can be reused and
    // the list does not grow a hole per unplugging.
    for (auto it = slots_.begin(); it != slots_.end();) {
        if (present.count(it->first) == 0) {
            it = slots_.erase(it);
        } else {
            ++it;
        }
    }

    std::sort(readings.begin(), readings.end(),
              [](const PadReading& a, const PadReading& b) { return a.index < b.index; });

    if (readings == last_) {
        return;
    }
    last_ = readings;
    writer_.writeLine(padsMessage(readings));
}

}  // namespace fc
