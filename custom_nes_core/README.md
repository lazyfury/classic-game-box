# custom_nes_core —— 自研 FC / NES 核心

单个现代 CMake 项目（`src/` 布局）：机器本体与 libretro 包装同源，产品侧的
正式构建入口是 [`../cores/custom_nes_core/build.sh`](../cores/custom_nes_core/build.sh)，
产出的 dylib 对着 `cores/cores.json` 的 `custom_nes_core` 条目。

```
CMakeLists.txt        项目根：产出 custom_nes_core(静态库) 与 custom_nes_core_libretro(模块)
src/core/             机器本体：CPU / Bus / PPU / APU / Mapper / State
src/ffi/              历史对照的私有 fc_* C 接口（不参与 libretro 构建）
src/libretro/         libretro 包装（retro_* → Machine 方法）+ 导出的 fc_libretro_ext.h
third_party/libretro/ 内置的 libretro.h（ABI 参考）
cmake/                Version.cmake / GoogleTest.cmake
tests/core/           机器与 ffi 的单元测试
tests/libretro/       libretro ABI 的单元测试
```

依赖方向只有一条：`src/libretro → src/core`。`src/ffi/` 的私有 `fc_*` 接口
只作历史对照；`cores/custom_nes_core` 只用标准 libretro ABI，宿主忽略该扩展。

配置与构建：

```bash
cmake -S custom_nes_core -B custom_nes_core/build
cmake --build custom_nes_core/build
ctest --test-dir custom_nes_core/build
```

> 前端只做 UI 与 libretro 兼容；这份核心是**对照与兼容性测试基准**，不再演进。
> 迁移调研见 [`../docs/architecture/libretro-migration.md`](../docs/architecture/libretro-migration.md)。
