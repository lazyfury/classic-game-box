# 0003 schema 变更整库重建，不做迁移链

- 状态：已接受（回溯整理）
- 背景：库模型演进很快（`games` / `tags` / `game_tags` / `screenshots` /
  `save_states` / `cheats`），子表都 `ON DELETE CASCADE`。维护一条逐版本升级的
  diesel migration 链，成本高、容易写错，而用户基数尚小。
- 决定：
  - `PRAGMA user_version` 与本构建的 `SCHEMA_VERSION` 不一致时，**整库重建**，
    然后自动重扫一次（这是唯一的自动 sync）。
  - 平时**不自动 sync**；重扫由库页面「重新扫描」手动触发。
- 否决的方案：
  - diesel migration 链 / 逐版本升级 —— 长期的正确做法，但现在不划算。
  - 保留旧表做兼容读取 —— 模型会越背越乱。
- 后果：
  - 坏库/版本不符能自愈，实现简单。
  - 代价：**schema 一改，用户元数据（改名、置顶、标签、游玩时长）会随重建丢失**。
    截图/存档 PNG 一旦被删本地无法恢复。这条要明确写进发版说明。
- 权威文档：[`../../../src/library/db.rs`](../../../src/library/db.rs) 顶部注释、
  [`../../../AGENTS.md`](../../../AGENTS.md)（Q3）。
