# 迁移到 quill 原生前端 —— 调研与落地计划

> 状态：**计划已落地，结构已建**（分支 `quill-native`）。本文件是这次重构的权威设计；
> 代码骨架在 `crates/`、`cores/`，旧栈整体移到 `legacy/` 只作参考。
> 决策已确认：目标核心是 **Mesen**（不是构建系统 Meson），UI 先做**最小闭环**，
> 手柄用 **gilrs**，自研核心保留在 `legacy/` 作对照。
> 目标：用 Rust + [quill](../../legacy/README.md)（`../quill`）重写前端，**只做 UI 与 libretro 兼容**，
> 接入 **Mesen**（NES）、**mGBA**（GB/GBA）与 **MAME 2003-Plus**（街机）等原生 libretro core。

---

## 0. 结论（TL;DR）

1. **Electron + WebAssembly 前端退役**。新前端是原生 Rust：`winit` + `wgpu` + quill。
2. **自研模拟器退役**。`fc-core` / `fc-libretro` / custom ABI 扩展不再作为运行路径，
   整体移到 `legacy/`（保留作教学与对照测试基准，不再构建进产品）。
3. **libretro 仍是唯一契约**。新前端实现 libretro **frontend** 一侧（environment / video /
   audio / input / serialize / memory），只加载**标准** libretro core，不再有 `fc_*` 私有扩展。
4. **原生 `.dylib` 直接 dlopen**。这是这次重构最大的红利：彻底绕开 wasm 沙箱、
   side module、`ALLOW_MEMORY_GROWTH=0`、C++ 运行时对齐等全部技术债。
   任意第三方 libretro core 即插即用。
5. **UI 先做最小闭环**：游戏库 → 选核 → 游玩 → 暂停/复位 → 存读档 → 设置。截图、封面、
   置顶、标签、金手指、倒带、扫描线等**移植优先级低**，结构上留位、按需接回。
6. **手柄用 `gilrs`**（不再依赖 Swift 助手进程）。

---

## 1. 范围

### 做
- 原生窗口 + GPU 上屏（quill `draw_backend_wgpu`）。
- libretro frontend：dlopen core、回调注册、逐帧运行、XRGB8888 画面、int16 stereo 音频、
  键盘/手柄输入、`retro_serialize` 存档与电池存档 `RETRO_MEMORY_SAVE_RAM`。
- Mesen 与 mGBA 的**原生 macOS 构建**（`cores/`）。
- 最小闭环 UI：库列表（虚拟化 `List`）、播放页、设置页、存档槽、手柄重绑定。
- 游戏库持久化（SQLite）与设置持久化（JSON）。
- 无截图的自动验证（`draw_backend_recording` + `draw_profile::inspect`）。

### 不做（本阶段）
- 自研 CPU/PPU/APU 模拟逻辑；`fc_*` 私有扩展；金手指原始字节面板。
- 截图/封面/置顶/标签的完整 UI（数据结构可保留）。
- 倒带、扫描线滤镜、多人手柄的完整矩阵（预留接口，按需接回）。
- Windows/Linux 打包；只保证 macOS Apple Silicon。

---

## 2. 目标架构

```
┌───────────────────────────────────────────────────────────────┐
│ cgb-app   (bin: classic-game-box)                              │
│   winit EventLoop(WaitUntil, 按 core fps) → wgpu surface 上屏  │
│   拥有：窗口、surface、帧循环、把各 crate 接线                 │
├───────────────┬───────────────┬───────────────┬───────────────┤
│ cgb-ui        │ cgb-libretro  │ cgb-audio     │ cgb-input     │
│ quill 视图    │ dlopen + 回调 │ cpal + 队列   │ 键盘 + gilrs  │
│ (无平台依赖)  │ libloading    │               │               │
├───────────────┴───────────────┴───────────────┴───────────────┤
│ cgb-library (SQLite 库 / 设置 / 存档槽 / .srm)                 │
├───────────────────────────────────────────────────────────────┤
│ cgb-systems (纯领域)：机种、核心注册表、joypad id、AV 信息     │
└───────────────────────────────────────────────────────────────┘
        │ dlopen
        ├── mesen_libretro.dylib   (NES)
        └── mgba_libretro.dylib    (GB / GBA)
```

依赖方向只有一条，不允许反向：

```
cgb-app  →  { cgb-ui, cgb-libretro, cgb-audio, cgb-input, cgb-library, cgb-systems }
cgb-ui        →  cgb-systems, quill(draw_*)
cgb-libretro  →  cgb-systems, libloading
cgb-input     →  cgb-systems, gilrs
cgb-audio     →  (cpal)
cgb-library   →  rusqlite
cgb-systems   →  (无)
```

`cgb-ui` **不认识 libretro**：它只吃一个纯数据 `ViewModel`，由 `cgb-app` 从 core 状态投影出来。
这样 UI 可以在 recording backend 上无头测试。

---

## 3. 仓库结构（before → after）

```
# before（main）
packages/fc-core  packages/fc-libretro  wasm/  electron/  tools/  cmake/  CMakeLists.txt

# after（quill-native）
Cargo.toml                  # Rust workspace
crates/                     # 新前端，全部 Rust
cores/                      # Mesen / mGBA 原生 dylib 构建脚本 + patches
scripts/                    # build-cores.sh / dev.sh
assets/                     # 图标等
docs/                       # 教学文档保留；architecture/ 放本计划
legacy/                     # 旧栈整体搬家，只读参考，不参与构建
  ├── packages/fc-core
  ├── packages/fc-libretro
  ├── wasm/  electron/  tools/  cmake/  CMakeLists.txt
  ├── scripts/  .vscode/  README.md  AGENTS.md
  └── .pi/skills/           # 旧的 fc-* 技能
```

**根目录以 Rust 为主**：顶层只有 Cargo 工作区、`cores/`、`scripts/`、`docs/`、`assets/`、`legacy/`。

---

## 4. crate 职责

| crate | 类型 | 职责 | 关键依赖 |
|---|---|---|---|
| `cgb-app` | bin | winit 事件循环、wgpu surface、帧循环、接线 | winit, draw_backend_wgpu, 全部内部 crate |
| `cgb-ui` | lib | quill 视图：侧栏、库列表、播放页、设置页、存档 | draw_{core,scene,ui,render,theme,components}, cgb-systems |
| `cgb-libretro` | lib | libretro frontend：dlopen、回调、ABI 类型 | libloading, cgb-systems |
| `cgb-systems` | lib | 纯领域：SystemId、CoreSpec、选核、joypad id | 无 |
| `cgb-audio` | lib | cpal 输出 + 无锁样本队列（int16 stereo） | cpal, ringbuf |
| `cgb-input` | lib | 键盘绑定 + gilrs → joypad 位掩码 | gilrs, cgb-systems |
| `cgb-library` | lib | SQLite 库、设置 JSON、存档槽、`.srm` | rusqlite, serde_json |

---

## 5. libretro host 设计（`cgb-libretro`）

### 5.1 加载顺序（libretro ABI 强制）

```
dlopen(dylib)
  → retro_set_environment(fn)      # 必须最先
  → retro_init()
  → retro_set_video_refresh / audio_sample_batch / input_poll / input_state
  → retro_load_game(&retro_game_info)     # ROM 以内存指针传入
  → 每帧 retro_run()
  → retro_unload_game() / retro_deinit() / dlclose
```

`.dylib` 路径由 `cgb-systems` 的核心注册表给出：
`mesen_libretro.dylib` / `mgba_libretro.dylib`，放在 app 的 `cores/` 资源目录里。

### 5.2 environment 回调（frontend → core）

最小必需集合：

| 命令 | 处理 |
|---|---|
| `SET_PIXEL_FORMAT` | 接受 `XRGB8888` 与 `RGB565`；其余拒绝，core 保持 `0RGB1555` |
| `GET_SYSTEM_DIRECTORY` / `GET_SAVE_DIRECTORY` | 返回 `<app_data>/system` / `<app_data>/saves` |
| `SET_INPUT_DESCRIPTORS` | 存起来，供输入绑定 UI |
| `SET_CONTROLLER_INFO` | 存起来，供设置页列端口 |
| `GET_VARIABLE` / `SET_VARIABLES` / `GET_VARIABLE_UPDATE` | 只读变量表，先返回空串/false |
| `GET_LOG_INTERFACE` | 返回一个真实 sink（转 `eprintln`）。MAME 系核心无条件调用该指针，返回 false 会让它拿到空指针而崩溃 |
| `GET_CAN_DUPE` | `true` |
| `SET_GEOMETRY` | 更新当前画面几何 |
| `SET_PERFORMANCE_LEVEL` / `SET_ROTATION` / `GET_OVERSCAN` / `SET_MESSAGE` | 接受 / 忽略 |

### 5.3 视频

- 回调签名 `video_refresh(data: *const c_void, width, height, pitch)`。
- `pitch` 是**行字节数**，不等于 `width * 4`；必须按 pitch 逐行拷贝。
- 格式 `XRGB8888`（内存里 little-endian 是 `B,G,R,X`）→ 转成 quill 后端要的 RGBA8：
  每像素 swizzle（`swap R/B`）。256×240 ≈ 61k 像素/帧，CPU 可接受；
  后续可在 `draw_backend_wgpu` 增加纹理格式参数省掉这一步（记在风险里）。
- 也接受 **`RGB565`**（上游 mGBA 的输出）：每像素 2 字节。每像素字节数（`bpp`）
  由格式决定，**行切片按 `width * bpp`**，不是写死的 `width * 4`；
  否则 mGBA（240 宽、pitch 512）会越界。`SET_PIXEL_FORMAT` 只在接受时才记录格式，
  被拒时保持 `0RGB1555`，否则会按错误的格式解读核心输出。
- `data == null` 且允许 dupe 时表示重复上一帧。
- 分辨率/帧率来自 `retro_get_system_av_info`（**mGBA 必须 load 之后读才准**），
  GBA 240×160、GB 160×144，都要动态处理，不能写死。
- quill 侧：`WgpuBackend::{register_texture, update_texture}` + `TextureFilter::Nearest`
  + `DrawImage`（缩放由宿主算 destination）。
- **Q1 已接（过渡方案）**：`draw_ui::Widget` 仍无 image 变体，但 `cgb-ui` 用 quill
  的公开扩展点自建了叶组件 `frame::FrameImage`（实现 `draw_components::Component`，
  在 `foreground` 装饰器里发 `DrawImage`，与 `Divider` 同一条路）。它按
  [`contain_fit`](../../crates/cgb-ui/src/frame.rs) 按比例放大到**撑满较短的一边**、
  较长的一边居中留黑边（非整数缩放；像素会略不均匀，换取画面尽量大）。
  **仍待做（属于 quill）**：`Widget::Image` + `draw_components::Image` 才是上游正解，
  这样任何 view 都能画图；本地组件只是不阻塞 Q1。

### 5.4 音频

- 回调 `audio_sample_batch(data: *const i16, frames) -> usize`：int16 **stereo**。
- 写进 `cgb-audio` 的无锁环形队列；cpal 输出线程按 core 的采样率（NES 44100 /
  Mesen 48000 / GBA 65536 / GB 131072，来自 `av_info.timing.sample_rate`）拉取。
- **回调里不加锁**（实时约束，旧项目踩过）。

### 5.5 输入

- `input_poll` 空实现；`input_state(port, device, index, id) -> i16` 返回按键电平。
- 键盘与 gilrs 各自维护按下集合，取 OR；避免互相覆盖（旧项目约定）。
- 设备 id 与 `RETRO_DEVICE_ID_JOYPAD_*` 的映射在 `cgb-systems::joypad`，
  `cgb-input` 与 `cgb-libretro` 共用，不硬编码在 core 或 UI 里。

### 5.6 存档与电池

- 即时存档/倒带：`retro_serialize_size` / `retro_serialize` / `retro_unserialize`（opaque 字节）。
- 电池存档：`retro_get_memory_data(RETRO_MEMORY_SAVE_RAM)` + size，落盘 `<saves>/<rom>.srm`，
  加载 ROM 后写回。mGBA/Mesen 都靠这个。
- 存档目录/命名由 `cgb-library` 统一管理。
- **Q2 已接线**：`cgb-app::Session` 在 `load_game` 后回写 `.srm`，暂停/退出/换游戏时落盘；
  即时存档走 `Session::{save,load}_state(slot)`，槽位 0 为快速槽，1–3 为命名槽；
  热键 F5/F6 = 快速存/读，F1–F3 存、Shift+F1–F3 读（沿用旧前端约定）。

---

## 6. Mesen / mGBA 原生构建（`cores/`）

**Q1 实测结论：**
- **Mesen**：`libretro/Mesen` 自带 `Libretro/Makefile` 与 `osx` 分支，
  `make -f Makefile platform=osx` 在 arm64 上编译通过，产出
  `mesen_libretro.dylib`（3.1MB arm64），`nm -gU` 确认导出全部 `retro_*`。
  这是 Q1 的“通”。wasm 版的内存卡带 patch 与异常 flag 原生都不需要。
- **mGBA：已构建。** 上游 `libretro/mgba` 现在只有 CMake
  （`-DBUILD_LIBRETRO=ON`），需要 cmake。产出 240×160 @ 59.73fps / 65536Hz，
  arm64 + `retro_*` 导出齐全。
  上游硬编码 `COLOR_16_BIT;COLOR_5_6_5`（**RGB565**），**不再打 patch**：
  宿主接受并转换 RGB565（见 §5.2 / §5.3）。旧的 `EmulatorJS/mgba` +
  `Makefile.libretro` 路径已弃用。

`cores/` 的构建脚本把这两个第三方的逻辑从 wasm 移植成原生版：

```bash
# cores/mesen/build.sh（已验证）
git clone --depth 1 https://github.com/libretro/Mesen → make -f Libretro/Makefile platform=osx
# cores/mgba/build.sh（已验证）
git clone --depth 1 https://github.com/libretro/mgba  → cmake -DBUILD_LIBRETRO=ON …
# 产物 → cores/dist/{mesen,mgba}_libretro.dylib
```

需要保留的 patch：
- Mesen：wasm 版的「内存卡带」patch 与 C++ 异常 flag **原生都不需要**
  （有文件系统、异常默认开）。
- mGBA：不需要 patch（RGB565 由宿主转换）。
- 都要求 Apple Silicon arm64，均已 `nm -gU` 验证 `retro_*`。`cores/dist/*.dylib`
  过 `crates/cgb-libretro/tests/cores_run_through_the_host.rs`（合成 ROM）。

> `cores/` 的产物（`cores/dist/`、`cores/sources/`）加入 `.gitignore`，按需构建。

### 6.1 cores.json：核心清单

**所有**核心都是数据，只有一个 [`cores/cores.json`](../../cores/cores.json)：
mesen、mGBA（×2 机种）、nestopia、custom_nes_core。

条目为 `key` / `name` / `system` / `dylib`（+ 可选 `sample_rate` / `fps`）。`key`
每机种唯一（同一模块可服务两机种，如 mGBA）；重复 `(system, key)` 保留第一个。
设置里记的 key 若清单没有，回退到该机种默认。

**构建流程**：
- 每个核心一个目录：`cores/<name>/build.sh` 产出到 `cores/dist/`。
- `scripts/build-cores.sh` 跑所有 `cores/*/build.sh`（`--skip-mgba` 跳过 mGBA）。
- 加核心 = 加目录 + 在 `cores.json` 加一行，不动 Rust。详见
  [`cores/README.md`](../../cores/README.md)。`custom_nes_core` 是唯一不用
  cmake/第三方的：直接 clang++ 编译只读的 `legacy/packages/fc-*`
  （`legacy/` 只读；`fc_*` 私有扩展被前端忽略）。

**运行**：

```bash
cargo run -p cgb-app -- --rom mario.nes --core mesen
cargo run -p cgb-app -- --rom mario.nes --core nestopia              # 清单里的 key
cargo run -p cgb-app -- --rom mario.nes --core ./x_libretro.dylib    # 直接指模块
```

- `--core <key>` 在合并后的清单里按 `(机种, key)` 查；`--core <path>` 按原样 dlopen。
  机种由 ROM 扩展名推断，帧率/采样率在 load 后从核心 `av_info` 读。
- 类型：`cgb-systems::CoreSpec`（owned，带 `key`）+ 纯函数 `choose_core`；
  清单解析在 `cgb-library::load_cores`；`App::find_module` 解析 dlopen 路径
  （绝对/存在的路径→原样，否则打包 `cores/` → dev `cores/dist/`）。
  `cores_for_system` 是 Q3 设置页选核列表的数据源。
- 设置持久化按**任意 key 字符串**（`Settings::core_key`），不再是枚举。

---

## 7. 最小闭环 UI（`cgb-ui`）

左栏 3 个 section（`Router` 或 `set_visible` 切换）：

| section | 内容 | quill 组件 |
|---|---|---|
| 游戏库 | 扫描目录、扩展名过滤 `.nes/.gba/.gb/.gbc/.zip`、机种徽章、双击进入播放 | `List` + `ListColumn` + `Badge` |
| 播放 | 画面（`DrawImage`）、暂停/复位/全屏、存档槽、当前核心与机种 | `Card` + `Button` + `DrawImage` |
| 设置 | 每机种选核、键盘绑定、手柄绑定、扫描目录 | `Overlays` + `Checkbox` + `Switch` |

结构上预留但**暂不实现**：搜索框（quill 无 TextInput，自建或后置）、截图/封面、标签、金手指。
搜索框是已知缺口，若最小闭环需要，先在 `cgb-ui` 内自建一个轻量 `TextInput`。

**画面已能上屏**：`cgb-ui` 的 `frame::FrameImage`（见 §5.3）在 `foreground`
装饰器里发 `DrawImage`，播放页因此显示真实画面；上游 `Image` 组件仍是待补项。

---

## 8. 帧循环（quill Phase 7 规避）

quill 示例是事件驱动（`ControlFlow::Wait`，只在变化时重绘）；模拟器需要持续 60fps。
`cgb-app` 自己处理：

```
ControlFlow::WaitUntil(now + frame_budget)
  → about_to_wait: 若到下一帧时刻，retro_run() + update_texture + request_redraw
  → RedrawRequested: update → layout → paint → submit → present
```

- 没有游戏时退回事件驱动（`Wait`），不空转。
- 画面用 `draw_backend_wgpu::TextureId` 流式更新（`update_texture`，不重新分配）。

---

## 9. 分阶段里程碑

| 阶段 | 内容 | 验收 |
|---|---|---|
| **Q0** ✅ | 计划 + 结构 + 脚手架 | `cargo check --workspace` 通过 |
| **Q1** | Mesen spike：原生 arm64 编译 + dlopen + 出画面 + 键盘 | ✅ 编译/ABI/dlopen/键盘齐，画面经 `frame::FrameImage` 上屏，待人眼确认 |
| **Q2** | 音频（cpal）+ gilrs 手柄 + 存档槽 + `.srm` | 🚧 音频/手柄/`.srm`/即时存取已接线，待人眼试听与存读验收 |
| **Q3** | `cgb-ui` 最小闭环 + 库（SQLite）+ 打开目录对话框 | 从库列表选游戏进入游玩 |
| **Q4** | mGBA 接入（原生）+ 机种路由 + 动态分辨率/帧率/输入描述 | 🚧 上游 mGBA 已构建并过 host（RGB565）；`.gba` 整机与输入描述待做 |
| **Q5** | 打包 `.app`、无头自检、发版脚本 | 可发布，`--selfcheck` 绿 |

---

## 10. 风险

1. **Mesen 1.x 的 arm64 原生编译**——最不确定，Q1 先 spike；不行则换 Mesen2 或只留 mGBA。
2. **quill 无 TextInput / 无音频 / 无手柄**——音频手柄自建（本计划已定），TextInput 后置。
3. **每帧 XRGB8888/RGB565→RGBA 转换** 有 CPU 成本；量大再给后端加纹理格式。
3b. **quill 没有 Image 组件**——Q1 已用 `cgb-ui::frame::FrameImage`（`Component` +
   `foreground` 装饰器）绕过；上游补 `Widget::Image` / `draw_components::Image` 后，
   这个本地组件可以撤掉。
4. **连续帧循环** quill 未原生支持，需在 `cgb-app` 自建（方案见 §8）。
5. **UI 功能面大**（旧前端约 1 万行 TS）——本阶段只做最小闭环，不追 1:1。
6. **quill 以 path 依赖 `../quill`**——需要同级 checkout；后续可改成 git rev 锁定。

---

## 11. 验收（无截图）

沿用 quill / 旧仓库的「不截图」纪律：

- `cargo check --workspace` / `cargo test --workspace` / `cargo fmt --check`。
- UI 用 `draw_backend_recording` 录 `DrawList`，`draw_profile::inspect` 查结构错误，
  断言文字与关键命令，不驱动真实窗口。
- core 侧用假 frontend 单测：直接调 `retro_*`，断言回调次数与内容（沿用旧 `test_libretro.cpp` 思路）。
- 端到端：加载合成 NROM / 一个真实 ROM，跑 N 帧，像素哈希稳定。

「好不好看」由人看，不由断言。
