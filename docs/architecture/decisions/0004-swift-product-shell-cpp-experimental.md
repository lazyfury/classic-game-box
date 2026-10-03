# 0004 Swift/macOS 是产品壳，C++/Win32 是实验壳

- 状态：已接受（回溯整理）
- 背景：产品要一个原生 macOS app。AppKit 只能用 Swift 写，早先的 `winit` 路径在
  macOS 上不理想；Windows 侧又需要一个原生壳，且没有 Windows 真机，必须优先
  「最大控制权、最少抽象层」。
- 决定：
  - 产品前端是 `macos/` 的 **Swift app**：只做 `NSWindow` + `CAMetalLayer` +
    AppKit 原生事件；UI、wgpu 渲染、libretro 全在 Rust。
  - Rust 侧统一在 `src/native/`，经 **`cgb_host_*` C ABI** 暴露，产出
    `libcgb_app.a` 静态链接进壳。
  - `windows/` 的 **C++/Win32 壳是实验性第二壳**，与 Swift 壳共用同一份
    `src/native/` 与 C ABI。
  - 不再把 `winit` 作为产品路径（依赖仍在，供非产品/测试路径）。
- 否决的方案：
  - 用 `winit` 统一跨平台窗口 —— 产品 macOS 路径弃用（虚拟机上 swapchain
    重建也不稳）。
  - Rust 直接用 `windows-rs` 写整个 Windows 层 —— 能省 C ABI，但与 macOS 壳形状
    不对称；既然目标是对标 macOS host，就保持同构。
- 后果：
  - 平台差异只留在两处：`surface::create_surface`（CoreAnimationLayer / HWND）与
    `input::key_from_code`（AppKit keyCode / Win32 VK）。
  - 代价：壳本身（Swift、C++）必然分开，无法一份代码通吃。
- 权威文档：[`../swift-macos-host-plan.md`](../swift-macos-host-plan.md)、
  [`../windows-host-plan.md`](../windows-host-plan.md)、
  [`../quill-native-migration.md`](../quill-native-migration.md)。
