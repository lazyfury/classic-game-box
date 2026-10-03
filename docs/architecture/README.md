# 系统架构 architecture/

> 状态：**Phase 2 和 Phase 7 已完成**（总线架构 + Electron 前端）
>
> 已定型决定的「为什么」记在 [`decisions/`](decisions/README.md)（架构决策记录）；
> 当前模块边界的可判定规则在 [`../../CONVENTIONS.md`](../../CONVENTIONS.md)。

## 目标结构

```
+-------------------------------------------+
|                  NES Core                 |
|                                           |
|   CPU ---- Bus ---- PPU ---- Framebuffer  |
|    |        |        |                    |
|   APU    Cartridge  VRAM (2KB)            |
|    |        |                             |
|  Controller Mapper                        |
+-------------------------------------------+
                     |
              Emulator API
                     |
            Electron / Canvas
                     |
                  Screen
```

## 已完成

| 文档 | 内容 |
|------|------|
| [bus.md](bus.md) | 为什么需要 Bus、Device 接口、依赖方向、测试替身 |
| [frontend.md](frontend.md) | C 接口、Canvas 渲染、音频环形缓冲、主循环 |
| [libretro-migration.md](libretro-migration.md) | 以 libretro 为准的迁移、多核心（mGBA / Mesen）接入 |
| [mame-integration.md](mame-integration.md) | 调研：MAME 能不能接进来、走哪条路、代价是什么（未实施） |

### 当前实际结构

```
                 +-----------+
                 |   Bus     |   抽象 (custom_nes_core/src/core/bus.hpp)
                 +-----+-----+
                       ^
                       | CPU 只知道这个
                 +-----+-----+
                 |   Cpu     |   custom_nes_core/src/core/cpu/
                 +-----------+

                 +-----------+
                 |  NesBus   |   实现 (custom_nes_core/src/core/nes/bus.hpp)
                 +-----+-----+
                       |
       +---------------+---------------+
       |               |               |
     Ram          Device*          Device*
  (2KB, 回绕)   (PPU 槽)        (卡带槽)
```

**依赖方向已用 grep 验证：**

```bash
$ grep -rn '#include "core/nes/' custom_nes_core/src/core/cpu/
  （无）
$ grep -rn 'nes::' custom_nes_core/src/core/cpu/
  （无）
```

CPU 层完全不知道 NES 的存在。这条 grep 保持为空，架构就是对的。

## 硬性规则（来自 AGENTS.md）

### 规则 1：CPU 不得直接访问 PPU

```
错误                         正确
CPU                           CPU
 |                             |
memory[]                       Bus
 |                             |
PPU                           PPU
```

CPU 只知道"我要往地址 `$2006` 写一个字节"。
是 Bus 决定了这个写操作其实是发给 PPU 的。

**收益：** CPU 可以完全独立测试；换一台机器（Game Boy）只需换 Bus 和 PPU。

### 规则 2：Core 不得依赖 UI

`custom_nes_core/src/core/` 里的代码不能 `#include <Metal/Metal.h>`，不能出现 `NSWindow`、
`document`。Core 只产出一个 `256×240` 的 RGB framebuffer，谁来显示它由
`electron/`（或 `tools/fc_headless`）决定。

**收益：** Core 可以在命令行、测试、无头环境下运行。

### 规则 3：一切通信经过 Bus

```
CPU 读 $8000  ->  Bus 判断：$4020-$FFFF?  -> 卡带
CPU 读 $2002  ->  Bus 判断：$2000-$3FFF?  -> PPU
CPU 读 $0000  ->  Bus 判断：<$2000?       -> RAM
```

## 完整的依赖方向

```
   electron/ (TypeScript)     UI 层
        |
   custom_nes_core/src/ffi/emulator_api.h     C 接口 —— 唯一的边界
        |
   custom_nes_core/src/core/nes/              NES 硬件
        |
   custom_nes_core/src/core/cpu/              6502
        |
   custom_nes_core/src/core/bit.hpp          位与字节
```

**箭头只能向下。** Core 不知道窗口存在，CPU 不知道 NES 存在。

```bash
$ grep -rn '#include "core/nes/' custom_nes_core/src/core/cpu/
  （无）
$ grep -rln 'Metal\|NSWindow' custom_nes_core/src/core/
  （无）
```

## 待写文档

- `overview.md` — 完整模块职责与依赖图
