# fc-core

自定义 FC / NES 核心。这是 monorepo 里的一个**独立 CMake 项目**：它有自己的
`project()`、自己的版本号、自己的测试，也可以完全脱离仓库根目录单独构建。

它模拟的是真实硬件：

```
CPU (6502) ── Bus ──> PPU ──> Framebuffer (256x240)
                ├───> APU ──> 采样
                ├───> Cartridge ──> Mapper ──> CHR/PRG ROM
                └───> Controller
```

## 两个 target

| target | 是什么 | 谁在用 |
|---|---|---|
| `fc_core` | 机器本身：CPU / Bus / PPU / APU / Mapper / State | `fc_ffi`、`fc-libretro`、`tools/`、wasm |
| `fc_ffi` | 机器外面那层纯 C 接口（`src/ffi/emulator_api.h`） | `wasm/`、`tools/fc_headless` |

依赖方向只有一条：`fc_ffi -> fc_core`。

## 目录

```
src/core/      纯 C++ 机器
  types.hpp      定宽整数
  bit.{hpp,cpp}  位运算工具
  alu.hpp        加法器与标志位
  bus.hpp        总线抽象
  flat_bus.hpp   测试替身：64KB 平铺内存
  cpu/           寄存器、opcode 表、反汇编、寻址、表驱动派发
  nes/           地址译码、卡带、PPU、APU、手柄、Machine、State
src/ffi/       纯 C 接口（前端唯一需要链接的东西）
tests/         单元测试 + 真实 ROM 测试的 fixtures
```

## 约束

- **禁止依赖 UI**：不能出现 `Metal` / `NSWindow` / Canvas。
- **禁止 CPU 直接访问 PPU**：一切经过 Bus。
- 没有文件 IO、没有线程、没有异常 —— 所以同一份源码能原样编译成原生库和
  WebAssembly，不需要一个 `#ifdef`。

## 构建与测试

```bash
# 单独构建这个包
cmake -S . -B ../../build-core -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build ../../build-core
ctest --test-dir ../../build-core --output-on-failure

# 或者，从 monorepo 根目录一起构建
cmake -S ../.. -B ../../build -G Ninja
cmake --build ../../build
```

真实 ROM 测试默认查找 `tests/data/*.nes`；文件夹为空时那些测试会自己跳过
（仓库里不提交任何 ROM）。也可以 `FC_TEST_ROM=/path/to/game.nes ctest ...`。
