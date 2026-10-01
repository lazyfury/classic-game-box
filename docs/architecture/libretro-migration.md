# 迁移到 libretro ABI —— 调研与计划 v2

> 状态：**L1~L6 完成**。FC 走 libretro 化并端到端验收；mGBA 编译 + ABI 对齐 + 前端集成完成，
> `.gba/.gb/.gbc` 可在 App 里运行。唯一未做：卡带电池存档（`.srm`）落盘。
> v2 变更：确立 **libretro 为准**；列出**暂时隐藏**的功能；新增 **custom ABI 扩展**设计；
> 用实验**确认了 wasm 动态加载外部核心的可行性**（结论：原生 core 不行，专用
> wasm side module 可以，已验证）。
> 目标：Core 以 libretro ABI 为唯一对外契约；前端以 libretro frontend 为骨架；
> 自定义能力走一层可选的 custom 扩展；为日后接入 mGBA 铺路。

---

## 0. 结论（TL;DR）

1. **libretro 是权威**。所有"能不能做"以 `libretro.h` 为准；现有 `fc_*` 接口
   降级为**内部/扩展**，不再是前端与 Core 之间的主契约。
2. **Core 不重写**。仍是 `fc_core`，但对外只暴露 libretro。新增一个薄适配层
   `custom_nes_core/src/libretro/fc_libretro.cpp`，把 `retro_*` 翻译成 `Machine` 方法。
3. **不重要的功能先隐藏**（§4.2）：金手指面板、扫描线滤镜、封面/截图、
   原生手柄助手、PC/cycles 诊断读数。它们要么与 libretro 字符串模型不兼容，
   要么是 host 的职责，等 libretro 骨架稳定后再逐个接回。
4. **custom 扩展**（§6）：用**额外导出的符号 + 版本化函数指针表**
   `fc_libretro_get_ext()` 暴露非标准能力；标准前端（RetroArch）忽略它，
   自家前端 `dlsym` 到就用。
5. **wasm 加载外部核心：已确认**（§7，含可复现实验）。
   - ❌ 原生 `.dylib` / `.so`：wasm 沙箱**不能**加载。
   - ⚠️ 任意第三方预编译 wasm core：**不能**即插即用（toolchain/ABI 强耦合）。
   - ✅ **专门编译的 wasm side module**：**能**。已在 `emsdk 6.0.9` + Node 24 上
     验证 `dlopen` + `dlsym` + 反向回调（`retro_set_video_refresh` 模式）+
     跨模块共享 framebuffer 指针，全部工作。
   - 代价：必须开 `MAIN_MODULE=2` / `ALLOW_MEMORY_GROWTH=1`，与当前
     `ALLOW_MEMORY_GROWTH=0`（为防 typed array 视图失效而设）冲突，需改造。
6. **推荐路线**：A（native core）→ custom ext → wasm core（已验证形态）→
   Electron 切 libretro host → mGBA 作为第二个 wasm side module。

---

## 1. libretro 为准意味着什么

| | 旧（v1 计划） | 新（v2，libretro 为准） |
|---|---|---|
| Core 对外契约 | `fc_ffi`（`fc_*`） | **libretro `retro_*`** |
| 前端与 Core 之间 | `useEmulator.ts` 直调 `fc_*` | **libretro `CoreHost`**（标准语义） |
| 非标准能力 | 散在 `fc_*` | **custom 扩展** `fc_libretro_get_ext()` |
| `fc_*` 的地位 | 主接口 | 适配层内部实现 + 测试/工具 |
| 视频格式 | `0x00RRGGBB` 约定 | `RETRO_PIXEL_FORMAT_XRGB8888`（等价） |
| 音频 | mono float + 私有队列 | `audio_sample_batch` **int16 stereo** |
| 输入 | `fc_set_button` | `input_poll` / `input_state` + JOYPAD id |
| 存档 | `fc_save_state` | `retro_serialize` / `unserialize` |
| 金手指 | `{addr,value,freeze}` | `retro_cheat_set` **字符串** |
| 组合多个模拟器 | 无 | 天生支持第二个 libretro core |

保留 `fc_*` 的理由：它是适配层的直接实现、`wasm/headless.mjs` 的测试入口、
`tools/fc_headless` 的依赖，删掉没有收益。**只是不再作为对外的首要 ABI。**

---

## 2. 现状盘点（哪些已就绪）

Core 已经满足 libretro 的三条硬性前提：无 UI 依赖、无文件 IO、无异常，且
Native / wasm 双编译。`custom_nes_core/src/core` 分层：

```
custom_nes_core/src/core/nes/  custom_nes_core/src/core/cpu/     机器本身（CPU/Bus/PPU/APU/Mapper/State）
custom_nes_core/src/ffi/emulator_api.{h,cpp}     现有 C 接口（22 个测试）
wasm/glue.cpp                    Emscripten 薄壳（只做 ABI 版本自检）
electron/                        Electron 前端（库/输入/音频/UI）
```

现状能力清单与 libretro 的逐项对照见 §3。

---

## 3. 功能对照表

图例：✅ 直接支持　🟡 需适配　❌ 不支持（隐藏或走 custom）　⏸ 暂时隐藏

| 功能 | 现状 | libretro 对应 | 结论 |
|---|---|---|---|
| 生命周期 | `fc_create/destroy` | `retro_init/deinit`（单例） | 🟡 静态 `fc_machine*` |
| 加载 ROM | `fc_load_rom(内存)` | `retro_load_game` | ✅ |
| 复位 | `fc_reset` | `retro_reset` | ✅ |
| 逐帧 | `fc_run_frame` | `retro_run`（一帧 + 一次 video/audio） | ✅ |
| Halt | `fc_is_halted` | 无 | 🟡 log + `GET_CAN_DUPE` |
| 帧计数 | `fc_frame_count` | 前端自计 | ✅ |
| 视频 | `0x00RRGGBB` 256×240 | `SET_PIXEL_FORMAT` + `video_refresh` | ✅ pitch=1024 |
| 几何/区域/帧率 | 固定 | `av_info.geometry` / `get_region` / `fps` | ✅ NTSC 60.0988 |
| 音频 | mono float 44100 + 内部队列 | `audio_sample_batch` int16 stereo | 🟡 **主工作量** |
| 音频设备/缓冲 | `fc_audio_queue_*` | 前端自带 | ✅ 仅本 App 用 |
| 输入 | `fc_set_button(port)` | `input_poll/state` | 🟡 id 映射 |
| 两端口 | 2×Controller | port 0/1 + `SET_CONTROLLER_INFO` | ✅ |
| 存档状态 | `fc_save_state*` | `retro_serialize*` | ✅ 倒带因此自动可用 |
| 电池存档 | PRG RAM 存在但未暴露 | `RETRO_MEMORY_SAVE_RAM` | 🟡 加 `Cartridge::prg_ram()` |
| 内存/调试 | `fc_peek/poke` | `RETRO_MEMORY_SYSTEM_RAM` + memory maps | 🟡 加 `NesBus::ram_data()` |
| 金手指 | `{addr,value,freeze}` | `retro_cheat_set(字符串)` | ❌ 走 custom / 先隐藏 |
| 诊断 PC/cycles | `fc_total_cycles/pc` | 无标准 | ⏸ 走 custom / 隐藏 |
| ROM 摘要 | `fc_rom_summary` | `SET_MESSAGE` / log | 🟡 |
| 扫描线滤镜 | 前端绘制 | host/shaders 职责 | ⏸ 隐藏 |
| 封面/截图 | SQLite + PNG | host 职责 | ⏸ 隐藏 |
| 原生手柄助手 | Swift/C++ helper | host 输入职责 | ⏸ 隐藏（先用标准输入） |
| 游戏库 | SQLite | host playlist 职责 | 🟡 先简化（列表+加载） |
| 存档槽 / 倒带 | 前端 | host 职责（基于 serialize） | ✅ 保留 |
| 全屏 / 整数缩放 | 前端 | host 职责 | ✅ 保留 |

---

## 4. 决策

### 4.1 以 libretro 为准

- 对外契约 = `libretro.h`；新功能先问"libretro 有没有对应"。
- `fc_*` 只服务于适配层内部和现有测试，不再新增公开用法。
- 前端后续统一走 `CoreHost`（§5.2），其方法名/语义对齐 libretro。

### 4.2 暂时隐藏清单（第一批）

原则：**与 libretro 模型冲突、或属于 host 职责、或非核心体验**的功能先关掉，
减少迁移面；UI 位置保留占位，后续按需接回。

| 隐藏项 | 原因 | 替代/恢复路径 |
|---|---|---|
| 金手指面板（原始 addr/value） | libretro 只认 Game Genie/PAR 字符串 | custom 扩展接回；或 host 字符串通道 |
| 扫描线滤镜 | 属 host 渲染 | 等 libretro host 的 shader/滤镜框架 |
| 封面与截图面板 | 属 host 资源管理 | 保留数据库表，UI 暂不显示 |
| 原生手柄助手（Swift/C++） | 属 host 输入 | 先用标准 JOYPAD 输入；助手作为可选加速 |
| PC / cycles 诊断读数 | 非 libretro 标准 | custom 扩展 `cpu_pc()/total_cycles()` |
| 游戏库高级项（置顶/排序/搜索） | 非核心 | 先"列表 + 加载"，稳定后恢复 |

**保留**：加载/运行/暂停/复位、画面、声音、键盘手柄、存档槽、快速存读、
倒带、全屏、整数缩放。这些要么是 libretro 标准，要么是 host 的通用能力。

### 4.3 custom ABI 扩展

见 §6。

### 4.4 wasm 加载外部核心

见 §7（已实验确认）。

---

## 5. 迁移方案

### 5.1 三个产物

```
fc_libretro.dylib        native libretro core      -> RetroArch / Lakka
fc_libretro.wasm         wasm  libretro core（side module）-> 自家 Electron 前端
fc_core.mjs              （可选保留）旧的单体 wasm，过渡期兜底
```

三者同源：`custom_nes_core/src/core` 不变，差异只在适配层与编译方式。

### 5.2 前端抽象 `CoreHost`（libretro 语义）

不按 `fc_*` 命名，而按 libretro 命名，让"标准 core"和"未来 mGBA"都能实现：

```ts
interface CoreHost {
  systemInfo(): { name; version; extensions: string[] };
  avInfo(): { width; height; fps; sampleRate };
  loadGame(bytes: Uint8Array): boolean;
  unload(): void;
  reset(): void;
  run(): void;                              // 一帧
  // 回调由 host 在构造 core 时注册，这里只取结果：
  takeVideo(): { ptr; width; height; pitch; format: 'XRGB8888' };
  takeAudio(): Int16Array;                  // stereo
  setInput(port: number, id: number, down: boolean): void;
  serialize(): Uint8Array;
  unserialize(b: Uint8Array): boolean;
  memory(id: 'SAVE_RAM' | 'SYSTEM_RAM'): Uint8Array | null;
  setCheat(index: number, enabled: boolean, code: string): void;
  resetCheats(): void;
  // custom 扩展（可选）
  ext?: CustomExt;
}
```

`useEmulator.ts` 直调 `fc_*` 的地方集中替换到这一层；UI/库/存档槽基本不动。
**这是 mGBA 可插拔的关键。**

### 5.3 分阶段计划

| 阶段 | 内容 | 产出 | 估时 |
|---|---|---|---|
| **L0** | 调研（本文） | 文档 + wasm 实验 | ✅ |
| **L1** | native 适配层 `fc_libretro.cpp` + `custom_nes_core/third_party/libretro/libretro.h` + CMake MODULE target；音频/视频/输入/存档转换 | RetroArch 能加载运行 | ✅ 已完成 |
| **L2** | custom 扩展符号 `fc_libretro_get_ext()`；`Cartridge::prg_ram()`、`NesBus::ram_data()`、电池标志；RAM 型金手指 | 电池存档、内存视图、custom 通道 | ✅ 已完成 |
| **L3** | Game Genie/PAR 解码 + ROM 补丁钩子 + `SET_MEMORY_MAPS` | 金手指完整、搜索可用 | ✅ 已完成 |
| **L4** | wasm 加载本 core：采用 **L4a —— 独立 wasm 模块 + JS libretro frontend**（`wasm/libretro.mjs`）。side module 机制对 C core 已验证可行；C++ 运行时对齐问题绕开 | 浏览器/Node 可加载本 core | ✅ 已完成 |
| **L5** | Electron `CoreHost` 切到 libretro wasm 宿主（`wasm/libretro.mjs`）；canvas 绘制、音频、金手指、诊断全部走 CoreHost | 前端 libretro 化、零回归 | ✅ 已完成 |
| **L6** | 接入 mGBA：自己编 wasm core、ABI 对齐、系统注册表、按扩展名路由与重建机器、库收 `.gba/.gb/.gbc`、分辨率/帧率/采样率按 core | `.gba/.gb/.gbc` 可玩 | ✅ 已完成 |
| 备选 | native core host（B1）`native/core-host` + IPC | 可加载任意现成 `.dylib` | 1~2 周 |

L1~L3 只增不改；L4 起才动 wasm/前端。

---

## 6. custom ABI 扩展（新增，按 libretro 惯例）

### 6.1 为什么需要

libretro 的 `environment` 是 **core → frontend** 方向，frontend 无法用它调用
core 的额外函数；`retro_cheat_set` 只收字符串；`fc_peek/poke`、原始金手指、
mapper 完整性、诊断读数都没有标准落点。所以需要一个**可选的、非侵入的**
core 出口。

### 6.2 设计：额外导出符号 + 版本化函数指针表

```c
/* custom_nes_core/src/libretro/fc_libretro_ext.h —— 只有自家 frontend 会读 */
#define FC_LIBRETRO_EXT_VERSION 1u

typedef struct fc_libretro_ext_v1 {
    uint32_t abi_version;   /* == FC_LIBRETRO_EXT_VERSION */
    uint32_t struct_size;   /* 构建时的 sizeof，便于跨版本判断 */

    int         (*peek)(uint16_t address);
    void        (*poke)(uint16_t address, uint8_t value);
    int         (*set_raw_cheats)(const uint8_t* data, int count); /* 现有 4 字节格式 */
    int         (*raw_cheat_count)(void);
    bool        (*mapper_saves_state)(void);
    const char* (*rom_summary)(void);
    uint64_t    (*total_cycles)(void);
    uint16_t    (*cpu_pc)(void);
} fc_libretro_ext_v1;

/* core 导出；frontend 用 dlsym 探测，找不到就只用标准 ABI */
const fc_libretro_ext_v1* fc_libretro_get_ext(void);
```

规则：

1. **只增不改**：新版加字段 + 提升 `abi_version`/`struct_size`，旧 frontend
   按 `struct_size` 截断读取。
2. **前缀 `fc_`**，避免与未来 libretro 官方符号冲突。
3. **标准前端忽略**：RetroArch 不 `dlsym` 这个符号，零影响。
4. **frontend 优雅降级**：`ext` 不存在 → 隐藏依赖它的 UI（金手指面板、
   诊断读数），标准功能不受影响。
5. **wasm 同样适用**：side module 用 `export_name`/`EMSCRIPTEN_KEEPALIVE`
   导出该符号，`dlsym` 行为与 native 一致（已在 §7 验证 dlsym）。
6. 若某个能力后来被 libretro 标准化，迁移到标准通道，custom 里保留但标注
   deprecated。

### 6.3 custom vs 标准 的边界

| 走标准 libretro | 走 custom 扩展 |
|---|---|
| 视频/音频/输入/存档/复位/加载 | `peek` / `poke` |
| `retro_cheat_set`（字符串） | 原始 `{addr,value,freeze}` 金手指 |
| `RETRO_MEMORY_*`（SaveRAM/SystemRAM） | mapper 完整性、ROM 摘要、PC/cycles |
| core options（`SET_VARIABLES`） | 未来 core 私有的调试/测试钩子 |

---

## 7. wasm 方式加载外部核心 —— 确认结果

### 7.1 结论

| 输入 | 能否在 wasm 中加载 | 说明 |
|---|---|---|
| 原生 `.dylib` / `.so` | ❌ | wasm 沙箱不允许 |
| 第三方预编译 libretro wasm core | ⚠️ 基本不能 | ABI/toolchain/命名空间强耦合，不能即插即用 |
| **专门用同版本 emsdk 编的 wasm side module** | ✅ **能** | 已实测 |

### 7.2 实测（可复现）

环境：仓库内 `third_party/emsdk`（Emscripten **6.0.9**）+ Node 24。

```bash
source third_party/emsdk/emsdk_env.sh

# 1) 模拟 libretro core，编成 side module
emcc core.c  -O2 -sSIDE_MODULE=1 -o core.wasm
# 2) 模拟 libretro frontend，编成 main module
emcc main.c  -O2 -sMAIN_MODULE=1 -sALLOW_MEMORY_GROWTH=1 -o frontend.mjs
# 3) 运行
node runner.mjs   # runner.mjs: import factory from './frontend.mjs'; await factory();
```

`core.c` 导出 `retro_api_version` / `retro_run` / `retro_set_video_refresh`
（用 `__attribute__((export_name(...)))`），frontend `dlopen("core.wasm")` +
`dlsym`，把**自己的函数指针**交给 core，core 每帧反向调用它并传 framebuffer。

实测输出：

```
START
api_version=1
counter=5
  [frontend] video_refresh #1: 256x240 first=0x11223344
  [frontend] video_refresh #2: 256x240 first=0x11223344
  [frontend] video_refresh #3: 256x240 first=0x11223344
callback count=3
```

**证明**：`dlopen` / `dlsym` / 反向函数指针回调 / 跨模块共享内存指针
（framebuffer）四件事在 wasm 动态链接下全部成立，足以承载 libretro ABI。

### 7.3 必须付的代价（写进 L4）

1. **构建模式改变**：需要 `-sMAIN_MODULE=2`（宿主）+ `-sSIDE_MODULE=1`（core），
   `MAIN_MODULE` 会把输出变成 `export default factory`（模块化工厂），
   与当前 `wasm/CMakeLists.txt` 的 `--no-entry` + `-sMODULARIZE=1` 组合需要重排。
2. **必须开 `ALLOW_MEMORY_GROWTH=1`**：动态链接会在加载/运行时增长内存。
   当前刻意设 `=0` 是为了避免"内存增长后缓存的 typed array 视图失效"
   这一类经典漏洞。改开后，**所有 HEAPU8/HEAPF32 视图必须在可能增长后重取**，
   framebuffer 指针每次 `takeVideo()` 重新解析。
3. **core 必须与宿主同版本 emsdk 编译**：不能直接吃 RetroArch 的预编译 core；
   mGBA 需要我们自己用 `-sSIDE_MODULE=1` 编。
4. **`FILESYSTEM=0` 下 `dlopen` 取不到文件**：需要在宿主里加 fetch/预加载逻辑，
   或把 core wasm 作为数据传入。
5. **单线程**：C++ 异常/RTTI/线程受限；mGBA 单线程可用，含音频线程的 core 需裁剪。
6. **`RTLD_NOW`**：未解析符号会在 `dlopen` 时直接失败，core 的依赖要齐。
7. **调试更难**：跨模块栈/符号，DevTools 支持有限。

### 7.4 对 mGBA 的直接影响

- 想在 Electron（wasm 路径）跑 mGBA → **把 mGBA 编成 libretro side module**（L6）。
  可行但需要移植 mGBA 的 build 到 emsdk 6.0.9 + `-sSIDE_MODULE=1`。
- 想直接跑官方 `mgba_libretro.dylib` → 走**备选 B1 native host**，代价是 IPC。
- 两条路都成立，**不冲突**：native 产物给 RetroArch/桌面 host；wasm 产物给
  渲染进程内 host。

### 7.5 L4 实测：真实 C++ core 的运行时障碍（**需决策**）

`L1~L3` 的适配层已能作为 side module 编出（单条 `emcc` 调用含全部 `fc_core`
源文件，4 秒，172KB），且 **C 语言 core 的 `dlopen`/`dlsym`/回调/共享内存
已在 §7.2 验证**。但把**本项目这个 C++ core** 装进 side module 时，
`dlopen` 失败，原因不在本项目，而在 Emscripten 的 C++ 运行时模型：

1. side module 会 **import** 一批 libc/libc++ 符号，而不是自带：
   `operator new/delete`、`__cxa_throw`、`std::logic_error`、
   `std::string::__grow_by_and_replace`、`std::to_string`、`lroundf`、
   `vsnprintf` 等。
2. 这些必须由 **main module 导出**。而 main 只链接自己用到的 libc++ 子集，
   一个普通 `host.cpp` 用不到 `std::to_string` / 异常，于是导出缺失。
3. 实测：给 core 加 `-fvisibility=hidden` 后，项目自身符号降为本地，
   side module 的 import 从 33 个降到 **17 个纯运行时符号**；但只要 main
   不导出它们，`dlopen` 就报 `could not load dynamic lib`（**不告诉你是哪个
   符号**）。在 main 里手动引用这些 libc++ 特性来“拉齐”也能走，但新增一个
   core 用到的运行时函数就会再次静默破坏加载，属于脆弱方案。

**结论：Emscripten side module 对 C core（mGBA）成立；对 C++ core
需要 main/side 的 libc++ 对齐，不宜作为本项目自身 core 的主路径。**

因此 L4 有三条路线，需选一条（推荐 L4a）：

| 路线 | 做法 | 适用 | 代价 |
|---|---|---|---|
| **L4a（推荐）** | core 编成**独立 wasm 模块**（各自一块线性内存），JS 侧实现 libretro frontend 回调（`addFunction` 传函数指针）；每个 core 一个模块 | 本项目 C++ core、mGBA（mGBA 官方也有 wasm 构建） | 每 core 各自的内存，JS 桥接；不是“dlopen 同一地址空间” |
| **L4b** | side module + `MAIN_MODULE` 宿主，共享地址空间 `dlopen` | **C core（mGBA）**；已验证机制 | C++ 运行时对齐问题（见上） |
| **L4c** | native host（`native/core-host`，仿 gamepad helper）加载 `.dylib` | 现成第三方 core、无需重编 | 帧/音频过 IPC；非 wasm |

**建议**：L4a 落地本项目自身的 libretro wasm 产物与 JS `CoreHost`；
L6 接 mGBA 时优先 L4a（编 mGBA wasm 模块），若坚持用官方预编译 core
则走 L4c。L5 的 `CoreHost` 接口对 a/b/c 三者都兼容。

**已决策并实现**：L4 采用 L4a。`wasm/CMakeLists.txt` 新增 `fc_libretro_wasm`
目标，产出 `wasm/dist/fc_libretro.mjs` + `fc_libretro.wasm`；回调由
`wasm/libretro.mjs` 用 `addFunction` 注册（environment / video / audio batch /
input poll+state），并对外暴露与计划一致的 `CoreHost` 形状（loadGame / run /
framebuffer / audio / memory / serialize / cheat / extension）。因为是独立模块，
`ALLOW_MEMORY_GROWTH=0` 得以保留，typed array 视图不会失效。
验收：`node wasm/libretro_test.mjs`（合成电池 NROM，无需真实 ROM）——
ABI、XRGB8888、256×240、~734 stereo/帧、两端口轮询、2KB/8KB 内存视图、
存档往返像素一致、custom 扩展版本，全部通过。

**L5 已落地**：CoreHost 的默认后端是**渲染进程内的 wasm libretro 前端**
（不是子进程）。`wasm/libretro.mjs` 同时实现 `useEmulator.ts` 一直在用的
`Emulator` 接口，所以切换是“换模块 + 换构造器”的一行改动，游戏循环、输入、
金手指、倒带均未变。canvas 绘制改成每帧重取 framebuffer 视图（`subarray`，O(1)）
——libretro core 的 framebuffer 指针只有跑过一帧才有效，缓存一次会永远画空帧。

音频是唯一“libretro 表达不了”的东西：ABI 只带 int16 stereo，而本项目的
`electron/verify.sh` 逐字节比对 APU 的 float32。因此扩展新增
`take_samples(float*, size_t)`（版本升到 **2**），把 APU 原始 float 样本交给
自家前端；标准前端仍走 int16 回调。已验证：SMB 60 帧下，libretro 路径与原生
`fc_headless` 的**像素哈希与音频字节完全一致**。

---

## 8. mGBA 接入（本次不执行，仅预留）

### 8.1 需要现在预留的接口（L5 的 `CoreHost` / 系统注册表）

1. **系统注册表**：`{ systemId, extensions, controllerInfo, saveRAMKind, fps }`，
   按扩展名分发（`.nes`→fc，`.gba`→mgba）。
2. **存档分级**：路径含 system + rom；`serialize` 字节 opaque，前端不得假设格式。
3. **输入描述数据化**：由 `SET_INPUT_DESCRIPTORS` / `SET_CONTROLLER_INFO`
   驱动 UI（GBA 是十字键 + A/B + L/R，无 2P），不要硬编码 NES 8 键。
4. **动态分辨率/帧率**：来自 `av_info`（GBA 240×160，59.7275 fps），
   前端时钟与缩放不得写死。
5. **音频统一为 int16 stereo**：与 FC 适配层同一形态，避免分叉。
6. **custom 扩展可选存在**：mGBA 没有 `fc_*`，`CoreHost.ext` 为 undefined，
   相关 UI 自动隐藏。

### 8.2 mGBA 特有事项

- BIOS：可选（内置 HLE），`GET_SYSTEM_DIRECTORY` 支持外部 `gba_bios.bin`。
- 存档类型：SRAM/Flash/EEPROM 大小不一，由 `RETRO_MEMORY_SAVE_RAM` size 决定。
- RTC：`RETRO_MEMORY_RTC`（宝可梦）。
- 金手指：CodeBreaker/GameShark 字符串，core 自解析，前端只透传。
- e-Reader / 多卡：`load_game_special`，可先不支持。

### 8.3 wasm 路线已验证（L6 里程碑）

mGBA **没有**上游 wasm 构建，EmulatorJS 的预编译产物又是它自己的
`EJS_Runtime` 胶水（没有 `_retro_*`/`addFunction`），不能直接用。所以自己编：

- `wasm/mgba/build.sh`：拉 `EmulatorJS/mgba` fork（含 libretro Makefile 的
  `platform=emscripten` 目标）→ 改两个宏 → `emmake make` 出目标文件 → 用
  `emcc` 自己链成导出 `retro_*` 的独立模块（与 `fc_libretro.wasm` 同形）。
- 两处关键修改：去掉 `-DCOLOR_16_BIT`（否则 mGBA 用 RGB565，与前端和 parity
  测试的 XRGB8888 不一致）；去掉 `-DHAVE_CRC32`（否则缺 zlib 的 `crc32`）。
- 验证：`node wasm/mgba_test.mjs <rom>` —— 2MB GBC ROM 能加载、跑 60 帧、
  画 160×144、出声、存/读 202KB 状态，全部通过。
- 顺带验证了“**换 core 不改前端**”：`wasm/libretro.mjs` 不用知道它在驱动 mGBA。
  为此加了两处容错：mGBA 的 `retro_get_system_av_info` 要 load 之后才安全
  （FC 任何时候都行），扩展函数（peek/poke/诊断）在其他 core 上不存在要降级。

**剩余（集成，非编译）**：系统注册表 + 按扩展名路由（`.nes`→fc，`.gba/.gb/.gbc`
→mgba）、库收非 `.nes`、按 core 重建机器（现在模块在启动时加载一次）、
分辨率/帧率/输入描述数据化、BIOS 与存档目录。估 **2~3 人天**。

### 8.4 集成已完成

- `electron/src/renderer/systems.ts`：扩展名 → core（`fc_libretro` / `mgba_libretro`）、
  以及每个机种的采样率与帧长。采样率必须随 core 走：NES 44100、GBA 65536、
  GB/GBC 131072，音频上下文按它建。
- `useEmulator`：建机器时根据扩展名（或命令行 ROM）选 core；已装卡时若换机种，
  先 `stopMachine()` 拆掉（帧循环、音频、倒带环）再重建。
- 库与打开面板收 `.nes/.gba/.gb/.gbc`；帧循环用 `core.frameSeconds`（GBA 59.7275）。
- 修了两个 mGBA 才暴露的问题：`retro_cheat_reset` 在 load 前解引用 `core`（
  装卡前不调 core 的 cheat），以及 canvas 尺寸要在 load 后（mGBA 才能报几何）重设。
- 实测（Pokémon Sapphire 256Mb，`--rom` 启动）：240×160、400 帧、音频 peak 0.208、
  画面在动、倒带落回同一帧。FC 路径 parity 仍逐像素逐采样一致。

**未做**：卡带电池存档（`.srm`）落盘。存档槽/倒带用的是 save state，已在；
但 Pokémon 自己的存档需要把 `RETRO_MEMORY_SAVE_RAM` 写到磁盘并在加载时读回。

### 8.5 第二个 NES 核心：Mesen，与「同机种多核心」

这是第一次**同一个机种有两个核心**，也是把 §8.1 的「系统注册表」真正用起来的一次。

- `wasm/mesen/build.sh`：拉 `libretro/Mesen`（旧 Mesen 1.x，C++11，自带 `platform=emscripten`
  目标），编译成 `mesen_libretro.wasm`。与 mGBA 用同一个 `wasm/libretro.mjs` 驱动，
  前端不知道自己在跑哪个核心。三处需要说明的改动：
  - **内存里的卡带**：Mesen 只从 `GET_GAME_INFO_EXT` 拿内存数据，否则按路径
    打开文件；wasm 没有文件系统，所以在 `libretro.cpp` 的回退分支里直接用
    `retro_game_info->data/size`。
  - **C++ 异常**：Mesen 的 loader 用 `throw`/`catch` 表达「这张卡带读不了」；
    Emscripten 默认关异常，会把 `throw` 变成整个模块 abort。给它加
    `-fexceptions`，链接加 `-sDISABLE_EXCEPTION_CATCHING=0`。
  - **文件系统**：与 fc/mGBA 不同，**不能**用 `-sFILESYSTEM=0`。Mesen 用 `ifstream`
    探 `disksys.rom` / `MesenDB.txt` / HdPacks；没有文件系统时这些探测把空流报成
    good、`tellg()` 回 -1，Mesen 于是 `resize(0xFFFFFFFF)` 抛 `std::length_error`。
    保留默认的 MEMFS，探测就以干净的失败告终。
- `electron/src/shared/api.ts` 新增 `SystemId` / `CoreId` / `CoreSelection`，以及
  两边共用的 `CORES_BY_SYSTEM`（哪个机种有哪些核心、默认是谁）。main 进程用它
  校验写盘的选择；renderer 的 `systems.ts` 用它给出模块名与采样率。
  **顺序即默认**：`nes: ['mesen', 'fc']`，所以 NES 默认跑 Mesen，内置 FC 核心
  是一次点击之外的第二个选项（它带 `fc_libretro_get_ext` 自定义扩展 —— 内存
  peek/poke、PC/周期读数，Mesen 没有这些，相关 UI 自动降级）。
- 设置界面「模拟器核心」按机种列出核心；切换核心时 `useEmulator.reload()` 拆掉
  当前机器、用新核心把同一张卡带重新装进去（不计游玩次数 —— 换的是硬件，不是
  又玩了一次）。
- `electron/verify.sh` 的比对基准是原生 `build/fc_headless`（即内置 FC 核心），
  而 NES 默认已是 Mesen；脚本现在用一个临时的 `--user-data-dir` 把核心钉成
  `fc`，既保证拿同一台机器比，也让开发者自己的 config.json 不影响这个测试。
- 命令行 `--rom` 是 eager 建机器，早于异步 preferences。主进程在建窗口前已经
  同步读过 config.json，所以把选择用 `--fc-cores=`（与 `--fc-eager` 同一条
  `additionalArguments` 链）递给渲染进程，首帧就是对的；异步 preferences 只管
  运行中的修改。
- 界面上做得出来的验证：`--user-data-dir` 指向一份写着 `{"cores":{"nes":"mesen"}}`
  的 config.json，`--selftest 120` 的画面哈希与音频样本数都与默认 fc 不同
  （48000 Hz 对 44100 Hz），且 `error: none`；`node wasm/mesen_test.mjs <rom>`
  独立验证 ABI/画面/声音/存档往返。FC 的 electron parity 仍逐像素逐采样一致。

**未做**：Mesen 的 `RETRO_MEMORY_SAVE_RAM`（电池存档）落盘，与 mGBA 同。

---

## 9. 风险与决策点

| # | 决策 | 结论/建议 |
|---|---|---|
| D1 | 谁为准 | **libretro.h**；`fc_*` 降级为内部实现 |
| D2 | `fc_*` 是否删除 | 保留（测试/工具/适配层实现），不再新增公开用法 |
| D3 | 不重要功能 | 按 §4.2 第一批隐藏，UI 留占位 |
| D4 | custom 扩展形态 | 额外导出符号 `fc_libretro_get_ext()`，版本化指针表 |
| D5 | wasm 外部核心 | 可以，但必须自编 side module；原生 dylib 不行（§7） |
| D6 | 内存增长 | L4 必须处理 typed array 视图失效，是最大技术债 |
| D7 | 金手指 ROM 补丁 | 需要碰 mapper 读路径，排 L3 |
| D8 | 线程安全 | 先假设 serialize/run 同线程，文档标注；需求出现再加锁 |
| D9 | AGENTS.md 教学要求 | 新文件按现有风格解释"模拟现实的哪一部分" |
| D10 | mGBA 路线 | 优先 wasm side module（与现架构一致）；native dylib 走备选 host |

---

## 10. 验收与测试

**L1（RetroArch）**
- [ ] 加载 `fc_libretro.dylib` 不报缺符号
- [ ] 游戏出画面/出声/可操作
- [ ] 存档 → 重开 → 逐像素一致
- [ ] RetroArch 自带倒带可用（证明 serialize 完整）
- [ ] `.srm` 电池存档落盘/读回

**L2/L3**
- [ ] `fc_libretro_get_ext()` 可被 dlsym；缺失时前端优雅降级
- [ ] `RETRO_MEMORY_SAVE_RAM` / `SYSTEM_RAM` 指针与大小正确
- [ ] Game Genie 6/8 位与 PAR 代码生效；内存搜索可用

**L4/L5（wasm）**
- [ ] side module 能被宿主 dlopen/dlsym，回调与 framebuffer 指针正确
- [ ] 内存增长后视图重取，画面不冻结/不撕裂
- [ ] 现有 429 单测与 `wasm/verify.sh` 逐像素逐采样仍通过
- [ ] `CoreHost` 切换后现有 NES 行为零回归

**通用**
- [ ] 假 frontend 单测：直接调 `retro_*`，断言回调次数/内容
- [ ] 音频 mono→stereo、float→int16 单测
- [ ] 输入 id 映射单测

---

## 11. 明确不做

- 不重写 Core；libretro 适配层与 `fc_ffi` 并列存在。
- 不把前端功能塞进 core（库/封面/手柄助手/滤镜/音频队列都不进）。
- 不实现 FDS / UNIF / PAL / 多机种子系统。
- 本次不写 mGBA 集成代码，只预留 `CoreHost`、系统注册表、custom 扩展。
- 不删除 `custom_nes_core/src/ffi/emulator_api.h`。

---

## 附录 A：libretro 符号 ↔ 现有实现

| retro_* | 现有/新增实现 |
|---|---|
| `retro_init` / `retro_deinit` | `new/delete fc_machine`（静态单例） |
| `retro_load_game` / `retro_unload_game` | `fc_load_rom` / 释放 |
| `retro_reset` | `fc_reset` |
| `retro_run` | `fc_run_frame` + video + audio + input |
| `retro_serialize_size/serialize/unserialize` | `fc_state_size` / `fc_save_state_into` / `fc_load_state` |
| `retro_get_memory_data(SAVE_RAM)` | **新增** `Cartridge::prg_ram()` |
| `retro_get_memory_data(SYSTEM_RAM)` | **新增** `NesBus::ram_data()` |
| `retro_cheat_set/reset` | **新增** Game Genie/PAR 解码 → `CheatSet` |
| `retro_get_region` | 常量 NTSC |
| `retro_get_system_av_info` | 常量 256×240 / 60.0988 / 44100 |
| `fc_libretro_get_ext`（custom） | peek/poke/raw cheats/diagnostics |

## 附录 B：最小 environment 命令

```c
case RETRO_ENVIRONMENT_SET_PIXEL_FORMAT:      // XRGB8888
case RETRO_ENVIRONMENT_GET_LOG_INTERFACE:
case RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS:
case RETRO_ENVIRONMENT_SET_CONTROLLER_INFO:
case RETRO_ENVIRONMENT_GET_CAN_DUPE:          // true
case RETRO_ENVIRONMENT_SET_MEMORY_MAPS:       // $0000-$07FF
case RETRO_ENVIRONMENT_GET_VARIABLE:
case RETRO_ENVIRONMENT_SET_VARIABLES:
case RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY:
case RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY:
case RETRO_ENVIRONMENT_SET_GEOMETRY:
case RETRO_ENVIRONMENT_SET_SERIALIZATION_QUIRKS:
default: return false;
```

## 附录 C：wasm 加载实验复现

见 §7.2。要点：

```c
/* core.c（side module） */
__attribute__((export_name("retro_set_video_refresh")))
void retro_set_video_refresh(video_cb cb) { g_video = cb; }
__attribute__((export_name("retro_run")))
void retro_run(void) { if (g_video) g_video(frame, 256, 240); }
```

```c
/* main.c（frontend, MAIN_MODULE=1） */
void* h = dlopen("core.wasm", RTLD_NOW);
set_cb set = dlsym(h, "retro_set_video_refresh");
set(my_video);         /* 把 main 的函数指针交给 core */
for (...) run();       /* core 反向调用 main */
```

结论：libretro 的"回调注册 + 逐帧反向调用"模型在 wasm 动态链接下成立。
