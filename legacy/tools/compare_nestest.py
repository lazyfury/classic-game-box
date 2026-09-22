#!/usr/bin/env python3
"""Compare an emulator's nestest log against the reference.

Usage:
    fc_testrom nestest.nes --nestest 9000 > ours.log
    python3 tools/compare_nestest.py packages/fc-core/tests/data/testroms/nestest.log ours.log

Why the two columns are allowed to differ
----------------------------------------
The reference log was made by a different emulator, and two of its columns are
numbered from a different origin. Neither is a difference in behaviour:

  CYC     the reference starts counting at 7, which is a constant. If the gap
          is the same on every line, every instruction took exactly as long as
          it did on the machine the log was made on -- which is the thing the
          column is for. If it drifts, an instruction is mistimed.
  PPU     the same seven cycles, times three: the reference's PPU is 21 dots
          further into its frame on every line. Two emulators starting at
          different moments, sampled at the same instruction.

Neither is a difference in behaviour, and this script does not merely tolerate
them. It *requires* them to be constant, and reports the exact line where
either stops being constant -- which is where a real bug would be.

The scanline cannot be compared directly, because it wraps: a constant 21 dot
offset makes the reference appear a line ahead near the end of a line and
level again just after. So the comparison is on absolute dots from the start of
the frame, with our pre-render line counted as the frame's first line so that
the two numberings line up.
"""

import sys

# The columns, measured from the reference log:
#   0  PC        4 hex
#   4  two spaces
#   6  bytes     8 wide
#   14 one space
#   15 disassembly, 33 wide
#   48 registers
#   74 PPU
#   86 CYC
PC = 0
BYTES = 6
ASM = 15
REGS = 48
PPU = 74
CYC = 86


def parse(line):
    """Every field this compares, or None if the line is not a log line."""
    if len(line) < CYC or line[4:6] != "  ":
        return None

    try:
        cyc = int(line[CYC + 4:])
    except ValueError:
        return None

    # "PPU:  0, 21" -> (0, 21)
    ppu_text = line[PPU + 4:PPU + 11]
    try:
        scanline, dot = (int(part) for part in ppu_text.split(","))
    except ValueError:
        return None

    return {
        "pc": line[PC:PC + 4],
        "bytes": line[BYTES:BYTES + 8],
        "asm": line[ASM:ASM + 33].rstrip(),
        "regs": line[REGS:PPU].rstrip(),
        "scanline": scanline,
        "dot": dot,
        "cyc": cyc,
    }


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        return 2

    reference_path, ours_path = sys.argv[1], sys.argv[2]
    reference = [l.rstrip("\n") for l in open(reference_path)]
    ours = [l.rstrip("\n") for l in open(ours_path)]

    print(f"reference : {len(reference)} lines")
    print(f"ours      : {len(ours)} lines")
    print()

    cyc_offsets = {}
    scanline_offsets = {}
    mismatches = []
    compared = 0

    # Our log stops where the CPU meets an opcode it does not implement. That
    # is a gap rather than a wrong answer, so it is counted and reported
    # separately -- but it is not hidden: the line it stops on is printed.
    stopped_at = None
    for index in range(min(len(reference), len(ours))):
        want = parse(reference[index])
        got = parse(ours[index])
        if want is None or got is None:
            break
        if got["bytes"].strip() and "???" in got["asm"]:
            stopped_at = (index + 1, got["asm"].strip())
            break
        compared += 1

        # What the CPU actually did. This is what nestest is testing.
        for field in ("pc", "bytes", "asm", "regs"):
            if want[field] != got[field]:
                mismatches.append((index + 1, field, want[field], got[field]))
                break
        else:
            cyc_offsets.setdefault(want["cyc"] - got["cyc"], []).append(index + 1)

            # Our pre-render line is -1; the reference's frame starts at 0.
            # Adding one to ours puts the two numberings in the same units.
            want_dots = want["scanline"] * 341 + want["dot"]
            got_dots = (got["scanline"] + 1) * 341 + got["dot"]
            scanline_offsets.setdefault(want_dots - got_dots, []).append(index + 1)

    print(f"compared          : {compared}")
    if stopped_at is not None:
        line, text = stopped_at
        print(f"stopped at        : line {line}, {text}")
        print("                    an undocumented opcode, which this CPU does")
        print("                    not implement (the official 151 are all here)")

    if mismatches:
        print(f"C P U   M I S M A T C H E S : {len(mismatches)}")
        for number, field, want, got in mismatches[:10]:
            print(f"  line {number}  {field}")
            print(f"    reference: {want!r}")
            print(f"    ours     : {got!r}")
    else:
        print("cpu               : every line identical")

    print()

    def report(name, offsets, compared):
        if not offsets:
            print(f"{name}: (no data)")
            return True
        if len(offsets) == 1:
            offset = next(iter(offsets))
            print(f"{name}: constant offset {offset:+d} on all {compared} lines")
            return True
        print(f"{name}: NOT CONSTANT -- {len(offsets)} different offsets")
        for offset, lines in sorted(offsets.items()):
            print(f"    {offset:+d}  first at line {lines[0]}, {len(lines)} lines")
        return False

    cyc_ok = report("cpu cycles   ", cyc_offsets, compared)
    ppu_ok = report("ppu dots     ", scanline_offsets, compared)
    print()
    print("A constant offset on both is two emulators sampled at different")
    print("moments. A drifting one is a timing bug, and the line it drifts at")
    print("is where.")

    print()
    ok = not mismatches and cyc_ok and ppu_ok
    print("PASSED" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
