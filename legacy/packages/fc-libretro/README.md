# fc-libretro

libretro 包装（core）。这是 monorepo 里的一个**独立 CMake 项目**：它有自己的
`project()`、自己的版本号、自己的测试，也可以单独构建 —— 单独构建时它会把
`../fc-core` 作为子项目拉进来。

它只做翻译，不新增任何模拟逻辑：

```
retro_* 调用  ──>  fc_core 的 Machine 方法
```

这样 RetroArch、Lakka 以及任何 libretro 前端都能加载这个模拟器，而不需要知道
它的存在。

## target

| target | 是什么 | 谁在用 |
|---|---|---|
| `fc_libretro_core` | 适配层源码（OBJECT library） | `.dylib` 与 wasm 模块共用同一批目标文件 |
| `fc_libretro` | 原生共享对象：`fc_libretro.dylib` / `.so` / `.dll` | RetroArch、`tools/fc_libretro_probe` |
| （wasm）| `wasm/CMakeLists.txt` 用 `fc_libretro_core` 链出 `fc_libretro.wasm` | Electron 渲染进程 |

产物名是 `fc_libretro.dylib` 而不是 `libfc_libretro.dylib`：每个前端的 core
扫描器都按前者找。

## 目录

```
src/libretro/
  fc_libretro.cpp      retro_* ABI 适配层（生命周期、视频、音频、输入、存档）
  cheat_codes.{hpp,cpp} Game Genie / Pro Action Replay 解码
  fc_libretro_ext.h    custom 扩展：额外导出的函数指针表（peek/poke/诊断）
third_party/libretro/
  libretro.h           vendored 的 ABI 契约
  README.md            来源、sha256、更新方式
tests/
  test_libretro.cpp    ABI 测试（用假前端，断言回调次数与内容）
  test_cheat_codes.cpp 金手指解码向量
```

## 构建与测试

```bash
# 单独构建这个包（会自动把 ../fc-core 作为子项目加进来）
cmake -S . -B ../../build-libretro -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build ../../build-libretro
ctest --test-dir ../../build-libretro --output-on-failure

# 或者，从 monorepo 根目录一起构建
cmake -S ../.. -B ../../build -G Ninja
cmake --build ../../build
./../../build/fc_libretro_probe ../../build/fc_libretro.dylib
```

## 与标准 libretro 的差别

标准能力走标准 ABI（视频 / 音频 / 输入 / 存档 / 加载）。libretro 表达不了的
能力（原始 RAM 金手指、`peek` / `poke`、PC / cycles 诊断）走一层**可选的**
custom 扩展：额外导出一个 `fc_libretro_get_ext()`，标准前端忽略它，自家前端
`dlsym` 到就用。设计见 `docs/architecture/libretro-migration.md`。
