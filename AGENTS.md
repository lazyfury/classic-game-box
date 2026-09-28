# Classic Game Box — 工作约定（Rust 版）

本仓库正在从 **Electron + WebAssembly + 自研核心** 迁移到 **原生 Rust + igui + libretro**。
旧栈整体在 `legacy/`，只作参考，不参与构建。权威设计见
[`docs/architecture/quill-native-migration.md`](docs/architecture/quill-native-migration.md)。

UI 栈是 [`igui`](https://github.com/lazyfury/igui)（`quill` 改名后的上游），以 git 依赖固定到
`v0.2.0` 的 commit；见 `Cargo.toml` 的 `igui` / `igui_svg` / `igui_winit` / `igui_core`。
不再需要相邻的 `../quill` checkout。

## 现状

- 分支 `quill-native`。
- **Q0 完成**：计划、目录结构、Rust 工作区骨架、`cargo check/test/clippy` 全绿。
- **Q1 完成**：Mesen 原生 arm64 编译 + dlopen + 出画面（`cgb-ui::frame::FrameImage`）+ 键盘。
- **Q2 进行中**：音频（cpal）+ gilrs 手柄已接线，`.srm` 电池存档与即时存档槽
  （`Session::{save,load}_state`，F5/F6 与 F1–F3/Shift+F1–F3），待人眼验收“能玩、能存读”。
- **Q3 完成**：库模型重建——DB 是模型（`games` + `tags`/`game_tags` + `screenshots`），
  **单库、自包含、可切换**：`--library-dir` / “打开游戏库…”选定唯一库根，
  DB / 截图 / 存档 / 金手指都在库根下，整个文件夹拷走即备份（不合并多个目录）；
  旧 `library_dirs` 只在迁移时读一次（取第一个）。app data 只留 `settings.json`、
  `cores` 与 `system`（BIOS 不放库里，避免 `neogeo.zip` 被扫成街机 ROM）。未选库时
  沿用旧布局。
  `name` 与 `file_name` 分离、可改名（`Library::rename`，UI 后置）；游玩次数/时长/最近；
  库顶统计总数量 + 按机种分别计数并点选筛选；
  置顶；排序（名称/大小/最近/时长/加入）；标签；截图（F12 / ⇧F12 设封面）+ 真实封面；
  卡片 SVG 图标 + 截图收藏页 + 大图预览。
- **性能**：DrawList 复用（运行游戏不重排重绘）+ 图标纹理化 + 库网格可见行虚拟化；
  `CGB_PERF=1` 打点。
- **Q5 打包**：`scripts/package-macos.sh` / `scripts/release.sh` 出 macOS `.app` + zip
  （cores/assets 入 Resources）；`--selfcheck` 无头自检。
- **Q6 定制**：输入能力对齐（16 键 + 模拟轴 + capabilities + 按机种绑定 + core descriptors）；
  存档 10 槽（按 core 隔离 + 缩略图）；金手指 `.cht`；倒带（每 2 帧 / 10s）；画面后处理
  shader（扫描线/CRT/LCD/锐化）；core options + `SET_CONTROLLER_PORT_DEVICE` + core 消息；
  截图多选删除；中栏可拖动；全屏游玩（`ViewModel::fullscreen`，F11/play 列
  “全屏”按钮进入，Esc 退出；只挂载右栏游戏视图，库网格与侧栏不入树）。
  后处理用 `igui_backend_wgpu::TextureEffect`（`igui` 自 `v0.2.0` 提供）。
- **运行时**：`cgb-app` 跑在 igui 的 `igui_app` 插件运行时上（`WinitPlugin` /
  `WgpuPlugin` / `PointerPlugin` / `KeyboardPlugin` / `ImePlugin` / `TextMeasurePlugin` /
  `ClipboardPlugin`），`App` 实现 `AppLogic`（`update/layout/paint`）。帧由
  `Session::advance(dt)` 的时间累积驱动，`needs_frame` 在跑游戏/带动画 overlay/倒带时为真。
  改名 / 搜索 / 标签编辑用上游 `igui_components::TextInput`（自带 caret/选区/IME 预编辑）。
- **核心清单统一**：所有核心都从单一 `cores/cores.json` 加载
  （mesen / mgba / nestopia / custom_nes_core / fbneo / genesis_plus_gx / picodrive）；
  `--core` 按 key 或路径选核。
  mGBA 用上游 `libretro/mgba`（CMake）构建，输出 **RGB565**，宿主已接受并转换。
  Sega 系（genesis / sms / gg / sg1000）有两个核心：Genesis Plus GX 与轻量的
  PicoDrive（同一模块四个机种，PicoDrive 是首个带 git submodule 的核心）。
  街机是 `SystemId::Arcade`（`.zip` → FBNeo，按 CRC 读标准 Neo Geo 套）。

## 硬规则

1. **根目录以 Rust 为主。** 顶层只有 `Cargo.toml`、`crates/`、`cores/`、`scripts/`、
   `assets/`、`docs/`、`legacy/`。不要往根目录丢构建产物或临时文件。
2. **只做 UI 与 libretro 兼容。** 不实现/移植自研模拟器；它已在 `legacy/`。
   新功能先问「libretro 有没有标准对应」。
3. **libretro 是唯一对外契约。** 只加载标准 libretro core（`cores/cores.json`
   里声明的，含 Mesen、mGBA、nestopia 与 legacy 的 `custom_nes_core`）；
   不使用 `fc_*` 私有扩展（`custom_nes_core` 会导出该扩展，但被忽略）。
   ABI 头是 `cores/libretro/libretro.h`；**不要整读**（≈8700 行），`rg` 定位再看。
4. **依赖方向单向**：`cgb-app → {cgb-ui, cgb-libretro, cgb-audio, cgb-input,
   cgb-library, cgb-systems}`；`cgb-ui` 不认识 libretro；`cgb-libretro` 不认识 UI
   与音频设备（只暴露 `Frame` / `Vec<i16>`）。`cgb-systems` 无依赖。
5. **igui 的边界**：`igui_core` / `igui_scene` / `igui_ui` / `igui_components`
   不得依赖 `web_sys`/`wgpu`/DOM。`cgb-ui` 只用 `igui_*` 的公开 API。
6. **不写截图 / 录屏测试。** 用 `igui_backend_recording` 录 `DrawList` +
   `igui_profile::inspect`，或 core 侧假 frontend 单测。UI 好不好看由人看。
7. **不改 `legacy/`**，除非明确要求；它是历史存档。
8. **不确定就问，不要猜。** 需求模糊、要动公共 API 或路线图时，先停下来问。
9. **不擅自开工。** 只实现已确认的任务；顺手发现的问题只汇报，不动手。

## 构建与验证（每阶段 gate）

```bash
./scripts/dev.sh          # fmt --check + clippy -D warnings + test
# 等价于：
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

原生 core 是第三方项目，按需构建（需网络，首次几分钟）：

```bash
./scripts/build-cores.sh   # → cores/dist/{mesen,mgba}_libretro.dylib
cargo run -p cgb-app -- --rom /path/to/mario.nes
cargo run -p cgb-app -- --rom mario.nes --core mesen           # 强制核心
cargo run -p cgb-app -- --rom mario.nes --core ./mycore_libretro.dylib  # 任意模块
./scripts/package-macos.sh  # → dist/Classic Game Box.app（含 cores + assets）
./scripts/release.sh        # gate + selfcheck + 版本戳 + 打包 + zip（dist/…-<版本>.zip）
cargo run -p cgb-app -- --selfcheck  # 无头自检（paths/library/settings/icons/render/cores）
```

## 目录地图

| 需要… | 看这里 |
|---|---|
| 迁移总设计、范围、里程碑、风险 | `docs/architecture/quill-native-migration.md` |
| 旧架构的来龙去脉（为什么用 wasm、为什么现在不用） | `legacy/docs/architecture/libretro-migration.md` |
| crate 职责与依赖 | `crates/README.md` |
| libretro frontend（dlopen / 回调 / 视频音频输入存档） | `crates/cgb-libretro/src/host.rs` |
| 机种 / CoreSpec 选核、joypad id | `crates/cgb-systems/src/` |
| UI 视图与帧循环 | `crates/cgb-ui/src/`、`crates/cgb-app/src/app.rs`（`AppLogic` + `run` 的插件组装） |
| 原生 core 构建 / 加核心流程 | `cores/README.md`、`cores/build.sh.example`、`cores/*/build.sh` |
| 核心清单（启动选核） | `cores/cores.json`、`crates/cgb-library/src/cores.rs`、`crates/cgb-app/src/cli.rs` |
| 旧 Electron/C++/wasm 栈 | `legacy/`（只读） |

## 已知缺口（先记录，不擅自补）

- igui 仍没有**标准 Image 内容类型**（上游 Stage 33 删了 `Widget`，改用
  `ControlContent`）：`cgb-ui/src/frame.rs` 的 `FrameImage` 仍用 `Component` +
  `Spec::foreground` 自绘 `DrawImage`。上游若有 image 内容类型，可考虑替换本地组件。
- **中文输入法（IME）** 已接：改名 / 搜索 / 标签用 `igui_components::TextInput`
  （自带 caret / 选区 / IME 预编辑），窗口由 `igui_winit::ImePlugin` 驱动，候选窗按
  `AppLogic::caret`（`focused_caret`）定位；但还未经人眼验证 CJK 组合。
- **帧循环**改由 `igui_app` 运行时驱动。`AppLogic::needs_frame` 在跑游戏 / 带动画的
  overlay / 倒带时为真；帧速由 wgpu `PresentMode::Fifo`(vsync) 限速，`Session::advance(dt)`
  的时间累积保证模拟速度。若要精确按核心帧率 `WaitUntil` 节流，参考 `../archiver` 的
  本地 `EventLoop<UserEvent>` runner（`src/host/runner.rs`）。
