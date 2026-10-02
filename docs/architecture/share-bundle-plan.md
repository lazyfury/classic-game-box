# 分享数据包（`.cgb` = zip + `manifest.json`）设计

状态：**待处理（仅设计，未实施）**。目标：把游戏库里的**一个游戏（或整库）**的可分享
数据（存档 / 截图 / 金手指 / 元数据，可选 ROM）打成自包含的包，方便导入到别人的库里。

## 已定方向

- **容器用 ZIP**（`zip` 已是依赖，自带 CRC32 / deflate / 随机访问），顶层一个
  `manifest.json`。所谓「自定义格式」就是一个约定后缀（`.cgb`）的标准 zip，**别自创容器**。
- **清单用 JSON**（`serde_json` 已在依赖）。**不用 YAML**：`serde_yaml` 上游已归档
  （2024），隐式类型 / 锚点是坑，机器生成的清单没必要。
- **身份 = `file_name` + `system`**，和 `Library::sync` 的「按文件名对位」一致。
- 压缩：deflate；已经压缩过的 ROM（`.zip` / `.chd`）可 store 不压。
- 完整性：zip 自带 per-entry CRC32，清单里再放 `size` 即可，**不需要全量 hash**。

## 清单草案（`manifest.json`，顶层）

```json
{
  "format_version": 1,
  "created_by": "Classic Game Box 0.2.0",
  "games": [
    {
      "file_name": "Super Mario Bros. 3.nes",
      "name": "超级马里奥 3",
      "system": "nes",
      "core_key": "mesen",
      "tags": ["平台", "童年"],
      "rom": "roms/nes/Super Mario Bros. 3.nes",       // 可省略 → 只分享数据、不带 ROM
      "saves": [
        {"core_key": "mesen", "kind": "manual", "slot": 1,
         "state": "saves/Super Mario Bros. 3.nes.mesen.state1",
         "thumb": "saves/Super Mario Bros. 3.nes.mesen.state1.png"}
      ],
      "cheats": ["cheats/Super Mario Bros. 3.cht"],
      "screenshots": [
        {"file": "screenshots/xxxx.png", "width": 256, "height": 224,
         "created_at": 0, "cover": true}
      ],
      "other": {}                                       // 预留扩展
    }
  ]
}
```

- `format_version` 让导入端拒绝 / 转换不认识的版本。
- 一个包可含多个 `games`（整库导出就是多个）。

## 导入流程（可扩展）

现在导入是 `import_roms(dir, sources)`（拷文件）+ `rescan_library`。包导入是**并列的新路径**：

1. 按后缀识别 `.cgb`（在 `App::add_game_paths` / `flush_drops` 里分流）。
2. 解压到 staging：**必须防 zip-slip**（拒绝绝对路径 / `..`）并限制大小。
3. 按清单分区放置：ROM → `roms/<system>/`；存档 → `saves/`；金手指 → `cheats/`；
   截图 → `screenshots/`。
4. **可推导的**（games / saves / cheats）交给现有 `sync` / `sync_saves` / `scan_cheats`
   对账；**不可推导的**（截图行、`name` / `tags` / 置顶 / 封面）由导入器直接写 DB 行。
5. 收尾 `rescan_library`。

**扩展性**：清单按 section 分派，以后加 `replays` / `other` 只是多一个分支；老导入器
忽略未知 section 即可；`format_version` 管大改。

## 难点（不是格式问题）

1. **截图不可从扫描重建**：`saves` / `cheats` 能从文件名扫出来，但截图文件名是随机
   `{millis}.png`，**只能靠 DB 行关联**。清单必须带「截图 → 游戏 + 是否封面」，
   导入器写 `screenshots` 行并处理 `cover_id`。
2. **合并策略**：接收方已有同一游戏（同 `file_name + system`）时，存档 / 截图是
   「跳过」「覆盖」还是「另存并列」？必须定。建议先**不覆盖**（重名加 `(2)`）并让用户确认。
3. **ROM 的合法性**：包里带 ROM 有版权风险（saves / 截图 / 金手指没事）。建议
   `rom` 字段可省略——只分享「游戏数据」，接收方自备 ROM，按 `file_name + system` 对上即合并。

## 待拍板

1. 容器接受 **zip + `manifest.json`（JSON）**？
2. 粒度：只做**单游戏**分享，还是也支持**整库导出**？
3. ROM：默认**不打包**（只分享存档 / 截图 / 元数据），还是允许可选打包？
4. 合并：重名默认**不覆盖**（加后缀）对吗？
