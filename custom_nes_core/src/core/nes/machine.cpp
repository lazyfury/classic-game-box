#include "core/nes/machine.hpp"

#include "core/state.hpp"

namespace fc::nes {

Machine::Machine()
    : cpu_(bus_)
{
    // The bus needs to know where the PPU's registers are, and the PPU is
    // what receives an OAM DMA.
    bus_.set_ppu(&ppu_);
    bus_.set_oam_target(&ppu_);

    // The APU answers $4000-$4017, and its DMC channel reads sample data back
    // out of CPU memory. That read goes through the bus, which is why the APU
    // is handed a pointer to it here rather than owning one.
    bus_.set_apu(&apu_);
    apu_.set_memory_reader(&bus_);
}

bool Machine::load_rom(std::span<const u8> rom, std::string& error)
{
    auto cartridge = Cartridge::from_bytes(rom, error);
    if (!cartridge) {
        return false;
    }

    cartridge_ = std::make_unique<Cartridge>(std::move(*cartridge));

    // The same chip answers on two different buses, so both need a pointer.
    bus_.set_cartridge(cartridge_.get());
    ppu_.set_cartridge(cartridge_.get());

    reset();
    return true;
}

// ---------------------------------------------------------------------------
// Save states
//
// Thin wrappers. The list of what a state contains is in src/core/state.cpp
// and nowhere else; these exist so that a caller does not have to know the
// StateAccess struct exists.
// ---------------------------------------------------------------------------

void Machine::save_state(std::vector<u8>& out) const
{
    StateWriter writer;
    StateAccess::save(*this, writer);
    out = writer.data();
}

std::size_t Machine::state_size() const
{
    std::vector<u8> bytes;
    save_state(bytes);
    return bytes.size();
}

std::size_t Machine::save_state_into(std::span<u8> out) const
{
    StateWriter writer(out);
    StateAccess::save(*this, writer);
    if (writer.overflowed()) {
        return 0;
    }
    return writer.size();
}

bool Machine::load_state(std::span<const u8> data)
{
    StateReader reader(data);
    return StateAccess::load(*this, reader);
}

bool Machine::mapper_saves_state() const noexcept
{
    return cartridge_ != nullptr && cartridge_->mapper().saves_state();
}

void Machine::reset()
{
    ppu_.reset();
    apu_.reset();
    cpu_.reset();
}

int Machine::step_one()
{
    const int cpu_cycles = cpu_.step();

    // The PPU runs at exactly three times the CPU clock. There is no
    // handshake: it simply gets three dots for every CPU cycle that passed,
    // and it does not care what the CPU was doing.
    ppu_.tick(cpu_cycles * Ppu::kPpuCyclesPerCpuCycle);

    // The APU runs at half the CPU clock and divides internally.
    apu_.tick_cpu(cpu_cycles);

    // Hand the CPU the NMI for the vblank we saw on the *previous* step,
    // after this instruction has run.
    //
    // This one-instruction delay is deliberate. The PPU is ticked after the
    // CPU instruction, so a vblank that starts during an instruction's
    // cycles only becomes visible to the *next* instruction. On real
    // hardware the PPU and CPU run side by side, so an instruction can read
    // $2002 after the flag was set but before the NMI is dispatched. Games
    // lean on that: a `LDA $2002 / BPL` vblank wait has to win the race
    // against their own NMI handler, which also reads $2002 and clears the
    // flag. Servicing the NMI at the very next instruction made the handler
    // always win, and games that poll this way - 封神榜 among them - span
    // forever. Letting the next instruction run first reproduces the window
    // the poll needs.
    if (nmi_hold_) {
        cpu_.request_nmi();
        nmi_hold_ = false;
    }
    if (ppu_.consume_nmi()) {
        nmi_hold_ = true;
    }

    // A cartridge can hold the CPU's /IRQ line low (MMC3's scanline counter
    // is the reason). The line is level triggered, so the mapper is asked
    // after every instruction when the line is already low.
    clock_mapper(cpu_cycles);
    cpu_.set_irq_line(cartridge_ != nullptr && cartridge_->mapper().irq_asserted());

    return cpu_cycles;
}

bool Machine::run_instructions(int count)
{
    for (int i = 0; i < count; ++i) {
        if (cpu_.is_halted()) {
            return false;
        }
        step_one();
    }
    return !cpu_.is_halted();
}

bool Machine::run_frame()
{
    // Put the cheats back before the frame the game is about to run. Once a
    // frame and not once an instruction: see cheats.hpp for the trade.
    cheats_.apply(bus_);

    const int target = ppu_.frame_count() + 1;

    // A frame is about 29780 CPU cycles, so a few tens of thousands of
    // instructions. The guard is here because a bug in the PPU timing must
    // not turn into a hang in a test.
    int guard = 0;
    constexpr int kMaxInstructions = 2000000;

    while (ppu_.frame_count() < target) {
        if (cpu_.is_halted()) {
            return false;
        }
        step_one();
        if (++guard > kMaxInstructions) {
            return false;
        }
    }
    return true;
}

void Machine::run_ppu_cycles(int cycles)
{
    ppu_.tick(cycles);
}

void Machine::clock_mapper(int cpu_cycles) noexcept
{
    if (cartridge_ == nullptr) {
        return;
    }
    Mapper& mapper = cartridge_->mapper();
    if (!mapper.clocks_on_cpu_cycles()) {
        return;
    }
    for (int i = 0; i < cpu_cycles; ++i) {
        mapper.on_cpu_cycle();
    }
}

} // namespace fc::nes
