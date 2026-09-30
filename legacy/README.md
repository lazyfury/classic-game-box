# legacy —— 自研 FC / NES 核心（保留）

这里只保留**自研 FC / NES 核心的 C++ 源码**，用于构建 `cores/custom_nes_core`
（`cores/custom_nes_core/build.sh`，clang++ 直接编译，不经 CMake）。旧的
Electron + TypeScript 前端、wasm 构建、教学 tools 与根 CMake 工程已删除。

```
packages/fc-core/        仿真机本体：CPU / Bus / PPU / APU / Mapper / State
packages/fc-libretro/    libretro 包装（fc_libretro）+ 内置 libretro.h
cmake/                   两个包独立 CMake 构建所需的 Version.cmake / GoogleTest.cmake
```

依赖方向：`fc-libretro → fc-core`（`fc-core` 无依赖）。`fc-core/src/ffi/` 的
私有 `fc_*` C 接口不参与 libretro 构建，只为历史对照保留；`cores/custom_nes_core`
只用标准 libretro ABI，忽略该私有扩展。

> 新前端只做 UI 与 libretro 兼容；这份核心是**对照与兼容性测试基准**，不再演进。
> 迁移调研见 [`../docs/architecture/libretro-migration.md`](../docs/architecture/libretro-migration.md)。
