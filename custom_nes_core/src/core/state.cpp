// ---------------------------------------------------------------------------
// Save states: the whole of it.
//
// Every field of every class that the machine's behaviour depends on, written
// in a fixed order. Read it top to bottom and you have read the complete list
// of what a running NES is made of, which is the reason it lives in one file
// rather than in six.
//
// The three rules this file follows:
//
//   1. Write only what cannot be recomputed. The framebuffer, the APU's sample
//      queue, the disassembler's output -- all derived, all omitted. Every
//      byte left out is a byte that cannot disagree with the machine.
//
//   2. Write the fields in the order they are declared, and read them back in
//      the same order. Nothing here is clever; the two halves sit next to each
//      other so that a field added to one and forgotten in the other is
//      visible rather than subtle.
//
//   3. Nothing is written by copying a struct. Padding bytes are not part of
//      the state, and copying them would make the file depend on the layout
//      the compiler happened to choose -- which is different on arm64 and on
//      wasm32, and the two are compared with cmp.
//
// On the counter-intuitive part: the mapper's bank registers look like
// configuration rather than state, and they are the thing most often left out
// of a save. They are not memory, they are the wiring of the cartridge, and a
// state that loses them comes back showing the wrong part of the ROM a few
// instructions later. See Mapper::serialize.
// ---------------------------------------------------------------------------

#include "core/state.hpp"

#include "core/cpu/cpu.hpp"
#include "core/nes/apu.hpp"
#include "core/nes/bus.hpp"
#include "core/nes/cartridge.hpp"
#include "core/nes/controller.hpp"
#include "core/nes/machine.hpp"
#include "core/nes/ppu.hpp"

namespace fc {

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

void StateAccess::write_cpu(const Cpu& cpu, StateWriter& out)
{
    out.put_u8(cpu.reg_.a);
    out.put_u8(cpu.reg_.x);
    out.put_u8(cpu.reg_.y);
    out.put_u8(cpu.reg_.sp);
    out.put_u8(cpu.reg_.p);
    out.put_u16(cpu.reg_.pc);

    out.put_u64(cpu.cycles_);
    out.put_u16(cpu.instruction_pc_);
    out.put_u8(cpu.last_opcode_);
    out.put_s32(cpu.branch_extra_cycles_);

    out.put_flag(cpu.nmi_pending_);
    out.put_flag(cpu.irq_line_);
    out.put_flag(cpu.halted_);
    out.put_u8(cpu.unimplemented_opcode_);
}

bool StateAccess::read_cpu(Cpu& cpu, StateReader& in)
{
    in.get_u8(cpu.reg_.a);
    in.get_u8(cpu.reg_.x);
    in.get_u8(cpu.reg_.y);
    in.get_u8(cpu.reg_.sp);
    in.get_u8(cpu.reg_.p);
    in.get_u16(cpu.reg_.pc);

    in.get_u64(cpu.cycles_);
    in.get_u16(cpu.instruction_pc_);
    in.get_u8(cpu.last_opcode_);
    in.get_s32(cpu.branch_extra_cycles_);

    in.get_flag(cpu.nmi_pending_);
    in.get_flag(cpu.irq_line_);
    in.get_flag(cpu.halted_);
    in.get_u8(cpu.unimplemented_opcode_);

    return in.ok();
}

// ---------------------------------------------------------------------------
// The bus: the console's RAM and the two controllers.
//
// The 2KB of RAM goes through Ram's own read/write rather than reaching for
// its array, which keeps the mirroring mask in one place and costs 2048 calls
// once per save.
// ---------------------------------------------------------------------------

void StateAccess::write_bus(const nes::NesBus& bus, StateWriter& out)
{
    for (u16 address = 0; address < nes::Ram::kSize; ++address) {
        out.put_u8(bus.ram_.read(address));
    }

    for (const auto& controller : bus.controllers_) {
        out.put_u8(controller.buttons_);
        out.put_u8(controller.shift_);
        out.put_flag(controller.strobing_);
        out.put_u64(controller.reads_);
    }

    out.put_u8(bus.open_bus_);
    out.put_s32(bus.pending_stalls_);
    out.put_s32(bus.oam_dma_count_);
}

bool StateAccess::read_bus(nes::NesBus& bus, StateReader& in)
{
    for (u16 address = 0; address < nes::Ram::kSize; ++address) {
        u8 value = 0;
        in.get_u8(value);
        bus.ram_.write(address, value);
    }

    for (auto& controller : bus.controllers_) {
        in.get_u8(controller.buttons_);
        in.get_u8(controller.shift_);
        in.get_flag(controller.strobing_);
        in.get_u64(controller.reads_);
    }

    in.get_u8(bus.open_bus_);
    in.get_s32(bus.pending_stalls_);
    in.get_s32(bus.oam_dma_count_);

    return in.ok();
}

// ---------------------------------------------------------------------------
// The PPU
//
// The framebuffer is not here. It is the PPU's output, not its state, and the
// next visible scanline overwrites it. Loading a state mid frame therefore
// shows one frame that is part old and part new, which is a great deal better
// than carrying 240KB in every save file.
//
// The shift registers and the eight selected sprites do have to be here: they
// are the PPU's pipeline, and losing them means the scanline being drawn when
// the save was taken comes back as garbage.
// ---------------------------------------------------------------------------

void StateAccess::write_ppu(const nes::Ppu& ppu, StateWriter& out)
{
    out.put_u8(ppu.ctrl_);
    out.put_u8(ppu.mask_);
    out.put_u8(ppu.status_);
    out.put_u8(ppu.oam_addr_);
    out.put_u8(ppu.read_buffer_);
    out.put_u8(ppu.io_latch_);

    out.put_u16(ppu.v_);
    out.put_u16(ppu.t_);
    out.put_u8(ppu.fine_x_);
    out.put_flag(ppu.write_toggle_);

    out.put_s32(ppu.scanline_);
    out.put_s32(ppu.dot_);
    out.put_s32(ppu.frame_count_);
    out.put_flag(ppu.nmi_occurred_);

    out.blob(ppu.nametables_);
    out.blob(ppu.palette_ram_);
    out.blob(ppu.oam_);

    out.put_u8(ppu.next_tile_id_);
    out.put_u8(ppu.next_tile_attr_);
    out.put_u8(ppu.next_tile_lsb_);
    out.put_u8(ppu.next_tile_msb_);
    out.put_u16(ppu.shifter_lo_);
    out.put_u16(ppu.shifter_hi_);
    out.put_u16(ppu.attr_lo_);
    out.put_u16(ppu.attr_hi_);

    for (const auto& sprite : ppu.sprite_line_) {
        out.put_u8(sprite.x);
        out.put_u8(sprite.attr);
        out.put_u8(sprite.pattern_lo);
        out.put_u8(sprite.pattern_hi);
        out.put_flag(sprite.is_sprite_zero);
    }

    out.put_s32(ppu.sprite_count_);
    out.put_flag(ppu.sprite_zero_in_range_);
    out.put_flag(ppu.sprite_zero_hit_);
    out.put_flag(ppu.sprite_overflow_);

    out.put_u64(ppu.vram_writes_visible_);
    out.put_u64(ppu.vram_writes_prerender_);
    out.put_u64(ppu.vram_reads_visible_);
    out.put_u64(ppu.vram_writes_blanking_);
}

bool StateAccess::read_ppu(nes::Ppu& ppu, StateReader& in)
{
    in.get_u8(ppu.ctrl_);
    in.get_u8(ppu.mask_);
    in.get_u8(ppu.status_);
    in.get_u8(ppu.oam_addr_);
    in.get_u8(ppu.read_buffer_);
    in.get_u8(ppu.io_latch_);

    in.get_u16(ppu.v_);
    in.get_u16(ppu.t_);
    in.get_u8(ppu.fine_x_);
    in.get_flag(ppu.write_toggle_);

    in.get_s32(ppu.scanline_);
    in.get_s32(ppu.dot_);
    in.get_s32(ppu.frame_count_);
    in.get_flag(ppu.nmi_occurred_);

    in.blob(ppu.nametables_);
    in.blob(ppu.palette_ram_);
    in.blob(ppu.oam_);

    in.get_u8(ppu.next_tile_id_);
    in.get_u8(ppu.next_tile_attr_);
    in.get_u8(ppu.next_tile_lsb_);
    in.get_u8(ppu.next_tile_msb_);
    in.get_u16(ppu.shifter_lo_);
    in.get_u16(ppu.shifter_hi_);
    in.get_u16(ppu.attr_lo_);
    in.get_u16(ppu.attr_hi_);

    for (auto& sprite : ppu.sprite_line_) {
        in.get_u8(sprite.x);
        in.get_u8(sprite.attr);
        in.get_u8(sprite.pattern_lo);
        in.get_u8(sprite.pattern_hi);
        in.get_flag(sprite.is_sprite_zero);
    }

    in.get_s32(ppu.sprite_count_);
    in.get_flag(ppu.sprite_zero_in_range_);
    in.get_flag(ppu.sprite_zero_hit_);
    in.get_flag(ppu.sprite_overflow_);

    in.get_u64(ppu.vram_writes_visible_);
    in.get_u64(ppu.vram_writes_prerender_);
    in.get_u64(ppu.vram_reads_visible_);
    in.get_u64(ppu.vram_writes_blanking_);

    return in.ok();
}

// ---------------------------------------------------------------------------
// The APU
//
// Every channel, in full. Unlike the PPU there is nothing derivable here:
// an envelope's decay level, a sweep's counter and the noise channel's shift
// register are all mid flight at any given moment, and dropping any of them
// changes the next sample.
//
// The two mix accumulators are here for the same reason. They hold the partial
// averages between output samples, which is to say the anti aliasing filter's
// memory; without them the first sample after a load is wrong, which is
// audible as a click.
//
// The sample queue is not here. It is output, on its way to a speaker that
// does not care about save states.
// ---------------------------------------------------------------------------

void StateAccess::write_apu(const nes::Apu& apu, StateWriter& out)
{
    const auto pulse = [&out](const nes::PulseChannel& channel) {
        out.put_flag(channel.is_pulse1_);
        out.put_u8(channel.duty_);
        out.put_flag(channel.length_halt_);
        out.put_flag(channel.constant_volume_);
        out.put_u8(channel.volume_);
        out.put_u8(channel.envelope_divider_);
        out.put_u8(channel.envelope_decay_);
        out.put_flag(channel.envelope_start_);
        out.put_flag(channel.sweep_enable_);
        out.put_u8(channel.sweep_period_);
        out.put_flag(channel.sweep_negate_);
        out.put_u8(channel.sweep_shift_);
        out.put_flag(channel.sweep_reload_);
        out.put_u8(channel.sweep_counter_);
        out.put_u16(channel.timer_);
        out.put_u16(channel.timer_counter_);
        out.put_u8(channel.duty_pos_);
        out.put_u8(channel.length_);
        out.put_flag(channel.enabled_);
    };

    pulse(apu.pulse1_);
    pulse(apu.pulse2_);

    {
        const auto& channel = apu.triangle_;
        out.put_flag(channel.control_);
        out.put_u8(channel.linear_reload_);
        out.put_flag(channel.linear_reload_flag_);
        out.put_u8(channel.linear_counter_);
        out.put_u16(channel.timer_);
        out.put_u16(channel.timer_counter_);
        out.put_u8(channel.sequence_pos_);
        out.put_u8(channel.length_);
        out.put_flag(channel.enabled_);
    }

    {
        const auto& channel = apu.noise_;
        out.put_flag(channel.length_halt_);
        out.put_flag(channel.constant_volume_);
        out.put_u8(channel.volume_);
        out.put_u8(channel.envelope_divider_);
        out.put_u8(channel.envelope_decay_);
        out.put_flag(channel.envelope_start_);
        out.put_flag(channel.mode_);
        out.put_u8(channel.period_index_);
        out.put_u16(channel.timer_counter_);
        out.put_u16(channel.lfsr_);
        out.put_u8(channel.length_);
        out.put_flag(channel.enabled_);
    }

    {
        const auto& channel = apu.dmc_;
        out.put_flag(channel.irq_enable_);
        out.put_flag(channel.loop_);
        out.put_u8(channel.rate_index_);
        out.put_u8(channel.level_);
        out.put_u16(channel.address_);
        out.put_u16(channel.length_);
        out.put_u16(channel.current_address_);
        out.put_u16(channel.bytes_remaining_);
        out.put_u16(channel.timer_counter_);
        out.put_u8(channel.sample_buffer_);
        out.put_flag(channel.sample_buffer_empty_);
        out.put_u8(channel.bits_remaining_);
        out.put_flag(channel.enabled_);
        out.put_flag(channel.irq_pending_);
    }

    out.put_u8(apu.enabled_);

    out.put_s32(apu.frame_step_);
    out.put_s32(apu.frame_counter_);
    out.put_flag(apu.five_step_);
    out.put_flag(apu.irq_inhibit_);
    out.put_flag(apu.frame_irq_);

    out.put_s32(apu.cpu_remainder_);
    out.put_f64(apu.sample_accumulator_);
    out.put_f64(apu.mix_accumulator_);
    out.put_s32(apu.mix_counter_);
    out.put_f32(apu.last_output_);
    out.put_u64(apu.cycles_);
}

bool StateAccess::read_apu(nes::Apu& apu, StateReader& in)
{
    const auto pulse = [&in](nes::PulseChannel& channel) {
        in.get_flag(channel.is_pulse1_);
        in.get_u8(channel.duty_);
        in.get_flag(channel.length_halt_);
        in.get_flag(channel.constant_volume_);
        in.get_u8(channel.volume_);
        in.get_u8(channel.envelope_divider_);
        in.get_u8(channel.envelope_decay_);
        in.get_flag(channel.envelope_start_);
        in.get_flag(channel.sweep_enable_);
        in.get_u8(channel.sweep_period_);
        in.get_flag(channel.sweep_negate_);
        in.get_u8(channel.sweep_shift_);
        in.get_flag(channel.sweep_reload_);
        in.get_u8(channel.sweep_counter_);
        in.get_u16(channel.timer_);
        in.get_u16(channel.timer_counter_);
        in.get_u8(channel.duty_pos_);
        in.get_u8(channel.length_);
        in.get_flag(channel.enabled_);
    };

    pulse(apu.pulse1_);
    pulse(apu.pulse2_);

    {
        auto& channel = apu.triangle_;
        in.get_flag(channel.control_);
        in.get_u8(channel.linear_reload_);
        in.get_flag(channel.linear_reload_flag_);
        in.get_u8(channel.linear_counter_);
        in.get_u16(channel.timer_);
        in.get_u16(channel.timer_counter_);
        in.get_u8(channel.sequence_pos_);
        in.get_u8(channel.length_);
        in.get_flag(channel.enabled_);
    }

    {
        auto& channel = apu.noise_;
        in.get_flag(channel.length_halt_);
        in.get_flag(channel.constant_volume_);
        in.get_u8(channel.volume_);
        in.get_u8(channel.envelope_divider_);
        in.get_u8(channel.envelope_decay_);
        in.get_flag(channel.envelope_start_);
        in.get_flag(channel.mode_);
        in.get_u8(channel.period_index_);
        in.get_u16(channel.timer_counter_);
        in.get_u16(channel.lfsr_);
        in.get_u8(channel.length_);
        in.get_flag(channel.enabled_);
    }

    {
        auto& channel = apu.dmc_;
        in.get_flag(channel.irq_enable_);
        in.get_flag(channel.loop_);
        in.get_u8(channel.rate_index_);
        in.get_u8(channel.level_);
        in.get_u16(channel.address_);
        in.get_u16(channel.length_);
        in.get_u16(channel.current_address_);
        in.get_u16(channel.bytes_remaining_);
        in.get_u16(channel.timer_counter_);
        in.get_u8(channel.sample_buffer_);
        in.get_flag(channel.sample_buffer_empty_);
        in.get_u8(channel.bits_remaining_);
        in.get_flag(channel.enabled_);
        in.get_flag(channel.irq_pending_);
    }

    in.get_u8(apu.enabled_);

    in.get_s32(apu.frame_step_);
    in.get_s32(apu.frame_counter_);
    in.get_flag(apu.five_step_);
    in.get_flag(apu.irq_inhibit_);
    in.get_flag(apu.frame_irq_);

    in.get_s32(apu.cpu_remainder_);
    in.get_f64(apu.sample_accumulator_);
    in.get_f64(apu.mix_accumulator_);
    in.get_s32(apu.mix_counter_);
    in.get_f32(apu.last_output_);
    in.get_u64(apu.cycles_);

    return in.ok();
}

// ---------------------------------------------------------------------------
// The cartridge
//
// Work RAM and the battery backed save RAM are the same 8KB on most boards,
// and they are the only part of the cartridge the game can write to. The ROM
// is not state: it came from the file and cannot change.
//
// The mapper's own registers come next, through Mapper::serialize. A mapper
// that does not implement it writes nothing, and the state is then incomplete
// rather than wrong -- see the note on the interface.
//
// The identifying fields (mapper number, ROM sizes) are written at the *top*
// of the file by save(), not here, so that load() can reject a state for a
// different game before it has overwritten a single byte of a running machine.
// ---------------------------------------------------------------------------

void StateAccess::write_cartridge(const nes::Cartridge& cartridge, StateWriter& out)
{
    out.put_flag(cartridge.prg_ram_enabled_);
    out.sized_bytes(cartridge.prg_ram_);

    // Whether the mapper below actually wrote anything, so a reader can tell
    // a mapper with no registers from one this build has not implemented.
    out.put_flag(cartridge.mapper_ != nullptr && cartridge.mapper_->saves_state());

    if (cartridge.mapper_ != nullptr) {
        cartridge.mapper_->serialize(out);
    }
}

bool StateAccess::read_cartridge(nes::Cartridge& cartridge, StateReader& in)
{
    in.get_flag(cartridge.prg_ram_enabled_);
    in.sized_bytes(cartridge.prg_ram_);

    bool mapper_saved = false;
    in.get_flag(mapper_saved);

    if (!in.ok()) {
        return false;
    }

    if (!mapper_saved) {
        // Written by a mapper that has no bank registers to save, or by one
        // this build has not implemented. Either way the cartridge is left
        // wired the way it was at power on, which is exactly where it would be
        // if the game had just started.
        return true;
    }

    if (cartridge.mapper_ != nullptr) {
        cartridge.mapper_->deserialize(in);
    }
    return in.ok();
}

// ---------------------------------------------------------------------------
// The machine
// ---------------------------------------------------------------------------

void StateAccess::save(const nes::Machine& machine, StateWriter& out)
{
    // No cartridge, no state. Writing a header that can never be loaded would
    // only give a front end a file to offer the player that would then refuse
    // to open.
    if (machine.cartridge_ == nullptr) {
        return;
    }

    out.raw(kStateMagic.data(), kStateMagic.size());
    out.put_u32(kStateVersion);

    // Which game this is, first, so load() can refuse a mismatched state while
    // the machine is still intact. A save from another cartridge cannot be
    // applied at all -- the ROM behind every bank number is different -- and
    // refusing early is the difference between an error and a corrupt machine.
    out.put_u32(machine.cartridge_->header_.mapper);
    out.put_u32(static_cast<u32>(machine.cartridge_->prg_rom_.size()));
    out.put_u32(static_cast<u32>(machine.cartridge_->chr_rom_.size()));

    write_cpu(machine.cpu_, out);
    write_bus(machine.bus_, out);
    write_ppu(machine.ppu_, out);
    write_apu(machine.apu_, out);

    out.put_flag(machine.nmi_hold_);

    write_cartridge(*machine.cartridge_, out);
}

bool StateAccess::load(nes::Machine& machine, StateReader& in)
{
    std::array<u8, 4> magic{};
    if (!in.raw(magic.data(), magic.size()) || magic != kStateMagic) {
        return false;
    }

    u32 version = 0;
    if (!in.get_u32(version) || version != kStateVersion) {
        return false;
    }

    if (machine.cartridge_ == nullptr) {
        return false;
    }

    u32 mapper = 0;
    u32 prg_size = 0;
    u32 chr_size = 0;
    if (!in.get_u32(mapper) || !in.get_u32(prg_size) || !in.get_u32(chr_size)) {
        return false;
    }

    const auto& header = machine.cartridge_->header_;
    if (mapper != header.mapper
        || prg_size != machine.cartridge_->prg_rom_.size()
        || chr_size != machine.cartridge_->chr_rom_.size()) {
        return false;
    }

    // Everything above this line has been checked. Past it the machine is
    // being overwritten, so a failure from here on leaves it in a state that
    // is not worth running -- the caller has to reload the ROM.
    if (!read_cpu(machine.cpu_, in)) {
        return false;
    }
    if (!read_bus(machine.bus_, in)) {
        return false;
    }
    if (!read_ppu(machine.ppu_, in)) {
        return false;
    }
    if (!read_apu(machine.apu_, in)) {
        return false;
    }

    in.get_flag(machine.nmi_hold_);

    if (!read_cartridge(*machine.cartridge_, in)) {
        return false;
    }

    // Anything left over means the file was written by a build that knows more
    // than this one does.
    return in.finished();
}

} // namespace fc
