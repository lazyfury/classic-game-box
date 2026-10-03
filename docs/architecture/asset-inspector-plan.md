# 资源检视器（Asset Inspector）计划

## 目标

围绕最初的目标 —— **FC / GBA 资源分析，学习游戏的做法与美术风格** —— 加一个检视器
子系统。它不是调试器：不反汇编、不单步、不打断游戏，只做「看」——看图案表、调色板、
背景图、精灵、内存。

**范围**：只做 2D tile 机种（NES / GB / GBA，后续可扩 SNES / Genesis）。
N64 / PSP / PS1 / PS2 等硬件渲染机种没有可读的 2D VRAM，入口直接不出现。

## 机制：libretro `SET_MEMORY_MAPS`（按 mGBA 的标准）

核心通过 `RETRO_ENVIRONMENT_SET_MEMORY_MAPS` 发布「模拟地址空间 → 宿主缓冲」的映射。
这正是 RetroArch 给金手指 / RetroAchievements 用的标准机制，不是私有的。前端只读这套
map，因此 **同一份代码对所有支持该命令的核心生效**。

已核实的核心支持：

| 核心 | 发布内容 |
|---|---|
| mGBA | IWRAM / EWRAM / SRAM / ROM / BIOS / VRAM / PAL / OAM / I/O，带 `start` + `select` |
| Mesen | CPU 总线 $0000-$FFFF 的 256 字节块 |
| nestopia | CPU 总线 + save RAM |
| custom_nes_core | SYSTEM_RAM / SAVE_RAM，以及新增的 PPU 区域 |
| snes9x / genesis_plus_gx | 支持该命令（后续可接） |

**重要**：描述符数组常常是核心的**栈上临时数组**（mGBA 的 `_setupMaps` 就是），
`env` 调用返回后即失效。前端必须在回调里**拷贝描述符值**（指针本身指向的字节由核心保证
整个会话有效，可以留着）。当前实现正是如此。

## 状态

- **P0 通用层**：完成。前端采集 `SET_MEMORY_MAPS` 并暴露 `MemoryRegion` / `read_memory`，
  mGBA、Mesen、custom_nes_core 端到端验证。
- **NES 解码器 + 检视器 UI**：完成。`src/inspect/nes.rs`（图案表 / 调色板 / 背景 / 精灵）+ 新
  `Section::Inspector`（左栏「资源」）；自研核心已发布 `NT/PAL/OAM/CHR`。
- **GBA 解码器**：完成。`src/inspect/gba.rs` 按 mGBA 的固定地址读 VRAM/调色板/OAM/IO，
  解 4bpp/8bpp tile、RGB555 调色板、文本 BG 图与 OAM 精灵；应用按 `SystemId` 分派。
- **hex / 内存视图**：完成。第 5 个视图「内存」：中栏点选任一内存区域，右栏显示该区域的
  分页 hex dump（`src/inspect/hex.rs`，自带 5×7 点阵字体烘成图片，因为 UI 没有等宽字体；
  每页 512 字节，← / → 翻页）。它 **对任何发布内存映射的核心都可用**，包括只发布 CPU RAM、
  没有 2D 图形内存的 Mesen。

## 已完成

### 前端 API（`crates/cgb-libretro`）

```rust
pub struct MemoryRegion {
    pub flags: u64,          // RETRO_MEMDESC_*
    pub start: usize,        // 模拟地址
    pub len: usize,
    pub select: usize,       // 地址位匹配掩码
    pub disconnect: usize,   // 地址位忽略掩码
    pub addrspace: String,   // 核心给的短名（可空）
    ptr: usize,              // 宿主指针，存成整数以保持 Send/Sync
}
impl MemoryRegion {
    pub fn is_video_ram/is_system_ram/is_save_ram/is_read_only(&self) -> bool;
    pub fn label(&self) -> String;          // addrspace 或按 flags 猜
    pub fn address_range(&self) -> (usize, usize);
    pub fn copy_into(&self, offset, out) -> usize;   // 缓冲视图，边界检查
    pub fn read_at(&self, address, out) -> usize;    // 模拟地址视图
}
impl CoreHost {
    pub fn memory_regions(&self) -> Vec<MemoryRegion>;
    pub fn read_memory(&self, address, out) -> usize; // 按 map 翻译
}
```

`src/session.rs` 透传 `memory_regions()` / `read_memory()`。地址翻译遵循 libretro 的
`ptr + (addr & ~disconnect) - start`，并先用 `select` 过滤；读取全程边界检查。

### 机种约定（解码器据此识别区域）

- **mGBA**：VRAM/PAL/OAM 未设 `RETRO_MEMDESC_VIDEO_RAM` 标志，按固定地址识别：
  `0x06000000` VRAM（96KB）、`0x05000000` 调色板（1KB）、`0x07000000` OAM（1KB）、
  `0x04000000` I/O（含 `DISPCNT` / `BGxCNT`）、`0x08000000` ROM（CONST）。
- **custom_nes_core**：按 `addrspace` 识别：`NT`（4KB nametable VRAM，PPU $2000）、
  `PAL`（32 字节，PPU $3F00）、`OAM`（256 字节，合成地址 $3F20）、
  `CHR`（CHR ROM，只读，合成地址 $4000）；另有 `SYSTEM_RAM` / `SAVE_RAM`。

### 解码层（`src/inspect/`）

- `InspectImage`：RGBA8 缓冲 + 边界检查的 `set` / `fill_rect`。
- `nes::view(index, chr, palette, nametables, oam)`：一次解出所选视图（图案表 / 调色板 /
  背景 / 精灵）+ 标题；`nes::color` 用与自研核心一致的 2C02 调色板表。纯函数，可单测。

### 应用与 UI

- `src/app/inspect.rs`：读区域 → `nes::view` → 上传成 `INSPECTOR_TEXTURE_BASE` 纹理。进入页面 /
  切视图时全量刷新并重建，游戏运行时每 8 帧流式更新（OAM / nametable 保持实时）。
- `Section::Inspector`（`src/ui/view/inspector.rs`）：中栏视图按钮 + 刷新 + 内存映射列表，
  右栏解码图。`inspector_ready` 仅在核心发布了 NES 图形缓冲（`CHR/PAL/NT/OAM`）时为真。

## 剩余工作（可选扩展）

- 其它 2D 机种（GB / SNES / Genesis）解码器。
- 精灵视图的仿射（旋转/缩放）对象目前按未变换处理。
- hex 视图暂无 ASCII 列（等宽字体问题；需扩展点阵字体）。

## 应用与 UI 分派

`src/app/inspect.rs` 按 `SystemId` 选解码器：`Gba` → `gba::view`（按地址取 VRAM
`0x06000000` / 调色板 `0x05000000` / OAM `0x07000000` / IO `0x04000000`）；其余（NES）
→ `nes::view`（按 `addrspace` 取 `CHR/PAL/NT/OAM`）。`inspector_ready` 与之一致：
GBA 看是否发布了 VRAM/调色板，NES 看是否发布了那四个图形区域。

## 已知限制

- **CHR 是原始 ROM，不是当前映射的 8KB 窗口**：带 CHR banking 的 mapper（MMC3 等）
  会显示整颗芯片的所有 bank（对「看全部素材」其实更好），但不反映运行时 bank 状态。
- **CHR-RAM 游戏（`chr_rom_pages == 0`）目前没有 CHR 区域**：CHR RAM 由 mapper 持有，
  没有统一的缓冲访问器。后续可给 `Mapper` 加一个默认返回空的 `chr_data()`。
- **可解码的核心**：自研 FC 核心（PPU 区域）与 mGBA（GBA，按固定地址）。Mesen 只发布
  CPU RAM，没有 PPU 图形；GB / SNES / Genesis 的解码器尚未写。
- **GBA 精灵的仿射对象**：`attr0` 的变换位为真时，按未变换处理（忽略旋转/缩放矩阵）。
- **重硬件机种不做**（无 2D VRAM 可读）。
