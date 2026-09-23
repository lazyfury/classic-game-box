# Classic Game Box — 工作约定（Rust 版）

本仓库正在从 **Electron + WebAssembly + 自研核心** 迁移到 **原生 Rust + quill + libretro**。
旧栈整体在 `legacy/`，只作参考，不参与构建。权威设计见
[`docs/architecture/quill-native-migration.md`](docs/architecture/quill-native-migration.md)。

## 现状

- 分支 `quill-native`。
- **Q0 完成**：计划、目录结构、Rust 工作区骨架、`cargo check/test/clippy` 全绿。
- **Q1 完成**：Mesen 原生 arm64 编译 + dlopen + 出画面（`cgb-ui::frame::FrameImage`）+ 键盘。
- **Q2 进行中**：音频（cpal）+ gilrs 手柄已接线，`.srm` 电池存档与即时存档槽
  （`Session::{save,load}_state`，F5/F6 与 F1–F3/Shift+F1–F3），待人眼验收“能玩、能存读”。
- **Q3 完成**：库模型重建——DB 是模型（`games` + `tags`/`game_tags` + `screenshots`），
  `name` 与 `file_name` 分离、可改名（`Library::rename`，UI 后置）；游玩次数/时长/最近；
  置顶；排序（名称/大小/最近/时长/加入）；标签；截图（F12 / ⇧F12 设封面）+ 真实封面；
  卡片 SVG 图标 + 截图收藏页 + 大图预览。
- **性能**：DrawList 复用（运行游戏不重排重绘）+ 图标纹理化 + 库网格可见行虚拟化；
  `CGB_PERF=1` 打点。
- **Q5 打包**：`scripts/package-macos.sh` / `scripts/release.sh` 出 macOS `.app` + zip
  （cores/assets 入 Resources）；`--selfcheck` 无头自检。
- **核心清单统一**：所有核心都从单一 `cores/cores.json` 加载
  （mesen / mgba / nestopia / custom_nes_core / fbneo）；`--core` 按 key 或路径选核。
  mGBA 用上游 `libretro/mgba`（CMake）构建，输出 **RGB565**，宿主已接受并转换。
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
5. **quill 的边界**：`draw_core/draw_scene/draw_ui/draw_render` 不得依赖
   `web_sys`/`wgpu`/DOM。`cgb-ui` 只用 `draw_*` 的公开 API。
6. **不写截图 / 录屏测试。** 用 `draw_backend_recording` 录 `DrawList` +
   `draw_profile::inspect`，或 core 侧假 frontend 单测。UI 好不好看由人看。
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
cargo run -p cgb-app -- --selfcheck  # 无头自检（paths/library/settings/cores/icons）
```

## 目录地图

| 需要… | 看这里 |
|---|---|
| 迁移总设计、范围、里程碑、风险 | `docs/architecture/quill-native-migration.md` |
| 旧架构的来龙去脉（为什么用 wasm、为什么现在不用） | `legacy/docs/architecture/libretro-migration.md` |
| crate 职责与依赖 | `crates/README.md` |
| libretro frontend（dlopen / 回调 / 视频音频输入存档） | `crates/cgb-libretro/src/host.rs` |
| 机种 / CoreSpec 选核、joypad id | `crates/cgb-systems/src/` |
| UI 视图与帧循环 | `crates/cgb-ui/src/`、`crates/cgb-app/src/app.rs` |
| 原生 core 构建 / 加核心流程 | `cores/README.md`、`cores/build.sh.example`、`cores/*/build.sh` |
| 核心清单（启动选核） | `cores/cores.json`、`crates/cgb-library/src/cores.rs`、`crates/cgb-app/src/cli.rs` |
| 旧 Electron/C++/wasm 栈 | `legacy/`（只读） |

## 已知缺口（先记录，不擅自补）

- quill 没有 **Image 组件**：`draw_ui::Widget` 无 image 变体。Q1 先用
  `cgb-ui/src/frame.rs` 的 `FrameImage`（`draw_components::Component` +
  `foreground` 装饰器）把画面画上去；上游补 `Widget::Image` 后应撤掉这个本地组件。
- quill 没有 **TextInput / IME**：改名 / 搜索 / 标签编辑已用自建最小输入层
  （`ViewModel.editing` + `edit_field`，应用层处理字符键），**但只有拉丁字符**。
  中文输入法（IME）需要 quill 上游支持或加一层输入法桥接（macOS `NSTextInputClient`），
  计划中、未做（数据层已就绪：`Library::rename`、`set_tags`）。
- quill 的 **连续帧循环（Phase 7）** 未落地，`cgb-app` 用 `WaitUntil` 自建。
