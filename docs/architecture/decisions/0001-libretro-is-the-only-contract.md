# 0001 libretro 是唯一对外契约

- 状态：已接受（回溯整理）
- 背景：旧栈是 Electron + WebAssembly + 自研核心，前端与核心之间用一套私有的
  `fc_*` 接口。每接入一个新机种都要重写一遍适配层，私有 ABI 也把第三方核心
  挡在门外。
- 决定：
  - 只加载**标准 libretro core**；前端只实现 libretro **frontend** 一侧
    （environment / video / audio / input / serialize / memory），以 `libretro.h`
    为权威。
  - 不再把 `fc_*` 私有扩展当契约。自研 FC 核心退为 `custom_nes_core/`（只读），
    它仍会导出该扩展，但**被忽略**。
  - 新功能先问「libretro 有没有标准对应」，没有就不做或单独评估。
- 否决的方案：
  - 继续维护自研 ABI —— 每接一个核心都要写适配，无法即插即用。
  - 把私有扩展作为主契约换取性能 —— 会把能力锁死在自家前端。
- 后果：
  - 任意第三方 libretro core 即插即用（Mesen / mGBA / SNES9x / ParaLLEl-N64 /
    PPSSPP / Beetle PSX 等）。
  - 代价：功能受 libretro 的接口模型限制（金手指是字符串、core options、内存映射
    走 `SET_MEMORY_MAPS`），部分能力要按标准方式重新表达。
- 权威文档：[`../libretro-migration.md`](../libretro-migration.md)、
  [`../../../AGENTS.md`](../../../AGENTS.md) 硬规则 2、3。
