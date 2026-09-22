---
name: fc-libretro
description: libretro 包装包 packages/fc-libretro（target fc_libretro_core 与 fc_libretro），把 fc-core 的 Machine 翻译成 retro_* ABI。用于 retro_init/load_game/run/serialize/unload、视频与音频回调、输入位掩码、SRAM/RTC、内存视图（peek/poke）、金手指（Game Genie / Pro Action Replay）、custom 扩展 fc_libretro_ext.h，以及 “RetroArch 里跑不起来” 这类问题。
---

# fc-libretro —— 只做翻译，不新增模拟逻辑

上游：`packages/fc-core`（`fc_core`）。
下游：`wasm/`（同一个 `fc_libretro_core` OBJECT 库链出 `.wasm`）、
`tools/fc_libretro_probe`、RetroArch 等 libretro 前端。

**这一层不允许出现任何模拟逻辑。** 发现计算发生在 `fc_libretro.cpp`
而不是 `fc_core`，就是 bug。

## 0. 上下文纪律

**白名单**：

```
packages/fc-libretro/src/libretro/fc_libretro.cpp     主适配层（大，按函数读）
packages/fc-libretro/src/libretro/cheat_codes.{hpp,cpp}
packages/fc-libretro/src/libretro/fc_libretro_ext.h   custom 扩展的函数指针表
packages/fc-libretro/tests/test_libretro.cpp          假前端 + 回调断言
packages/fc-libretro/tests/test_cheat_codes.cpp
packages/fc-libretro/README.md                         ≈60 行，可整读
```

**禁读**：

- `third_party/libretro/libretro.h` —— **8716 行，永远不要整读或分段通读**。
  只 `rg -n 'RETRO_ENVIRONMENT_|retro_(load_game|run|serialize|memory)' <该文件>`
  取需要的符号或常量的定义。
- `build*/`、`build-libretro/`、`wasm/dist/`。

**读 `fc_libretro.cpp` 的正确方式**：`rg -n '^RETRO_API|^static' <file>` 拿到
函数清单，再 `read offset/limit` 读那一个函数。不要从头读到尾。

## 1. 文件地图

```
src/libretro/
  fc_libretro.cpp      retro_* 适配层：生命周期、视频、音频、输入、存档、内存视图
  cheat_codes.{hpp,cpp} Game Genie（6/8 字母）/ Pro Action Replay 解码
  fc_libretro_ext.h    custom 扩展（peek/poke/诊断），标准前端忽略
third_party/libretro/
  libretro.h           vendored 的 ABI 契约（附带来源与 sha256，不要手改）
tests/
  test_libretro.cpp    ABI 测试：假前端，断言回调次数与内容
  test_cheat_codes.cpp 解码向量
```

两个 target：`fc_libretro_core`（OBJECT，供 `.dylib` 与 wasm 共用）、
`fc_libretro`（MODULE → `fc_libretro.dylib`）。

> 产物名是 `fc_libretro.dylib` 而**不是** `libfc_libretro.dylib`：
> libretro 前端的 core 扫描器按前者找。改 `OUTPUT_NAME` 会静默破坏加载。

## 2. 构建与测试

```bash
# 单包（会自动把 ../fc-core 作为子项目拉进来）
cmake -S packages/fc-libretro -B build-libretro -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build build-libretro
ctest --test-dir build-libretro --output-on-failure

# 只跑相关
ctest --test-dir build-libretro -R CheatCodes --output-on-failure

# 手动验证 ABI：用探针加载 .dylib，跑一段游戏
./build/fc_libretro_probe ./build/fc_libretro.dylib          # 从根目录构建时
```

## 3. 任务菜谱

**加一个 custom 扩展函数**（标准 libretro 表达不了的能力走这里）：

1. `fc_libretro_ext.h`：在函数指针表里加成员（结构体末尾加，不要动前面的）。
2. `fc_libretro.cpp`：实现 + 填入表 + 在 `retro_get_...` 里暴露。
3. `wasm/libretro_ext_glue.cpp`：导出给 JS（见 `fc-wasm` 技能）。
4. `tests/test_libretro.cpp`：加断言。

**改标准能力**（视频/音频/输入/存档/加载）：只走标准 `retro_*`，
不要加扩展；扩展层的设计理由见 `docs/architecture/libretro-migration.md`
（`rg -n '^#' 该文件` 先看目录）。

**金手指**：解码在 `cheat_codes.cpp`，应用在 `fc_libretro.cpp`；
应用逻辑必须落在 `fc_core`（现在通过 `core/nes/cheats.hpp`），
不要在适配层里改内存。

## 4. 检索菜谱

```bash
rg -n '^RETRO_API|^static' packages/fc-libretro/src/libretro/fc_libretro.cpp
rg -n 'RETRO_ENVIRONMENT_SET_(PIXEL|INPUT|SUPPORT)' packages/fc-libretro/src/libretro/fc_libretro.cpp
rg -n 'retro_memory_map|RETRO_MEMORY_SAVE_RAM' packages/fc-libretro/src/libretro/fc_libretro.cpp
rg -n 'retro_serialize' packages/fc-libretro/third_party/libretro/libretro.h
rg -n '^\s*TEST\(' packages/fc-libretro/tests/test_libretro.cpp
```

## 5. 陷阱

- **反复 `retro_load_game`**：必须先 `unload` 上一张卡，否则残留状态。
- **帧 pitch ≠ `width * 4`**：回调的 `pitch` 是行字节数，不要自己乘
  （历史上 GBA 帧被画歪就是这个原因）。
- **选核时机**：core 要在**每次 load** 时决定，不是只在懒加载那一次。
- **AV 信息 / 几何**：改 `retro_get_system_av_info` 要同步 `wasm/` 与前端，
  前端的横纵比和像素缩放在 `electron/src/renderer/usePixelScale.ts`。
