# N64 硬件加速接入计划（GL 路径 spike）

状态：**已完成并人工验收**。N64 用 **ParaLLEl-N64 + GLideN64**（OpenGL），
在 macOS 上用**离屏 CGL 上下文 + 每帧回读**复用现有的 `Frame`/`FrameImage` 纹理路径。

范围限定：**只做 GL 路径、只做 N64、只做 macOS**。Vulkan / parallel-rdp / PSP / Metal 原生
都不在本计划内。

## 0. 为什么单独开 GL 路径

现有 frontend 是纯软件帧缓冲：核心调 `video_refresh`，host 把像素转 RGBA8，`session` 上传纹理。
N64 核心（Mupen64Plus-Next）走 libretro 的硬件渲染：它要 frontend 提供 GL 上下文和 FBO，自己
渲染，然后只发一个「帧好了」的信号。所以必须新增一条 `SET_HW_RENDER` 通道。

## 1. 调查结论（已核对上游源码）

上游仓库 `libretro/mupen64plus-libretro-nx`（本地调查副本 `/tmp/m64p-nx`，commit `6752836`）：

- **构建形态**：Makefile 核心，`make platform=osx` 可用。GLideN64 / mupen64plus-core /
  parallel-rdp 等**全部 vendored 在树里**（不是 submodule），clone 简单。
  dry-run 确认桌面 GL 走 `-DCORE -DHAVE_OPENGL`（`Makefile.common:514-518`）。
- **默认渲染后端 = GLideN64（GL）**：没给 `rdp-plugin` 选项时回落到 GL（`libretro.c:930`；
  core option 默认值也是 `gliden64`，`libretro_core_options.h:68`）。Vulkan 只在显式选
  `parallel` + 编译了 `HAVE_PARALLEL_RDP` 时才用。
- **它请求的 GL 契约**（`libretro-common/glsm/glsm.c:3302-3325`）：非 Windows 桌面平台请求
  `RETRO_HW_CONTEXT_OPENGL_CORE`，`version_major=3, version_minor=3`，
  `depth=true`、`stencil=false`、`bottom_left_origin=true`、`cache_context=true`；
  context_reset/context_destroy 由核心提供；frontend 必须填
  `get_current_framebuffer` + `get_proc_address`。
- **帧信号**（`libretro.c:2076`）：GLideN64 下调用
  `video_cb(RETRO_HW_FRAME_BUFFER_VALID, w, h, 0)`，即 `data == (void*)-1`。
  当前 `host.rs::on_video` 会把这个指针当像素读 → **必须特判，否则段错误**。
- **相关 core option 默认值**（`libretro_core_options.h`）：`rdp-plugin=gliden64`、
  `ThreadedRenderer=False`（单线程，渲染发生在 frontend 调用 `retro_run` 的线程）、
  `EnableFBEmulation=True`。我们的 frontend 会把 core option 默认值原样喂回
  （`host.rs::read_core_options` 存 `default_value`），所以核心会明确拿到 GL 路径和单线程。
- **osx Makefile 的两个坑**：
  - `HAVE_PARALLEL_RDP = 1` / `HAVE_PARALLEL_RSP = 1` / `LLE = 1` 是**无条件赋值**
    （`Makefile:414-417`）。命令行 `make HAVE_PARALLEL_RDP=0 HAVE_PARALLEL_RSP=0 LLE=0`
    可覆盖（GNU make 命令行优先），从而去掉 Vulkan 依赖、只保留 GLideN64 + HLE RSP。
  - `WITH_DYNAREC =` 为空（`Makefile:412`）→ **macOS 上不编译 arm64 dynarec**，跑解释器。
    这是最大风险，见 §5。

前端现状（本仓库）：

- `crates/cgb-libretro/src/host.rs::environment` 对 `SET_HW_RENDER` 等一律 `_ => false`
  （`host.rs:379` 附近），即诚实地说「没有 GL」。
- 视频唯一出口是 `Frame { width, height, rgba }` + `take_frame()`；`session.rs:187` 用
  `backend.update_texture` 上传，分辨率变化时会重建纹理（`session.rs:187-195`）。
- `ffi.rs` 里没有任何 `retro_hw_render_*` 类型或 `RETRO_HW_FRAME_BUFFER_VALID` 常量。
- crate 不依赖 OpenGL；`cgb-ui` 不认识 libretro（依赖规则不变）。

macOS GL 事实：

- OpenGL 虽已弃用仍可用；离屏 **CGL**（`CGLChoosePixelFormat` / `CGLCreateContext` /
  `CGLSetCurrentContext`）不需要窗口，最高 4.1 core，满足核心要的 3.3 core。
- CGL 上下文与 wgpu/Metal 上下文互不干扰；所有 GL 调用都在主线程（`Session::advance` 所在线程），
  与 `retro_run` 同线程，无需跨线程共享。

## 2. 方案（数据流）

```
SET_HW_RENDER(callback)
    └─ host 建 CGL 离屏上下文 + FBO(color tex + depth rb)，写回
       get_current_framebuffer / get_proc_address，调用 core.context_reset()
retro_run:
    GLideN64 画进 FBO
    video_cb(RETRO_HW_FRAME_BUFFER_VALID, w, h, 0)   → host 置 hw_frame_valid
host.take_frame:
    glBindFramebuffer(fbo); glReadPixels(RGBA8)      → 翻转 y → Frame{rgba}
Session.advance:
    update_texture(...) 同现有路径 → FrameImage → 后处理 → 屏幕
```

## 3. 改动清单

### A. `cores/` — 构建与清单

- **A1** `cores/mupen64plus_next/build.sh`（照 `build.sh.example`）：clone
  `https://github.com/libretro/mupen64plus-libretro-nx` 到 `cores/sources/`，
  `make platform=osx HAVE_PARALLEL_RDP=0 HAVE_PARALLEL_RSP=0 LLE=0 -j`，
  拷 `mupen64plus_next_libretro.dylib` → `cores/dist/`。
  验证 `file`（arm64）+ `nm -gU | grep _retro_`。
- **A2** `cores/README.md` 表格加一行。
- **A3** `cores/cores.json` 加一行：`key=mupen64plus_next`、`system=n64`、
  `dylib=mupen64plus_next_libretro.dylib`、`sample_rate`/`fps` 仅 hint（真实值来自 av_info）。

### B. `cgb-systems` — 机种

- **B1** `SystemId::N64`：更新 `SYSTEMS`、`name`、`short`、`extensions`
  （`["z64","n64","v64"]`；**不要**动 `.bin`，它已被 Genesis 占用）、`key`、`parse_key`、
  `system_for_path`，并补 `system_for_path` / round-trip 测试。
- **B2** 检查 `core_choice.rs` 是否需要为 N64 加默认核 —— 一般 `choose_core` 按 system 过滤即可，
  预计无需改动。

### C. `cgb-library` — 设置

- **C1** `settings.rs`：加 `n64_core: Option<String>`，补 `core_key` / `set_core_key` 分支，
  补 `Settings::default()` 的字段（可能无需默认值）。

### D. `cgb-libretro` — 核心工作

- **D1 `ffi.rs`** 新增（ABI 手写，与 `libretro.h` 对齐）：
  - `RETRO_ENVIRONMENT_SET_HW_RENDER = 14`（已有 `ffi.rs` 常量风格）、
    `RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER = 56`。
  - `RETRO_HW_FRAME_BUFFER_VALID: usize = usize::MAX`（即 `(void*)-1`）。
  - `retro_hw_context_type` 常量：`OPENGL=1`、`OPENGLES2=2`、`OPENGL_CORE=3`、
    `OPENGLES3=4`、`OPENGLES_VERSION=5`、`VULKAN=6`、D3D…
  - 回调类型：`RetroHwContextResetFn = unsafe extern "C" fn()`、
    `RetroHwGetCurrentFramebufferFn = unsafe extern "C" fn() -> c_uint`、
    `RetroHwGetProcAddressFn = unsafe extern "C" fn(*const c_char) -> *mut c_void`。
  - `#[repr(C)] struct retro_hw_render_callback { context_type, context_reset,
    get_current_framebuffer, get_proc_address, depth: bool, stencil: bool,
    bottom_left_origin: bool, version_major: c_uint, version_minor: c_uint,
    cache_context: bool, context_destroy, debug_context: bool }`。
    field 顺序/类型必须和 `libretro.h:5813` 完全一致。
- **D2 新模块 `crates/cgb-libretro/src/gl.rs`**（`#[cfg(target_os = "macos")]`）：
  - 通过 `build.rs` `cargo:rustc-link-lib=framework=OpenGL` 链接 OpenGL.framework，
    声明 CGL 的 `extern "C"`。
  - `get_proc_address(name) -> *mut c_void`：`dlsym(RTLD_DEFAULT, name)`
    （framework 已加载；必要时再 dlopen）。
  - `struct GlContext { cgl_context, pixel_format, fbo, color_tex, depth_rb, w, h }`：
    - `new(width, height, depth, stencil)`：选 pixel format（`kCGLOGLPVersion_GL4_Core`
      或 3.2 core），建 context，make current，建 FBO + 颜色纹理（RGBA8）+ 可选 depth/stencil
      renderbuffer，`glViewport`。
    - `make_current()`、`framebuffer() -> u32`、`resize(w, h)`、
      `read_pixels() -> Vec<u8>`（bind FBO → `glReadPixels(0,0,w,h,GL_RGBA,GL_UNSIGNED_BYTE)`）；
      `bottom_left_origin=true` 时读回是自底向上，**在 host 侧或这里翻转 y**。
    - `Drop` 销毁 FBO/纹理/context/pixelformat。
  - 非 macOS：该模块给一个「不支持 GL hw render」的桩，`SET_HW_RENDER` 返回 false。
- **D3 `host.rs`**：
  - `HostShared` 增状态：`hw_render: Mutex<Option<HwRenderState>>`（存 core 的
    `context_reset`/`context_destroy`、context_type、是否激活），
    `gl: Mutex<Option<GlContext>>`，`hw_frame_valid: AtomicBool`。
  - `environment`：
    - `SET_HW_RENDER`：读入 `retro_hw_render_callback`；**只接受**
      `OPENGL` / `OPENGL_CORE`（macOS 无 GLES）→ 建 `GlContext`、填回
      `get_current_framebuffer` / `get_proc_address`、保存 core 回调、调 `context_reset()`，
      返回 `true`；其余 context_type（VULKAN/D3D/GLES）返回 `false`。
    - `GET_PREFERRED_HW_RENDER`：返回 `OPENGL_CORE`（帮助核心选择；可选）。
    - 其余保持 `false`（`GET_HW_RENDER_INTERFACE`、`SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE`
      都是 Vulkan 用）。
  - `on_video`：开头特判 `data as usize == usize::MAX`
    （`RETRO_HW_FRAME_BUFFER_VALID`）→ 记 `(width, height)`、置 `hw_frame_valid=true`、return；
    否则走现有软件转换路径。
  - `take_frame`（或 `run_frame` 之后）：若 `hw_frame_valid.swap(false)`，取
    `gl.read_pixels()` 构造 `Frame`。
  - `unload_game` / `Drop`：先调 core 的 `context_destroy()`，再销毁 `GlContext`。
  - 回调 `get_current_framebuffer_cb` → `gl.framebuffer()`；`get_proc_address_cb` → `gl.rs` 的转发。
- **D4** `tests/`：新增纯 Rust 单测（不建真 GL），验证
  `SET_HW_RENDER` 的 `retro_hw_render_callback` 布局、`on_video` 对 `-1` 的特判不会崩、
  不支持的 context_type 返回 false。符合「不写截图测试」。

> **归属决定**：`GlContext` 放在 `cgb-libretro` 内部，因为 FBO 与 `get_proc_address` 是 libretro
> 契约的一部分，`get_current_framebuffer` 回调也需要它；`cgb-libretro` 因此会（在 macOS 上）
> 链接 OpenGL framework，但仍是 UI-free、音频-free。

### E. `cgb-app` — 接线

- **E1** 基本不用改：`Session::advance` 已消费 `Frame` 并 `update_texture`；N64 内部分辨率可变，
  现有「分辨率变则重建纹理」逻辑可覆盖（注意有大小上限，检查 `update_texture` 的假设）。
- **E2** 后处理 shader 照旧（hw 帧也是 RGBA8）。
- **E3** 存档/即时存档走通用 `RETRO_MEMORY_SAVE_RAM` 与 `serialize`，无需特殊；但 N64 state 很大，
  **倒带**（`REWIND_STRIDE=2`）可能内存暴涨 —— 建议对 N64 关闭或加大 stride（**待确认**）。
- **E4** `--selfcheck` 的 cores 检查补齐 N64。

### F. 验证 gate

- **F1** `./scripts/dev.sh`（fmt + clippy -D warnings + test）全绿。
- **F2** 真机：`cargo run -p cgb-app -- --rom game.z64 --core mupen64plus_next`，人眼看画面/手柄/存档。
- **F3** 性能 gate：测能否满帧（见 §5 R1）。
- **F4** `--selfcheck`。

## 4. 里程碑（建议顺序）

- **M0（~半天）**：`cores/mupen64plus_next/build.sh` 建出 dylib；`file`/`nm` 验证；用
  `--core <path>` 试加载，确认前端目前因 `SET_HW_RENDER=false` 让核心 `retro_load_game` 失败
  （预期日志 `libretro frontend doesn't have OpenGL support`）。这是「核心能跑」的基线。
- **M1（1–2 天）**：`ffi.rs` 类型 + `host.rs` env/sentinel + `gl.rs` CGL/FBO/readback；
  一帧出画（先不管机种清单，`--core` 直连）。
- **M2**：`SystemId::N64` + `cores.json` + `settings` 接入；库里的 `.z64` 能被识别/播放。
- **M3**：存档 / 即时存档 / 手柄 / 音量验证；`--selfcheck`。
- **M4**：性能评估（dynarec 与否）、倒带策略、文档与打包。

## 5. 风险与开放问题

- **R1 性能（最大风险）**：osx Makefile `WITH_DYNAREC=` 为空 → arm64 跑解释器，N64 很可能不满帧。
  选项：
  - (a) 试 `make WITH_DYNAREC=aarch64`：上游有 arm64 new_dynarec
    （`mupen64plus-core/src/device/r4300/new_dynarec/arm64/`），但为 Linux/Android 写，
    macOS 上需要 `MAP_JIT` 且未签名/非沙箱的进程才允许 —— dev 直接 `cargo run` 也许可行，
    打包成签名 `.app` 可能要 JIT entitlement。**需实测**。
  - (b) 转 Vulkan/parallel-rdp（超出本 spike）。
  - (c) 接受不满帧，先要正确性。
- **R2 GL 版本**：核心请求 3.3 core，macOS 实际给 4.1 core；GLideN64 `-DCORE` 编译。
  需实测 shader/扩展是否都对。
- **R3 线程**：`ThreadedRenderer` 默认 False，单线程，正好。若将来开 True，才会碰到
  `GET_CLEAR_ALL_THREAD_WAITS_CB` / 共享上下文等（本期不碰）。
- **R4 context_reset 时机**：必须在我们 make current 之后回调；全屏切换我们不做多上下文，
  `cache_context=true` 影响不大。
- **R5 颜色/朝向**：`bottom_left_origin=true` 的 y 翻转；sRGB 与现有纹理路径一致性。
- **R6 `pitfall`/`on_video` 特判**：`-1` 判定必须用 `usize::MAX` 精确比较，不能用 `data.is_null()`。
- **R7 打包**：OpenGL framework 链接与 `.app` 签名；`cores.json` 里新核心的 dylib 入包。

## 6. 明确不做

Vulkan / parallel-rdp / ParaLLEl-N64；PSP；Metal 原生；把 UI 搬去 C++ / igui FFI。

## 7. 进度（已实现）

- **M0 构建**：`cores/mupen64plus_next/build.sh`（`platform=osx`，关掉
  parallel-rdp/RSP 以去掉 Vulkan；现代 clang 预定义 `TARGET_OS_MAC` 导致
  vendored libpng/zlib 走 classic-Mac 分支，脚本内幂等打补丁跳过 `<fp.h>` /
  `fdopen` 宏）。产物 arm64，25 个 `retro_*` 导出。源码 `cores/sources/` 被
  gitignore，用 `MUPEN64PLUS_NEXT_SRC` 可指到本地检出。
- **M1 前端 GL 通路**：
  - `crates/cgb-libretro/build.rs`：macOS 链接 OpenGL framework。
  - `crates/cgb-libretro/src/gl.rs`：CGL 离屏 4.1 core + FBO（颜色 RGBA8、
    可选 depth/stencil）+ `glReadPixels` 回读（y 翻转、alpha 置 255）；
    `proc_address` 用 `dlsym(RTLD_DEFAULT)`。默认 640×480，帧尺寸变了重建 FBO。
    非 macOS 走空桩。
  - `crates/cgb-libretro/src/ffi.rs`：`retro_hw_render_callback`、
    `RETRO_HW_CONTEXT_*`、`RETRO_HW_FRAME_BUFFER_VALID`、env 常量。
  - `crates/cgb-libretro/src/host.rs`：`SET_HW_RENDER` 建上下文并回调
    `context_reset`；`GET_PREFERRED_HW_RENDER` 回 `OPENGL_CORE`；`on_video` 特判
    `-1`；`take_frame` 回读；`Drop` 先 `context_destroy` 再销毁上下文。
  - 单测：`retro_hw_render_callback` 拒绝 Vulkan、`GET_PREFERRED_HW_RENDER`、
    `-1` 哨兵不崩、`flip_rows`/alpha、以及一个真实 CGL 上下文回读空帧的冒烟测试。
- **M2 机种/清单**：`SystemId::N64`（`.z64/.n64/.v64`）、`settings.n64_core`、
  `cores.json` 一行、`cgb-input` 的 N64 默认键盘布局（B/Z/R/C 按钮）。
  `session.rs` 对 N64 关闭倒带（state 太大）。
- **验证**：`./scripts/dev.sh` 全绿；`--selfcheck` 报 15 个核心；
  `cores_run_through_the_host` 验证 N64 核能打开并声明扩展名。
- **待做（需真人）**：用真实 ROM 跑 GL 通路。

  ```bash
  CGB_MANUAL_ROM=game.z64 \
      cargo test -p cgb-libretro --test _manual_n64 -- --nocapture --ignored
  # 或直接：
  cargo run -p cgb-app -- --rom game.z64 --core mupen64plus_next
  ```

### 7.1 真机结果（Super Mario 64 USA，2026-09）

前端 GL 通路**完全跑通**：

- `SET_HW_RENDER` 被接受，离屏 CGL 4.1 core + FBO 创建成功；
- 每帧 `glReadPixels` 回读 640×480 RGBA8，`take_frame` 正常；
- 崩溃已修：`context_reset` **必须**在 `retro_load_game` 返回后调用（Mupen 只在
  load 末尾把 `first_context_reset` 置真，提前调会让 `emu_step_initialize` 永不执行、
  `gfx.romOpen` 为空指针）。
- 离屏 CGL 无 drawable，真实 FBO 0 无效，且 `CGLSetOffScreen` 在 macOS 26 已不可用；
  因此在 `get_proc_address` 里对 `glBindFramebuffer` 返回一个 shim，把 0 重定向到
  前端 FBO（GLideN64 经 GLSM 的 `rglBindFramebuffer` 也会走到它）。

**遗留问题（卡在核侧，不是前端）**：游戏在 frame ~146（≈2.4s）开始出一帧极小内容
后图形不再更新：

- CPU 在跑（RDRAM 持续增长到 ~2.3MB、入口 `0x80246000` 有 MIPS 代码）；
- 音频连续（~44000Hz）；
- VI 未 blank（`VI_STATUS=0x00013016`, `vitype=2`），`VI_ORIGIN` 稳定在 `0x3b5280`，
  GLideN64 的 `findBuffer(vi_origin)` 能找到缓冲；但该缓冲内容全黑。
- 已排除：GL 前端（换 Angrylion **软件** RDP 同样黑）、RSP 插件（HLE 与 cxd4 同样）、
  `EnableFBEmulation`（True 全黑，False 只有中间一个点）。
- 结论：RSP 产出的 RDP 显示列表几乎是空的 / 图形任务没真正跑完，属 Mupen64Plus-Next
  在这台 macOS 26 / Apple M4 上的模拟问题，需要在核侧继续查（RSP 任务完成中断、
  GLideN64 在 Metal GL 驱动上的纹理警告 “using zero texture”）。

复现：

```bash
CGB_MANUAL_ROM="Super Mario 64 (USA).z64" CGB_MANUAL_PPM=/tmp/f.ppm \
    cargo test -p cgb-libretro --test _manual_n64 -- --nocapture --ignored
# 可选：CGB_MANUAL_GFX=gliden64|angrylion  CGB_MANUAL_RSP=auto|hle|cxd4
```

### 7.2 换核：Mupen64Plus-Next → ParaLLEl-N64（已解决，已验收）

§7.1 的黑屏经真机对照定位：**同一核 Mupen64Plus-Next 在 RetroArch 里用默认
GLideN64 也是黑屏，切到 ParaLLEl-RDP（Vulkan）才显示**。即 GLideN64 的 GL 路径
在这台 macOS 26 / Apple M4 上是坏的（Mupen64Plus-Next 的软件 Angrylion 也黑）。

改用 **ParaLLEl-N64**（`libretro/parallel-n64`）：

- 它的 GLideN64 在同一个前端 GL 通路上**渲染正常**（Super Mario 64 标题画面完整）；
- 自带 **aarch64 dynarec**（`new_dynarec/arm64/apple_jit_protect.c`），不需要
  Mupen 那样的解释器，速度也够；
- 构建：`cores/parallel_n64/build.sh`，`platform=osx HAVE_PARALLEL=0 HAVE_PARALLEL_RSP=0`
  （关掉 Vulkan 后端，只用 GLideN64），产物 `cores/dist/parallel_n64_libretro.dylib`；
- 清单：`cores.json` 的 `n64` 行改为 `key=parallel_n64`。

`cores/mupen64plus_next/` 已删除。前端 GL 代码（`gl.rs` / `host.rs` / `ffi.rs`）
无需任何改动——它本来就是通用 libretro GL 前端。
