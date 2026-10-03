# 输入与玩家/端口分配设计（键盘 + 多手柄 + 1P/2P…）

状态：**进行中**。目标：把「键盘 / 多个手柄 / 网络远程输入」统一成一个规范的分层输入模型，
支持 1P/2P… 分配、按机种对齐 libretro 端口数。

## 已确认的决策

1. **端口数对齐 libretro**：编译期 `MAX_PORTS = 8`（RetroArch 的 8 玩家惯例）；实际可用数
   取核心上报的 `RETRO_ENVIRONMENT_GET_INPUT_MAX_USERS`，UI 据此显示。
2. **键盘两种模式**：单人（WASD 与方向键**同时**映射到 P1）／双人（WASD = P1，方向键 = P2）。
   每个玩家一套键位表。
3. **分配方式两种都要**：手动选择设备 + 「按 **Start** 认领」；**分配 UI 放在游戏区域
   （play view）的按钮组区域**，也可从设置页进。主控（port 0）长按 **Select** 重置全部。
4. **手柄后端**：默认用 Rust 的 **`gilrs`**（先做评估 spike，通过后设为默认）；**macOS 用原生
   `GameController` 作为例外**（`gilrs` 会把蓝牙 Xbox 手柄的映射认错）。用一个「判断逻辑」
   （host 提供了 `SharedGamepad` 就用它，否则用 `gilrs`）来选。
5. **网络对战本期只预留**：输入层与传输解耦（`Source::Remote`），netplay 之后作为独立模块接。

## 术语与分层

```
物理来源 Source            →   玩家 Player / 端口 Port    →   libretro
├─ Keyboard(键位表)              P1..P8                      port 0..7
├─ Gamepad(device_id)            每个 Port 一个 Binding       RETRO_DEVICE_JOYPAD / *_ANALOG
└─ Remote(client_id)  (预留)     （multitap 再映射）          SET_CONTROLLER_PORT_DEVICE
```

| 术语 | 含义 |
|---|---|
| `Port` | `0..MAX_PORTS`，libretro `input_state` 的 port |
| `Player` | UI 上的 1P/2P…（默认 `Player-1 == Port`，允许错位） |
| `Source` | `Sides`：`Keyboard` / `Gamepad(DeviceId)` / `Remote(ClientId)` / `None` |
| `Binding` | `Port -> Source`，持久化 |
| `DeviceId` | 手柄的稳定标识（见下） |

**职责边界**：
- **壳（Swift/C++）只做设备枚举 + 原始状态**（连接/断开、按钮/轴），不做「谁是谁」。
- **app 做分配**（`Binding` 表）；键盘、手柄、远程在壳之外统一。
- **libretro 边界**：`port` 直接喂 `input_state`；设备类型走 `SET_CONTROLLER_PORT_DEVICE`。

## `cgb-libretro` 改动

- `MAX_PORTS: usize = 8`（新常量）。
- `InputState`：`keyboard`/`gamepad` 从 `[u16; 2]` → `[u16; MAX_PORTS]`；`analog` 同理
  `[[[i16;2];2]; MAX_PORTS]`。`mask(port)` / `set_gamepad_mask` / `set_analog` 的越界行为
  保持「忽略」。
- `GamepadSnapshot`：同样 N 端口（或改成 `Vec`/数组），`apply` 遍历 `0..MAX_PORTS`。
  ABI 不变（`cgb_host_gamepad_state(port, …)` 的 `port` 已是 `u32`，只是底层真的支持到 7）。
- **键盘键位表**：`KeyboardBindings` 增加「归属端口」概念。两种模式：
  - 单人：一份表，`WASD` 与方向键都绑到 P1（现状 + 明确）。
  - 双人：P1 表（WASD/J K U I …）、P2 表（方向键/数字小键盘 …）。
  统一成 `KeyboardLayout { mode, binds: Vec<(PlayerSlot, Key, JoypadButton)> }`，或
  «每个 Port 一张 `KeyboardBindings`» + 模式决定方向键归谁。

## app 改动

- `InputBindings`：`[Source; MAX_PORTS]`（或 `Vec`），存 `settings.json`。
- 设备管理：枚举手柄 → 分配 `DeviceId` → 决定哪个 `Port`。支持：
  - **自动分配**：新连接的设备占「第一个空闲端口」，与 `gilrs` 对齐（单手柄即 1P）；
    手动「未分配」后不会被反弹。
  - **按下认领**：分配模式里，第一个按下 **Start** 的**未分配**设备占「第一个空闲端口」。
  - **手动选择**：从已连接设备列表里选（「输入分配」的下钻菜单：手柄 → `1P..4P/未分配`）。
  - **重置**：主控（port 0）长按 **Select** ≈1s 清空全部，需再长按一次确认。
- 热插拔：断开时清空该端口并标记；重连按 `DeviceId` 恢复绑定（拿不到稳定 id 时退回「首个空位」）。
- 键盘事件 → 按模式查对应端口表。
- 每帧把 `InputState` 交给会话（现在 `App::step_gamepad` 的位置）。

## 手柄后端（关键）

- **非 macOS：`gilrs`（Rust）为默认**。它跨平台、支持 **>4** 个手柄（Windows 上 XInput 只有 4，
  `gilrs` 还走其它 API）。先做评估 spike：连接数、断连、轴/扳机、热插拔、Xbox/PS/Switch 映射。
- **macOS：原生 `GameController`（Swift）为例外**。保持现在的 `SharedGamepad` 注入路径
  （`App::init` 优先用 host 提供的 `GamepadSource`）。
- **判断逻辑**：`App::init` 若拿到 `SharedGamepad` 服务 → 用它（macOS）；否则构造 `gilrs`
  （Windows/Linux）。可用环境变量/设置覆盖，便于调试。
- **`DeviceId`**：`gilrs` 用其 `Gamepad::id()`（进程内稳定）；macOS 原生用
  `vendorName + productCategory + 实例序号`（系统不保证跨重连 UUID）。

## 每机种端口数

- 上限由核心决定。默认值 + multitap core option：
  NES/SNES/GB/GBA/Genesis/SMS 2；**N64 4**；**Saturn/PSX 2（multitap 4/8）**；街机 2–4。
- UI 显示「本核心支持 N 名玩家」，超出部分的绑定灰掉。

## UI

- **游戏区域（play view）**：按钮组区域的「输入分配」入口，按钮文本是「键盘单人/双人 +
  手柄1（1P）」；点开是下钻菜单（手柄列表 → 每个手柄 → `1P..4P/未分配`）。菜单底部
  「分配模式…」打开分配模态框：等待手柄反馈、按 Start 认领、列表显示各手柄端口、
  主控长按 Select 重置全部、「确认完成分配」关闭。
- **设置页 → 输入**：键盘模式（单人/双人）、各玩家键位重映射、设备类型（`SET_CONTROLLER_PORT_DEVICE`）。

## 网络对战（本期只预留）

- libretro **没有 netplay API**；标准是**前端级确定性 lockstep / rollback**（双方跑同一 core+ROM，
  逐帧交换输入，rollback 用 `retro_serialize`）。`RETRO_ENVIRONMENT_SET_NETPACKET_INTERFACE`
  只给「核心自己要联网」的场景。
- 「远程手柄 / 输入中继（串流式）」不是标准、延迟差，不做。
- 做法：`Source::Remote(client_id)` 作为 `Source` 的一种；netplay 作为**独立模块**把每个
  远程玩家的输入注入对应 `Port`。本地/远程共用同一 `InputState`，所以现在不做也不挡。

## 阶段

- **P1**：`cgb-libretro` N 端口 + 键盘双模式；app `InputBindings` + settings。
- **P2 完成**：`gilrs` 已设为**非 macOS 默认**（`src/app/gamepads.rs`，`App::init` 的 `cfg!(target_os = "macos")` 判断逻辑：macOS 用 host 的 `GameController`，其余用 `gilrs`）。
- **P3 完成**：设置页 → 输入 的「键盘分配」卡片（单人/双人）；play view 按钮组的「输入分配」下钻菜单：**已连接手柄列表 → 每个手柄 → 1P/2P/3P/4P/未分配**（当前端口打 ✓，点击即分配），按钮文本显示「键盘单人/双人 + 手柄1（1P）」。
  「分配模式」模态框：按 **Start** 认领（只认未分配的手柄）、主控长按 **Select** ≈1s 重置全部（二次确认）、「确认完成分配」关闭；模态框打开时保持帧循环以轮询手柄。macOS 与 gilrs 统一为「shell 只报**设备级**原始状态（`cgb_host_gamepad_device`/`_state` 按 slot），app 把 slot 映射到 port」，且连接时默认占第一个空闲端口。
  仍缺：设置页的**逐键重映射**；每核心端口数（暂用固定 4）。
- **P3.1 修复（卡方向）**：`1d2a1ea` 把 macOS 从「每帧整体快照」（`GamepadSnapshot::apply` 覆盖全部端口）改成「按 slot 打补丁」时，丢了「每帧重置」这个不变量：`NativeGamepad::poll` 只写有映射的端口，未分配 / 换槽 / 断开漏清的端口会**冻结在最后一帧的方向位**；`assign` 结构上又拿不到 `InputState`，所以「未分配 → 1P」只是恰好重写了一遍那个端口，看起来像修好了。修复：
  - `InputState::clear_gamepad()`：每帧开头只清 gamepad 半区（buttons + analog），再按映射写入——**从零重算**，陈旧位不可能存在；键盘半区不动（两者仍 OR）。
  - macOS 槽位重建：`Gamepads.swift` 以 `GCController.controllers()` 为准 `reconcile()`（连接时先 reconcile，兜底蓝牙漏掉的 `DidDisconnect`），`attach` 幂等、`extendedGamepad` 未就绪不宣告 connected。修掉「幽灵 slot 占着 port 0 卡方向、真手柄被移到 port 1」。
  - **手柄独立循环**：新增 `cgb_host_poll(app) -> bool`（`App::poll_input`，不布局不绘制）；macOS 用 60Hz `Timer` 跑 `reconcile()` + `poll`，Windows 空闲 `MsgWaitForMultipleObjectsEx` 改 16ms 超时后 `poll`，返回 true 才 `RunFrame()`。这样空闲时手柄仍有自己的节拍。
- **P4**（之后）：netplay（lockstep → rollback）。
