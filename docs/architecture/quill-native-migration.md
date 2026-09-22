# 迁移到 quill 原生前端 —— 调研与落地计划

> 状态：**计划已落地，结构已建**（分支 `quill-native`）。本文件是这次重构的权威设计；
> 代码骨架在 `crates/`、`cores/`，旧栈整体移到 `legacy/` 只作参考。
> 决策已确认：目标核心是 **Mesen**（不是构建系统 Meson），UI 先做**最小闭环**，
> 手柄用 **gilrs**，自研核心保留在 `legacy/` 作对照。
> 目标：用 Rust + [quill](../../legacy/README.md)（`../quill`）重写前端，**只做 UI 与 libretro 兼容**，
> 接入 **Mesen**（NES）与 **mGBA**（GB/GBA）两个原生 libretro core。

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
| `cgb-systems` | lib | 纯领域：SystemId/CoreId/CoreChoice 注册表、joypad id、AV 表 | 无 |
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
| `SET_PIXEL_FORMAT` | 记下格式；只接受 `XRGB8888`（余下拒绝，core 退化） |
| `GET_SYSTEM_DIRECTORY` / `GET_SAVE_DIRECTORY` | 返回 `<app_data>/system` / `<app_data>/saves` |
| `SET_INPUT_DESCRIPTORS` | 存起来，供输入绑定 UI |
| `SET_CONTROLLER_INFO` | 存起来，供设置页列端口 |
| `GET_VARIABLE` / `SET_VARIABLES` / `GET_VARIABLE_UPDATE` | 只读变量表，先返回空串/false |
| `GET_LOG_INTERFACE` | 转发到 `tracing`/`eprintln` |
| `GET_CAN_DUPE` | `true` |
| `SET_GEOMETRY` | 更新当前画面几何 |
| `SET_PERFORMANCE_LEVEL` / `SET_ROTATION` / `GET_OVERSCAN` / `SET_MESSAGE` | 接受 / 忽略 |

### 5.3 视频

- 回调签名 `video_refresh(data: *const c_void, width, height, pitch)`。
- `pitch` 是**行字节数**，不等于 `width * 4`；必须按 pitch 逐行拷贝。
- 格式 `XRGB8888`（内存里 little-endian 是 `B,G,R,X`）→ 转成 quill 后端要的 RGBA8：
  每像素 swizzle（`swap R/B`）。256×240 ≈ 61k 像素/帧，CPU 可接受；
  后续可在 `draw_backend_wgpu` 增加纹理格式参数省掉这一步（记在风险里）。
- `data == null` 且允许 dupe 时表示重复上一帧。
- 分辨率/帧率来自 `retro_get_system_av_info`（**mGBA 必须 load 之后读才准**），
  GBA 240×160、GB 160×144，都要动态处理，不能写死。
- quill 侧：`WgpuBackend::{register_texture, update_texture}` + `TextureFilter::Nearest`
  + `DrawImage`（整数倍缩放由宿主算 destination）。
- **缺口（Q1 必解）**：`draw_ui::Widget` 没有 image 变体，`draw_components` 没有
  `Image`，所以 view 目前**发不出** `DrawImage`。两条路：(a) 给 quill 加
  `Widget::Image` + `draw_components::Image`（推荐，属于 quill）；(b) 过渡期在 app
  里把画面直接画进 `PaintContext`。几何规则见 `crates/cgb-ui/src/frame.rs::integer_fit`。

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

---

## 6. Mesen / mGBA 原生构建（`cores/`）

两个核心都自带 libretro 桌面 Makefile，且都有 `osx` platform 分支（已确认）。
把 `legacy/wasm/*/build.sh` 的逻辑改成原生版：

```bash
# cores/mesen/build.sh
git clone --depth 1 https://github.com/libretro/Mesen  →  make -f Libretro/Makefile platform=osx
# cores/mgba/build.sh
git clone --depth 1 https://github.com/libretro/mgba   →  make -f Makefile.libretro platform=osx
# 产物 → cores/dist/{mesen,mgba}_libretro.dylib
```

需要保留的 patch（与 wasm 版相同理由）：
- mGBA：去掉 `-DCOLOR_16_BIT`（保持 XRGB8888）、去掉 `-DHAVE_CRC32`（自带 crc32）。
- Mesen：wasm 版的「内存卡带」patch **原生不需要**（有文件系统）；
  C++ 异常是原生默认，也**不需要**。
- 两者都要求 Apple Silicon arm64；Mesen 1.x（C++11）是最可能踩坑的一处，先做 spike。

> `cores/` 的产物（`cores/dist/`、`cores/sources/`）加入 `.gitignore`，按需构建。

---

## 7. 最小闭环 UI（`cgb-ui`）

左栏 3 个 section（`Router` 或 `set_visible` 切换）：

| section | 内容 | quill 组件 |
|---|---|---|
| 游戏库 | 扫描目录、扩展名过滤 `.nes/.gba/.gb/.gbc`、机种徽章、双击进入播放 | `List` + `ListColumn` + `Badge` |
| 播放 | 画面（`DrawImage`）、暂停/复位/全屏、存档槽、当前核心与机种 | `Card` + `Button` + `DrawImage` |
| 设置 | 每机种选核、键盘绑定、手柄绑定、扫描目录 | `Overlays` + `Checkbox` + `Switch` |

结构上预留但**暂不实现**：搜索框（quill 无 TextInput，自建或后置）、截图/封面、标签、金手指。
搜索框是已知缺口，若最小闭环需要，先在 `cgb-ui` 内自建一个轻量 `TextInput`。

**画面同样有缺口**：`cgb-ui` 现在只能显示占位文字，因为 UI 栈没有 image 组件
（见 §5.3）。Q1 的“出画面”要么先给 quill 加组件，要么用 §5.3 的过渡方案。

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
| **Q1** | Mesen spike：原生 arm64 编译 + dlopen + 出画面 + 键盘 | 打开一个 NES ROM 能看到画面 |
| **Q2** | 音频（cpal）+ gilrs 手柄 + 存档槽 + `.srm` | 能玩、能存读 |
| **Q3** | `cgb-ui` 最小闭环 + 库（SQLite）+ 打开目录对话框 | 从库列表选游戏进入游玩 |
| **Q4** | mGBA 接入 + 机种路由 + 动态分辨率/帧率/输入描述 | `.gba/.gb/.gbc` 可玩 |
| **Q5** | 打包 `.app`、无头自检、发版脚本 | 可发布，`--selfcheck` 绿 |

---

## 10. 风险

1. **Mesen 1.x 的 arm64 原生编译**——最不确定，Q1 先 spike；不行则换 Mesen2 或只留 mGBA。
2. **quill 无 TextInput / 无音频 / 无手柄**——音频手柄自建（本计划已定），TextInput 后置。
3. **每帧 XRGB8888→RGBA swizzle** 有 CPU 成本；量大再给后端加纹理格式。
3b. **quill 没有 Image 组件**——目前 view 画不出模拟器画面。Q1 必须先给 quill 加
   `Widget::Image` / `draw_components::Image`，或走 app 内过渡方案。
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
