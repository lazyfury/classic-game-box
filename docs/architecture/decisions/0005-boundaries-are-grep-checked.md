# 0005 模块边界写成可判定规则，并由脚本守住

- 状态：已接受（回溯整理）
- 背景：模块职责靠口头约定或一份大文档来维持，会随开发腐烂。本仓库本来就有
  「用 grep 保持为空」的架构验证传统（例如 `custom_nes_core` 的 CPU 层不得
  `#include "core/nes/"`，见 `docs/architecture/README.md`）。
- 决定：
  - 把模块边界提炼成**可判定规则** `[B1]`–`[B6]`，写在根目录
    [`CONVENTIONS.md`](../../../CONVENTIONS.md)。
  - 用 [`scripts/check-boundaries.sh`](../../../scripts/check-boundaries.sh) 实现，
    并入每阶段 gate [`scripts/dev.sh`](../../../scripts/dev.sh)，在 fmt/clippy/test
    之前跑（便宜、快速失败）。
  - 改公共边界时按顺序：先加一条本目录的决策记录 → 改 `CONVENTIONS.md`
    → 同步改校验脚本。
- 否决的方案：
  - 只写文档、不做校验 —— 规则很快过期。
  - 只靠 code review —— 会漏，且新人/agent 不知道规则存在。
- 后果：
  - 边界只能靠**同时改脚本**来跨越，否则 gate 直接失败（除非显式修改脚本并留下记录）。
  - 检查忽略注释里的提及：文档注释**命名**一条边不算依赖。
  - 无法用 grep 判定的（UI 观感、是否该加依赖）明确列为人工评审，不假装是硬约束。
- 权威文档：[`../../../CONVENTIONS.md`](../../../CONVENTIONS.md)。
