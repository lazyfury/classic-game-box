---
name: fc-tools
description: 教学 demo 与命令行工具目录 tools/：11 个按学习顺序排列的 demo（bitwise/overflow/cpu/disasm/addressing/instructions/bus/cartridge/ppu/input/apu）、fc_headless 无头跑帧工具、fc_libretro_probe libretro ABI 探针、fc_testrom 测试 ROM 工具、run_testroms.sh 与 compare_nestest.py。用于跑 demo、加新 demo、用无头工具复现画面/音频问题。
---

# fc-tools —— 教学 demo 与命令行工具

上游：`fc_core`、`fc_ffi`、`fc-libretro`。
下游：无（终端工具）。**只在根目录构建**（`tools/` 没有自己的 CMake 项目）。

这些 demo 是项目的教学骨架：顺序本身就是课程顺序
（二进制 → … → CPU → 总线 → 卡带 → PPU → 输入 → APU）。
改 demo 时保持这个叙述顺序和「先讲原理再输出结果」的风格。

## 0. 上下文纪律

**白名单**：

```
tools/<要改的那一个>.cpp       每个 demo 自包含，一次只读一个
tools/fc_headless.cpp          ROM -> 帧 dump / 哈希（复现问题的首选）
tools/fc_libretro_probe.cpp    加载 .dylib 的 ABI 冒烟测试
tools/run_testroms.sh  tools/compare_nestest.py
```

**禁读**：`build*/`、`packages/fc-core/tests/data/`、其它 10 个 demo
（要参考风格时用 `rg -n 'int main' -A 20 tools/demo_ppu.cpp` 只取 main 那段，
不要整读）。

demo 通常 200-500 行且是线性教学脚本：**改哪个读哪个**，不要横向全读。

## 1. 清单与运行

```bash
./build/demo_bitwise      # 位、字节、数制、补码
./build/demo_overflow     # C 与 V 标志、有符号比较（N XOR V）
./build/demo_cpu          # 取指 / 译码 / 执行循环
./build/demo_disasm       # 汇编 <-> 机器码
./build/demo_addressing   # 有效地址、zero page 回绕、JMP 间接的硬件 bug
./build/demo_instructions # 完整指令集、ADC、中断、周期
./build/demo_bus          # 地址译码、镜像、open bus、OAM DMA
./build/demo_cartridge    # iNES 文件头、Mapper 0-15、运行真实 ROM
./build/demo_ppu          # 渲染真实游戏画面 -> PPM
./build/demo_input        # 模拟按键：标题画面 -> 开始游戏
./build/demo_apu          # 五个声道波形 -> game_audio.wav
```

构建：

```bash
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build build --target demo_ppu     # 只建一个
```

## 2. 无头复现（排查问题的第一选择）

`fc_headless` 是「把问题变成可比较文件」的工具：同样的 ROM、同样的输入脚本，
输出帧 dump 或哈希。**先复现，再改代码。**

```bash
./build/fc_headless --help
./build/fc_headless <rom> --headless --dump        # 导出帧
./build/fc_headless <rom> --frames 300 --hash      # 只看哈希
```

`electron` 的 `--selftest` / `--dump` 走的是同一份 Core，所以两边的输出
**应当逐字节一致**；不一致说明问题在前端（渲染/缩放/音频），不在 Core。

```bash
./build/fc_libretro_probe ./build/fc_libretro.dylib     # ABI 冒烟
./tools/run_testroms.sh <rom-dir>                       # 批量跑测试 ROM
```

## 3. 任务菜谱

**加一个 demo**：

1. 新建 `tools/demo_<话题>.cpp`，风格对齐邻居（先说原理，再打印每一步的
   中间状态，最后给结论）。
2. 加进根 `CMakeLists.txt` 的 tools 列表（`rg -n 'demo_ppu' CMakeLists.txt` 找到位置）。
3. 在根 `README.md` 的 demo 清单里补一行（`rg -n 'demo_apu' README.md`）。
4. 若它演示的东西对应某篇文档，在 `docs/` 里互相引用。

**改 demo 的输出格式**：检查是否被 `tools/compare_nestest.py` 或
`wasm/verify.sh` 之类脚本依赖（`rg -n '<输出里的关键字>' tools/ wasm/ electron/`）。

## 4. 检索菜谱

```bash
rg -n 'demo_' CMakeLists.txt                  # demo 在哪注册
rg -n 'int main' tools/*.cpp                  # 每个 demo 的入口行号
rg -n 'Argv|--headless|--dump|--frames|--hash' tools/fc_headless.cpp
rg -n '\[demo|demo_' README.md                # 文档里的 demo 清单
```
