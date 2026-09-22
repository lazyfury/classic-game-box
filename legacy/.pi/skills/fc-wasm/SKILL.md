---
name: fc-wasm
description: WebAssembly 构建目录 wasm/：把 fc-core（经 fc_ffi）和 fc-libretro 编成 .wasm，外加 JS 绑定、headless 回归测试、以及 mGBA / Mesen 侧模块。用于 wasm/build.sh、emcmake、EMSCRIPTEN_KEEPALIVE 导出、glue.cpp、libretro.mjs/emulator.mjs 绑定、wasm/dist 产物、verify.sh，或 “Electron 里加载 wasm 失败/画面冻结” 这类问题。
---

# fc-wasm —— 同一份核心，编成 WebAssembly

上游：`packages/fc-core`（`fc_ffi`）、`packages/fc-libretro`（`fc_libretro_core`）。
下游：`electron/`（渲染进程）、`node wasm/headless.mjs`（回归测试）。

设计要点：**被回归测试的东西就是被发布的东西**。同一批目标文件既链成原生
`.dylib`，也链成 `.wasm`。

## 0. 上下文纪律

**白名单**：

```
wasm/CMakeLists.txt       构建定义（含所有 Emscripten 链接选项及其理由）
wasm/build.sh             入口脚本
wasm/glue.cpp             fc_ffi 的 C++→JS 导出层
wasm/emulator.mjs         加载 fc_core.wasm 的 JS 绑定
wasm/libretro.mjs         加载 fc_libretro.wasm 的 JS 绑定
wasm/libretro_ext_glue.cpp
wasm/headless.mjs         无头回归
wasm/libretro_test.mjs / mesen_test.mjs / mgba_test.mjs
wasm/verify.sh
wasm/mesen/build.sh  wasm/mgba/build.sh    第三方核的侧模块构建
```

**禁读**：`wasm/dist/`（生成物，`*.mjs` 是产物不是源码，不要编辑也不要读）、
`third_party/emsdk`、`wasm/mgba/**` / `wasm/mesen/**` 里的上游源码。

判断产物是否过期就跑 `./wasm/build.sh`，**不要**去读 `wasm/dist/fc_core.mjs`。

## 1. 构建

```bash
source third_party/emsdk/emsdk_env.sh
./wasm/build.sh                     # 推荐入口

# 等价的手工方式
emcmake cmake -S . -B build-wasm -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build-wasm
```

产物：`wasm/dist/fc_core.{mjs,wasm}`、`fc_libretro.{mjs,wasm}`、
`mesen_libretro.*`、`mgba_libretro.*`。后两个不属于 CMake 构建：它们是
`wasm/mesen/build.sh` / `wasm/mgba/build.sh` 按需克隆上游、用 `emmake make` 编的
（首次要网络，几分钟）。全量入口 `./scripts/build-all.sh` 已经把这两个串进去了，
`--fast` 则跳过它们。

`wasm/CMakeLists.txt` **只能经 emcmake 进入**（根 `CMakeLists.txt` 里有条件块），
不要当普通 CMake 直接 `cmake -S wasm`。

## 2. 三个不该动的链接选项（改之前先读文件里的注释）

| 选项 | 为什么 |
|---|---|
| `FILESYSTEM=0` | ROM 以内存指针传入，没有东西要挂载 |
| `ALLOW_MEMORY_GROWTH=0` | heap 增长会让已缓存的 typed array view 全部失效（画面冻结/撕裂的经典 bug）。固定 64MB 覆盖最大 2MB 卡带 |
| `--no-entry` | 模块是一袋函数，没有 `main()` |

`HEAPU8` / `HEAPF32` / `UTF8ToString` 是显式点名导出的（默认不再导出）。
`emulator_api.h` 返回裸指针，只有能转回 typed array 才有用 —— 这就是原因。

## 3. 测试

```bash
node wasm/headless.mjs          # fc_ffi 路径：跑帧、比对哈希
node wasm/libretro_test.mjs     # libretro ABI 路径
node wasm/mesen_test.mjs        # Mesen 侧模块
node wasm/mgba_test.mjs         # mGBA 侧模块
./wasm/verify.sh                # 与 Electron 前端逐像素/逐采样对比
```

## 4. 任务菜谱

**新增一个导出给 JS 的 C 函数**：

1. `packages/fc-core/src/ffi/emulator_api.h` 声明（纯 C，`extern "C"`）。
2. 实现 + `EMSCRIPTEN_KEEPALIVE`（或在 `wasm/glue.cpp` 里包一层）。
3. `wasm/emulator.mjs` 暴露成 JS 方法。
4. `electron/src/renderer/wasm.d.ts` 补类型（前端的 TS 需要它）。
5. `wasm/headless.mjs` 加断言。

**改 libretro 的 custom 扩展**：见 `fc-libretro` 技能；wasm 侧只需
`wasm/libretro_ext_glue.cpp` + `wasm/libretro.mjs` 同步。

## 5. 检索菜谱

```bash
rg -n '\-\-no-entry|ALLOW_MEMORY_GROWTH|EXPORTED_FUNCTIONS|FILESYSTEM' wasm/CMakeLists.txt
rg -n 'EMSCRIPTEN_KEEPALIVE' wasm/glue.cpp packages/fc-core/src/ffi/emulator_api.cpp
rg -n 'exports\.|export function' wasm/emulator.mjs wasm/libretro.mjs
rg -n 'fc_libretro|mesen|mgba' wasm/build.sh
```
