---
name: fc-repo
description: Classic Game Box monorepo 的入口技能（路由表 + 仓库级上下文纪律）。用于跨多个包的任务、新增或搬动包、改版本号、发版、跑全量测试，或不确定该读哪个目录时。给出仓库布局、依赖方向、整体构建/测试命令、三个项目名各自的用途，并把任务路由到 fc-core / fc-libretro / fc-wasm / fc-frontend / fc-tools / fc-docs。
---

# fc-repo —— monorepo 总览与路由

先读这个技能，再读被路由到的那个包的技能。**不要直接跳到源码。**

## 0. 上下文纪律（对全仓库生效，硬规则）

本仓库同时是教学材料和一个十几万行的工程：`build*`、`wasm/dist`、
`third_party/libretro/libretro.h`（8716 行）、`electron/node_modules` 都很大。
「读整个项目」既慢，又会把真正相关的那 20 行埋掉。所以：

**先定位，再精读。一次只打开一个包。**

| 禁止 | 替代做法 |
|---|---|
| `ls -R .`、`find .`（不带 `-maxdepth`）、`cat` 整个目录 | `find <一个子目录> -type f`，或直接查下面的路由表 |
| 读 `build*/`、`wasm/dist/`、`electron/node_modules`、`electron/dist`、`electron/release`、`.git/` | 这些是产物。要判断是否过期就跑 `ctest` / 构建命令，不要读它们 |
| 整读 `third_party/libretro/libretro.h` | `rg -n "retro_serialize" <该文件>`，只看需要的那几个符号 |
| 整读 700+ 行的 `README.md` / `AGENTS.md` | 先 `rg -n '^#{1,3} ' <file>` 拿目录，再 `read offset/limit` 只读相关节 |
| 同一批文件读两遍、一次开一大堆文件 | 一轮对话默认最多打开 3 个文件；要更多先说明理由 |

搜索优先级：`rg -n '符号' <某个子目录>` → 拿到行号 → `read offset/limit` 只读那一段。
不要用 `cat`，不要用 `read` 从第 1 行读到末尾。

## 1. 布局与依赖方向

依赖方向只有一条，反向禁止：

```
wasm / tools / electron  ->  packages/fc-libretro  ->  packages/fc-core
                             packages/fc-core/src/ffi (fc_ffi)  也是从 fc-core 出发
```

| 路径 | 是什么 | 加载哪个技能 |
|---|---|---|
| `packages/fc-core/` | 纯 C++ NES 机器（CPU/Bus/Cartridge/Mapper/PPU/APU/State）+ 纯 C 接口 | `fc-core` |
| `packages/fc-libretro/` | libretro ABI 适配层（`retro_*`、金手指、custom 扩展） | `fc-libretro` |
| `wasm/` | 同一份核心的 Emscripten 构建 + JS 绑定 + mGBA/Mesen 侧模块 | `fc-wasm` |
| `electron/` | Electron + TypeScript 前端 | `fc-frontend` |
| `tools/` | 教学 demo 与命令行工具 | `fc-tools` |
| `docs/` | 教学文档（computer-science / nes / architecture / assembly） | `fc-docs` |
| `cmake/` | `Version.cmake`（版本号）、`GoogleTest.cmake` | 本技能 |
| `scripts/release.sh` | 本地打包发版 | 本技能 |

`AGENTS.md` 是项目宪法（教学优先、分阶段、架构约束）。改代码前用
`rg -n '^# ' AGENTS.md` 定位章节再读那一节，**不要整读**。

## 2. 构建与测试

根目录只做组装，不含模拟逻辑。全量一条命令（native C++ + wasm + Mesen/mGBA
+ 前端 + 全部测试，七步，失败即停并报出是哪一步、跑了多久）：

```bash
./scripts/build-all.sh              # 全量
./scripts/build-all.sh --fast       # 跳过两个第三方核心（几分钟，与 packages/ 无关）
./scripts/build-all.sh --verify     # 再加 wasm/verify.sh 与 electron/verify.sh 的逐像素校验
```

想单独跑某一步时：

```bash
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build build
ctest --test-dir build --output-on-failure
```

单包构建见各自的技能。写代码时用 `ctest -R <正则>` 只跑相关测试，
不要每次都跑全量 517 个；**只在准备合并前**跑一次全量。

## 3. 版本号：一处修改，两处同步

- `cmake/Version.cmake` —— C++ / libretro 的 `library_version`
- `electron/package.json` —— npm / electron-builder

`scripts/release.sh` 同时写这两处，并会检测两者是否已经不一致。
**不要只改一个。**

## 4. 发版

```bash
./scripts/release.sh --dry-run   # 预演：构建、打包、生成 SHA256，不发 Git 也不传 GitHub
./scripts/release.sh 0.2.0       # 改版本号 -> 构建 -> 打标签 -> 推送 -> 上传
```

前置条件：工作区干净、当前在 `main`、`gh` 已登录、`v<版本>` 标签不存在。

## 5. Git 约定

- 功能分支用 `--no-ff` 合并进 `main`，保留合并点（历史上
  `Merge branch 'monorepo-refactor'`、`Merge branch 'libretro-abi'` 都是这样）。
- 提交信息一句话说清「哪个部件 + 做了什么」，正文解释根因，参考现有 `git log` 的风格。
- 合并前：全量 `ctest` 通过 + 工作区干净。

## 6. 什么时候不必用技能

改一个 typo、跑一个已知命令，直接做。技能的用途是「不知道该从哪下手」和
「不确定这次改动会波及谁」。
