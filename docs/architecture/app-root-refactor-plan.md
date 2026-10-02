# App 为根项目 + 按功能拆分 —— 重构计划（提案 v2）

> 状态：**已完成**（阶段 A + B1）。参考 `../archiver` 的目录约定。
>
> ## 0. 执行结果
>
> - **阶段 A（结构）**：根包 = `cgb-app`（`src/` 在根），`cgb-ui` 收为 `src/ui` 模块
>   （bench → 根 `benches/ui.rs`，图标 → `assets/icons`），`crates/` 保留
>   `cgb-systems / cgb-libretro / cgb-audio / cgb-input / cgb-library`。
> - **阶段 B1（代码）**：`app.rs`（3540 行）拆为 `src/app/`：`mod.rs`(App/AppLogic/run，
>   ~680) + `input/window/library/cores/screenshots/settings/textures/project/helpers/
>   saves/cheats/tests`。全部方法按功能归位，`pub(super)` 跨子模块可见。
> - 验证：`./scripts/dev.sh` 全绿（fmt + clippy `-D warnings` + test），`--selfcheck` ok。
> - **阶段 B2（代码）**：`cgb-library` 按功能拆分：新增 `cgb-paths`（paths + settings）
>   与 `cgb-cores`（cores 清单 + catalog + download）；`cgb-library` 保留
>   error/png/saves/cheats/library/import（共享 `LibraryError` 带 rusqlite，故不拆开）。
> - **阶段 B3（代码）**：`cgb-input` 定义自己的 `Key`（只含可绑定键），app 在
>   `to_input_key` 里把 `igui_core::Key` 映射进来；去掉 `cgb-input → igui_core` 依赖。
> - 未做：`cgb-library` 不并入 app（保留 crate，`包名不变`；也避免 `cgb-libretro`
>   测试对根 app 的 dev-依赖环）。
> - 提交：`5a4bd0e` 结构、`c854dd7` app 拆分、下一个 `cgb-library` 拆分。
>
> 用户方向：**app 为根项目；`src/` 为 app；UI 收为 `src/ui`；其余 crate 暂时保留；
> 先做「文件夹结构」并测试通过，再做「代码重构」；包名不变。**
> 应用侧的库功能落在 `src/app/library.rs`（不是 `src/library/`）；`cgb-library` 仍为 crate。
>
> 两个阶段严格分离：
> - **阶段 A（结构迁移）**：只搬文件 / 改路径 / 改 Cargo 声明，**函数体一行不动**，
>   每步 `./scripts/dev.sh` 全绿。
> - **阶段 B（代码重构）**：拆 `app.rs` 上帝对象、拆 `cgb-library`。**必须在阶段 A
>   全部通过后**才开始。

---

## 1. 参考：`../archiver` 的布局

```
Cargo.toml          [workspace] members = ["crates/archiver-core"]
                    [package] archiver + [[bin]]（根包 = 应用）
src/main.rs         瘦 CLI，调 lib
src/lib.rs          pub mod 面；re-export engine
src/app.rs          纯应用状态（无 UI、无磁盘）
src/ui/             视图，一组件一模块（mod/nav/toolbar/members/…）
src/host/           窗口宿主：mod / logic( AppLogic ) / runner / services / helpers
src/icons.rs theme.rs prefs.rs i18n.rs …   其余为顶层模块
crates/archiver-core/   engine（独立 crate，UI-free），app 依赖并 re-export
benches/ui.rs       bench 在根
```

关键点：**根包即应用**；`src/ui` 是**模块**不是 crate；`src/app.rs` 是纯状态；
`src/host/` 放 `AppLogic` 与平台装配；引擎是 `crates/*` 子包。

---

## 2. 目标布局（classic-game-box → archiver 风格）

包名不变：**`cgb-app`**（package）、**`classic-game-box`**（bin）。

```
Cargo.toml                     [workspace] + [package cgb-app] + [[bin]] + [[bench]]
src/
  main.rs                      瘦 CLI（现 cgb-app/src/main.rs）
  lib.rs                       新增：pub mod 面（app / host / ui / library / session …）
  app.rs                       应用状态（现 app.rs 整体搬入，结构阶段不改内容）
  host/                        窗口宿主（阶段 B 拆出；结构阶段可先并入 app.rs）
    mod.rs  logic.rs  runner.rs  services.rs  helpers.rs
  ui/                          ← 原 cgb-ui（模块化；mod/model/theme/icons/frame/view/*）
  library/                     ← 应用侧的库功能模块（阶段 B 从 app.rs 拆出）
  session.rs                   一局游戏
  cli.rs  cores_cli.rs  selfcheck.rs
  icons.rs  frame.rs  theme.rs …（按需，参照 archiver 顶层小模块）
benches/ui.rs                  ← 原 cgb-ui/benches/ui.rs
crates/
  cgb-systems/  cgb-libretro/  cgb-audio/  cgb-input/     ← 保持不变（“其他暂时保留”）
  cgb-library/                  ← 暂保留为 crate（见 §6 待确认）
```

依赖方向不变：`cgb-app(root) → { ui(内部) , cgb-libretro, cgb-audio, cgb-input, cgb-library, cgb-systems }`；
`src/ui` 只吃 `ViewModel`，不认识 libretro。

---

## 3. 现状审查（为什么这么切）

| 问题 | 证据 | 对应动作 |
|---|---|---|
| `cgb-app/src/app.rs` 上帝对象 | 3539 行 / 102 方法 / ~50 字段，混了宿主、库、截图、存档、金手指、设置、核心下载、输入、纹理、全屏、投影、打点 | 阶段 B：拆成 `app.rs`(状态) + `host/`(AppLogic) + 各功能模块 |
| `cgb-ui` 只被 app 使用 | 除自身 bench 外无其它使用者 | 阶段 A：收为 `src/ui` 模块 |
| `cgb-library` 职责过多 | 库 DB / 导入 / 存档 / 金手指 / 设置 / 路径 / 核心清单 / 下载 / PNG（3104 行） | 阶段 B（可选）：拆 crate 或收为 `src/library` |
| `cgb-input → igui_core::Key` | 设备层反向依赖 UI-core | 记录，阶段 B 可选处理 |
| 文档/脚本漂移 | skill 仍写 quill/`../quill`/`view.rs`；脚本引用 `crates/cgb-app` | 阶段 A5 收尾 |
| 分支漂移 | `AGENTS.md` 写 `quill-native`，实际 `main` | 记录，与本重构无关 |

依赖方向本身是干净的（`cgb-systems` 无依赖；`cgb-ui` 不认识 libretro；`cgb-libretro` 不认识 UI/音频）。

---

## 4. 阶段 A —— 文件夹结构迁移（先做，只搬不改）

> 原则：`git mv` 保历史；只改 `mod`/`use` 路径与 Cargo 声明；不改任何函数体。

### A0. 基线
- 记录 `./scripts/dev.sh`、`cargo run -p cgb-app -- --selfcheck` 结果。
- 快照当前 bench 基线（若启用）。

### A1. app 增加 lib target（仍在 crates/）
- `cgb-app` 加 `src/lib.rs`：`pub mod app; pub mod cli; pub mod cores_cli; pub mod selfcheck; pub mod session;`
- `main.rs` 改为 `use cgb_app::{cli, cores_cli, selfcheck, app}`。
- Gate 绿。→ 让后续 bench / 集成测试能引用 app。

### A2. app 成为根包（archiver 同构）
- `git mv crates/cgb-app/src src`；把 `crates/cgb-app/Cargo.toml` 的 `[package]`/`[[bin]]`/
  `[dependencies]` 合并进根 `Cargo.toml`，并加 `[workspace] members = ["crates/*"]`。
- `[workspace.dependencies]` 里 `cgb-app` 的 path 由 `crates/cgb-app` 改为 `.`（其余 crate 不变）。
- 更新 `macos/scripts/package.sh` 中 `crates/cgb-app` 注释/路径
  （`-p cgb-app`、bin 名不变，命令多数无需改）。
- Gate 绿 + `cargo run`（无 `-p`）可编译。

### A3. 收编 UI → `src/ui`
- `git mv crates/cgb-ui/src/* src/ui/`；`git mv crates/cgb-ui/benches/ui.rs benches/ui.rs`。
- 根 `Cargo.toml`：加 `igui_svg` / `igui_scene` / `igui_core` 依赖；`igui` 的 `ui` feature；
  `[dev-dependencies] igui`(bench) + `[[bench]] name = "ui"`。
- 机械改写：`src/ui/**` 内 `crate::` → `crate::ui::`；app 侧 `cgb_ui::` → `crate::ui::`。
- 从 workspace 成员与 `[workspace.dependencies]` 删除 `cgb-ui`；删 `crates/cgb-ui/`。
- Gate 绿 + `cargo bench --bench ui` 可跑。

### A4.（若确认）收编 library → `src/library`
- 同 A3。**这一步取决于 §6 的问题 1。** 若不收编，则结构阶段到此为止，
  `src/library/` 只作为阶段 B 从 app.rs 拆出的**应用侧库功能模块**。

### A5. 收尾（结构阶段）
- 更新 `README.md`、`crates/README.md`（或改为 `src/` 说明）、
  `docs/architecture/quill-native-migration.md` §3–§4、`.pi/skills/cgb-rust/SKILL.md`
  （quill→igui、`view.rs`→`ui/view/`、白名单路径）。
- Gate 绿 + `--selfcheck`。

---

## 5. 阶段 B —— 代码重构（结构 A 全绿后再做）

> 原则：纯搬迁优先，行为不变；每步编译 + 测试；`app.rs` 逐块外移，不重写逻辑。

### B1. 拆 `src/app.rs`（3539 行）
按 archiver 的 `app.rs`(纯状态) + `host/`(AppLogic/装配) + 功能模块 三层：

```
src/app.rs        纯应用状态（字段按功能分组为内聚小结构体）
src/host/
  mod.rs          插件装配（现 app.rs::run + HostPlugin + safe_area）
  logic.rs        impl AppLogic（init/event/update/layout/paint/caret/needs_frame/drop）
  runner.rs       本地 EventLoop 运行器（若沿用 igui_app 可省）
src/library/      库：扫描/导入/排序/置顶/删除/改名（refresh_library、add_game_paths…）
src/screenshots.rs 截图页（capture/筛选/多选删除/set_cover…）
src/saves.rs      存档槽（save_to_slot/load/delete/refresh_saves）
src/cheats.rs     金手指（populate/import/toggle）
src/settings.rs   设置 / shader / msaa / theme / core options
src/cores.rs      核心清单 / resolve_core / 下载与 catalog
src/input.rs      热键 / feed / 手柄 / bindings
src/textures.rs   封面 / 截图 / 存档缩略图 / 图标纹理注册
src/window.rs     全屏 / resize / safe-area / 渲染（layout_ui/paint_ui/profile_frame）
```

目标：`host/logic.rs` 只做路由与装配，单文件 < ~600 行；`app.rs` 只存状态。

### B2.（可选）拆 `cgb-library`
若 A4 未收编，则按功能把 crate 拆成 `cgb-paths / cgb-settings / cgb-cores / cgb-saves / cgb-library`；
若 A4 已收编，则在 `src/library/` 内按同样边界分模块，底层纯逻辑仍可下沉为 crate。

### B3.（可选）`cgb-input → igui_core::Key` 去除 UI-core 依赖。

---

## 6. 待确认问题（动手前必须定）

1. **`cgb-library` 是否收为 `src/library/`？**
   - 收：与 `src/ui` 一致，彻底 app 化；`cgb-library` 包名消失（与“包名不变”是否冲突？）。
   - 不收（推荐）：保留 `crates/cgb-library`，`src/library/` 仅作为 B1 拆出的**应用侧库功能模块**。
2. **`src/host/` 是否现在就建？** 沿用 `igui_app` 插件运行时可先不写 `runner.rs`，
   仅 `host/{mod,logic}.rs`。
3. **是否把 `cgb-systems` 也收进 app？** 它是纯领域、被多个 crate 复用，建议**保留为 crate**。
4. **bench 是否随 UI 迁到根 `benches/ui.rs`？** （archiver 如此，推荐是。）
5. `AGENTS.md` 的分支名（quill-native vs main）是否顺带修正？**默认不动**。

> 确认后从阶段 A0 开始执行；未确认不前移文件。
