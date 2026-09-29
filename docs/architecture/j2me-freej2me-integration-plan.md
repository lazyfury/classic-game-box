# J2ME 接入（Java ME / FreeJ2ME-Plus）

状态：**已实现，能出画面、能用手柄玩**（`cargo run -p cgb-app -- --rom game.jar`）。
本文记录选型、架构、宿主改动、两个必须绕开的上游 bug，以及已知缺口。

“Java 游戏”在复古语境里指 **J2ME（Java ME）功能机游戏**（`.jar` / `.kjx`），
不是桌面 Java 或 applet。唯一现实的路线是 **FreeJ2ME-Plus 的 libretro 核心**
（`TASEmulators/freej2me-plus`，fork 自 `hex007/freej2me`）。

---

## 1. 结论先行

- 核心不是纯 C：libretro 模块只是 **C shim**，`fork/exec`s 一个 Java VM
  （`freej2me_plus-lr.jar`），双方走 **stdin/stdout 管道**。
- 视频是软件 **XRGB8888**（Java 送 RGB888，shim 转 XRGB8888），复用现有软件
  `Frame` 路径；**零 GL 改动**。
- 因此多了两件别的核心没有的事：**要随包一个 JRE**，**构建 jar 要 JDK**。
- 上游 macOS 支持已就绪（`__APPLE__` + `make platform=osx`，arm64 实测可编）。

## 2. 为什么不走别的路

| 路线 | 结论 |
|---|---|
| **FreeJ2ME-Plus（libretro）** | ✅ 采用：官方支持 osx/arm64，GPLv3 |
| `hex007/freej2me` 原版 | ❌ C 文件只有 `__linux__`/`_WIN32` 分支，Mac 缺代码 |
| `libretro/libretro-JamVM` | ❌ 2017 年后停更、无 license |
| SquirrelJME / FroggyKVM / wie-libretro | ❌ 不是 libretro / 玩具 |
| freej2me 独立 AWT/SDL2 版 | ❌ 自己开窗口，画面进不了我们的视图 |

## 3. 架构

```
SystemId::J2me (.jar/.kjx)
  └─ freej2me_plus_libretro.dylib (C shim)
       └─ fork/exec: java -jar freej2me_plus-lr.jar <w> <h>
             ↕ stdin/stdout 管道（帧/输入/存档路径/配置）
        video_cb(XRGB8888) → 现有软件 Frame → FrameImage
        输入：JOYPAD + ANALOG（手机键盘已映射到 16 键）
        存档：java 写 <save_dir>/freej2me/；无即时存档
```

管道协议（`src/org/recompile/freej2me/Libretro.java` ↔ `freej2me_libretro.c`）：
5 字节事件头 + 负载；`0xA` 载入、`0xB` 存档路径、`0xD` 启动、`0xF` 请求帧、
按键事件、配置串。

## 4. 组件与部署

构建 `./cores/freej2me_plus/build.sh` 产出三样：

| 产物 | 位置 |
|---|---|
| C core | `cores/dist/freej2me_plus_libretro.dylib` |
| Java 程序 | `cores/dist/freej2me_plus/freej2me_plus-lr.jar` |
| 精简 JRE | `cores/dist/freej2me_plus/runtime/`（`jlink`） |

app 启动时（`crates/cgb-app/src/app.rs`）：

- `j2me_dir()`：打包后在 `Resources/freej2me_plus/`，开发时看
  `cores/dist/freej2me_plus/`。
- 把 **jar** 复制进可写的 `<app data>/system/`（core 在那里找它）。用 `fs::copy`
  **覆盖**，不用 `seed_dir`（后者保留已存在文件，会把旧 jar 留住；jar 是随包资源、
  不是玩家数据，重建后必须生效）。
- `prepend_path` 把 `runtime/bin`（canonicalize 成绝对路径）前置到 `PATH`：
  core 用 `execvp("java")` 找 JVM，且它在 exec 前 `chdir` 到 system 目录，
  所以必须是绝对路径。**没有改动 core**。
- 没构建 bundle 时不改 `PATH`，退回系统 `java`。

打包 `scripts/package-macos.sh` 把 `cores/dist/freej2me_plus/` 拷进
`Resources/freej2me_plus/`，并用 `codesign --deep` 连嵌套 JRE 一起签名。

## 5. 运行时版本（实测）

| 问题 | 结论 |
|---|---|
| 运行时下限 | 官方 release jar 是 Java 6 字节码；我们编译目标定 Java 8 |
| 运行时上限 | 无。**JDK 21** 实测正常 |
| 构建 jar 的 JDK | 上游 Ant 脚本要 **JDK 8**（`-source 1.5` + `lib/rt.jar`）。我们绕过 Ant，直接用现代 JDK `javac --release 8`（1217 个源文件全过） |
| 随包运行时 | `jlink --add-modules java.base,java.desktop,jdk.charsets --compress=2` |

两个易漏点：

- **必须带 `java.desktop`**（内部用 AWT 做字体/图像），不能只带 `java.base`。
- **`jdk.charsets` 决定 CJK**：默认 `ISO-8859-1` 在 `java.base` 里够用，但
  `Shift_JIS` / `EUC_KR` / `GBK` 只在 `jdk.charsets` 模块里；日韩中文游戏需要它。

## 6. 宿主改动

- `cgb-systems`：`SystemId::J2me`（扩展名 `jar` / `kjx`）。
- `cgb-library`：`settings.rs` 的 `j2me_core`；`cores/cores.json` 加
  `freej2me_plus` 行。
- `cgb-input`：J2ME 默认键位。关键点：核心的 libretro **`SELECT` = 左软键**（LCDUI 菜单用），
  **`Y` = OK/Fire**（Canvas 游戏的“选择/确认”），`START` = 右软键。所以“有些游戏对 Select
  没反应”是因为那些游戏用 `FIRE`/`KEY_NUM5`，不是软键。默认键位因此把 **Enter/Space = OK/Fire**
  （原来绑的是 Start），`q` = 左软键（Select）、`e` = 右软键（Start），数字键映射手机键
  （1/3/5/7/9、0=CLR、5 同时是 Num5/Fire）。
- `cgb-app`：`session.rs` 对 J2ME 关闭倒带（无 `retro_serialize`）。
- **前端默认值**：manifest 支持 `option_defaults`，`Session::start` 在 **load 之前**套用。
  J2ME 用它把 `freej2me_backlightcolor` 设成 `Disabled`——核心自带默认是 `Green`
  （模拟早期单色机 LCD 背光），会给所有彩色游戏蒙一层 `0xFF77EF5A` 的绿；独立 AWT
  前端也默认 `Disabled`。玩家在设置里改过的值仍会覆盖它。
- `cgb-libretro`：
  - **core options v2**：`GET_CORE_OPTIONS_VERSION` 报 2，新增
    `SET_CORE_OPTIONS_V2` 解析（`retro_core_options_v2` / `v2_definition`）。
  - **`GET_RUMBLE_INTERFACE`**：返回一个 no-op `set_rumble_state`。

### 6.1 四个必须绕开的上游 bug / 行为

1. **v1 下把 v2 数组当 v1 读。** `freej2me_libretro.c` 在
   `core_opt_version == 1` 时把 `struct retro_core_option_v2_definition`
   数组传给 `SET_CORE_OPTIONS`（应为 v1 布局）。字段错位 → 前端读不到
   `freej2me_resolution` 的默认值 → 分辨率变 `0x0` → Java 侧
   `getLcdFrontbuffer()` NPE。**对策：宿主说 v2，走 `SET_CORE_OPTIONS_V2`。**
2. **rumble 空指针。** `retro_run` 的 `else` 分支无条件调
   `rumble.set_rumble_state(...)`，而前端若在 `GET_RUMBLE_INTERFACE` 返回
   false，这个指针是 NULL → **SIGSEGV**（RetroArch 总提供接口，所以上游没发现）。
   **对策：宿主提供 no-op 实现**（同 `GET_LOG_INTERFACE` 的处理思路）。
3. **非 ASCII 路径读成乱码。** shim 用 UTF-8 字节把游戏/存档路径通过管道发给
   Java，但 `Libretro.java` 用 `new String(buffer, 0, bytesRead)` 解码（JVM 默认字符集，
   core 又固定成 `-Dfile.encoding=ISO_8859_1`），于是中文游戏名被二次编码，
   `new File(...).isFile()` 返回 false → `System.exit(0)` → **一直黑屏**。
   **对策：`build.sh` 在编译前给 `Libretro.java` 打两行补丁**（游戏路径与存档目录
   显式按 UTF-8 解码），并校验补丁已命中。ASCII 路径不受影响。
4. **按键重复被加速到 60Hz。** libretro 模式里 `Libretro.java` 在**每个仿真帧**都
   调一次 `keyRepeated`（前端每帧都请求帧），按住一个键 = 每秒重复 ~60 次；真实手机
   是“~400ms 首次延迟 + ~12Hz”。按住方向/菜单会飞。
   **对策：`build.sh` 给 `Libretro.java` 打补丁做限速**（首次 400ms，之后 80ms），
   并在按下/松开时重新计时。注意：上游本身没有这个开关，也无法从宿主侧控制（宿主只
   提供持续按住的位掩码，重复是核心自己合成的）。

## 7. 验证

- 单元/集成：`cargo test -p cgb-libretro --test cores_run_through_the_host`
  会打开核心、断言 `jar`/`kjx` 扩展名、`need_fullpath`，并检查
  `freej2me_resolution == "240x320"`（直接回归 bug ①）。无 jar/JRE 时跳过。
- 出画面：手工跑一个 MIDlet `.jar`，`CGB_PERF=1` 下能看到帧；核心侧
  `run_frame` 返回 240×320 RGBA。
- 颜色：用一个画纯红/绿/蓝/白四段的 MIDlet，`load_game` 前套用 manifest 默认
  （`freej2me_backlightcolor=Disabled`），四段应是精确的 `[255,0,0]` / `[0,255,0]` /
  `[0,0,255]` / `[255,255,255]`。
- gate：`./scripts/dev.sh`、`cargo run -p cgb-app -- --selfcheck`。

## 8. 已知缺口（先记录）

- **音频不经 libretro**（当前按 A 方案收尾）：核心从不调用 `retro_audio_sample_batch`，
  管道里也没有音频命令。声音是 **Java 子进程自己**用 JavaSound（`Clip` / `Synthesizer` /
  `SourceDataLine`）直接播到 CoreAudio 的——所以**能出声**，但**不听前端控制**：
  暂停游戏时它照放，应用内音量/设备管不到，也不进 `cgb-audio`。实测游戏时子进程有
  活跃的 `com.apple.audio.IOThread.client`，即确实在输出。若要做暂停静音/应用内音量，
  需把 PCM 经管道转给 libretro（见下方“音频”小节）。
- **无即时存档/倒带**：`retro_serialize` 返回 false（`Session` 已按空串拒绝）。
- **键盘回调未接**：核心用 `SET_KEYBOARD_CALLBACK` 收键盘；目前只有 joypad
  （手机键盘已映射到 16 键，可玩），文本输入类游戏受限。
- **鼠标/触摸未接**：核心读 `RETRO_DEVICE_MOUSE`/`POINTER`，宿主返回 0。
- **手柄的“Select”键是左软键，不是确认**：确认是 `Y`（OK/Fire，Xbox 手柄顶面键），
  因为核心的输入描述就是这样。键盘已把确认放到 Enter/Space。
- `GET_RUMBLE_INTERFACE` 只保证非空，**不真震动**。
- `.jad`（描述文件）不被核心声明支持，只认 `jar`/`kjx`。
- 启动依赖子进程 JVM：启动慢、内存占用高；`.jar` 会被库扫描当成游戏。
- 核心抗噪有限：Java 侧启动时往 stdout 打的日志（这个游戏约 8 KB）会先被当作
  帧噪声丢弃，约 30 帧（~0.5s）后才拿到真实画面。再多就会被
  `FRAMES_DROPPED_MSG` 顶住。
