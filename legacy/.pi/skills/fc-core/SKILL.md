---
name: fc-core
description: 自研 FC/NES 核心包 packages/fc-core（target fc_core 与 fc_ffi）。用于 6502 CPU/指令/周期、总线与内存映射（$2000/$4016/$8000 译码）、iNES 文件头、Mapper（0-4、7、9-11、13、15、18、19、21/22/23/25、32、33、66、68、71、74、78、87、121、162-165、177、178、190、199、226、227、241、242、245、246、249）、PPU 渲染与扫描线时序、APU、手柄、存档 state、C 接口 emulator_api.h 的修改与测试。
---

# fc-core —— 机器本身

上游：无（这一层不依赖任何东西）。
下游：`fc_ffi`、`fc-libretro`、`tools/`、`wasm/`。改这里的接口会波及下方全部。

## 0. 上下文纪律

**白名单**（只读这些，读完就停）：

```
packages/fc-core/src/core/           机器源码（按需挑 1-3 个文件）
packages/fc-core/src/ffi/emulator_api.h   C 接口契约（唯一需要看的 ffi 文件）
packages/fc-core/tests/test_<话题>.cpp   改哪个部件就只读对应的那个测试
packages/fc-core/README.md              ≈60 行，可以整读
```

**禁读**：`build*/`、`packages/fc-core/tests/data/`（ROM，二进制）、
整个 `src/core/nes/mapper*.hpp` 全集。

**按需读**：

- `src/core/nes/mapper.hpp` 是接口（约 60 行），**先读它**再读具体 mapper。
- 改哪个 mapper 就只读 `mapper<NN>.hpp` + `cartridge.cpp` 的 `case NN:` 段
  （用 `rg -n 'case NN' src/core/nes/cartridge.cpp` 定位）。
- `ppu.cpp` / `apu.cpp` / `cpu.cpp` 都偏大：先 `rg -n '函数名'` 定位，再 `read offset/limit`。

## 1. 目录与入口

```
src/core/
  types.hpp  bit.{hpp,cpp}  alu.hpp  bus.hpp  flat_bus.hpp
  cpu/    registers / opcode 表 / disassembler / addressing / cpu（表驱动派发）
  nes/    bus（地址译码）/ ines（文件头）/ cartridge（卡带 + mapper 工厂）
          mapper<NN>.hpp（头文件实现，无 .cpp）
          ppu / apu / controller / machine（CPU-PPU 3:1 同步、NMI、脚本输入）
          ram.hpp / ram_cartridge.hpp / framebuffer.hpp / device.hpp / cheats.hpp
  state.{hpp,cpp} / state_fwd.hpp      序列化（存档）
src/ffi/emulator_api.{h,cpp}           纯 C 接口，前端唯一需要链接的东西
tests/                                 17 个测试文件 + data/（放 ROM，不提交）
```

两个 target：`fc_core`（机器）、`fc_ffi`（→ `fc_core`）。

## 2. 架构铁律

- **禁止依赖 UI**：不能出现 Metal / NSWindow / Canvas / DOM。
- **CPU 不许直接访问 PPU**：一切经过 `Bus`。
- 没有文件 IO、没有线程、没有异常 —— 同一份源码要能原样编成原生库和 wasm，
  不允许出现 `#ifdef`。

## 3. 构建与测试

```bash
cmake -S packages/fc-core -B build-core -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build build-core
ctest --test-dir build-core --output-on-failure

# 只跑相关测试
ctest --test-dir build-core -R Cartridge --output-on-failure
```

真实 ROM 测试找 `tests/data/*.nes`，目录为空会自动 skip；
也可 `FC_TEST_ROM=/path/to/game.nes ctest ...`。

## 4. 任务菜谱

**新增一个 Mapper**（三处，容易漏第三处）：

1. 新建 `src/core/nes/mapper<NN>.hpp`（头文件实现，保持和邻居同样的构造签名）。
2. `src/core/nes/cartridge.cpp`：加 `#include` + `case <NN>:` 分支。
   若 CHR 为 0 页要 `mapper->make_chr_ram();`。
3. `tests/test_cartridge.cpp` 加测试。
   **不需要改 CMakeLists**（mapper 是 header-only）；但**新增测试文件**需要
   把它加进 `tests/CMakeLists.txt` 的 `add_executable` 列表。

**改一条 6502 指令**：`cpu/opcode.cpp`（表）+ `cpu/cpu.cpp`（执行）+
`tests/test_instruction_set.cpp` / `test_cycles.cpp`。周期表改动必须同时更新测试。

**改内存映射**：`nes/bus.cpp` + `tests/test_nes_bus.cpp`。
注意镜像、open bus、`$4014` OAM DMA。

## 5. 检索菜谱

```bash
rg -n 'case 1[0-9]:' packages/fc-core/src/core/nes/cartridge.cpp   # mapper 分派
rg -n 'irq_asserted|on_scanline|on_cpu_cycle|read_expansion' packages/fc-core/src/core/nes/mapper.hpp
rg -n 'FC_TEST_ROM|data/' packages/fc-core/tests/test_real_rom.cpp
rg -n '^\s*TEST\(' packages/fc-core/tests/test_cartridge.cpp      # 看已有测试的命名风格
```

## 6. 已知陷阱（都有回归测试，改动后必须仍然通过）

- **PPU `v` 寄存器是双用**：既是 `$2006/$2007` 的写地址，也是渲染取 tile 的指针。
  强制消隐（forced blanking）期间真机不推进 `v`，所以 `increment_y / copy_x /
  copy_y / 精灵评估` 全部要用 `rendering_enabled()` 包住。否则游戏清 nametable
  时会被拨走地址（表现为 HUD 成片的 `0`、地面空洞）。
- **MMC1 8KB CHR 模式**：`8KB bank = chr0 >> 1`（寄存器低位移出给 PPU A12）。
- **MMC3 扫描线计数靠 PPU A12 上升沿**：每行必须发生一次，即使该行没有精灵
  （`evaluate_sprites()` 末尾要做空取指），否则 IRQ 永不触发（SMB3 状态栏）。
- **APU 单位**：数据手册的表以 CPU 周期为单位，而 APU tick 是半个 CPU 周期，
  换算要 `/2`（否则帧序列器/噪声/DMC/三角波整体慢一个八度）。
