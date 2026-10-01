#pragma once

// ---------------------------------------------------------------------------
// Framebuffer - the PPU's only output.
//
// 256x240, one 32 bit 0x00RRGGBB value per pixel. That is the entire interface
// between the core and whatever draws it on screen. The core does not know
// about Metal, or Swift, or even that a window exists.
//
// 256 x 240 = 61440 pixels = 240KB as 32 bit. The real NES has no framebuffer
// at all: it generates a video signal one pixel at a time and never stores a
// picture. We store one because the modern world expects to be handed an image.
//
// Rule from AGENTS.md: Core must not depend on the UI. A plain pixel array is
// how you keep that promise.
// ---------------------------------------------------------------------------

#include "core/types.hpp"

#include <array>
#include <cstddef>

namespace fc::nes {

struct Framebuffer {
    static constexpr int kWidth = 256;
    static constexpr int kHeight = 240;
    static constexpr std::size_t kPixelCount =
        static_cast<std::size_t>(kWidth) * static_cast<std::size_t>(kHeight);

    std::array<u32, kPixelCount> pixels{};

    void clear(u32 colour = 0) noexcept { pixels.fill(colour); }

    void set(int x, int y, u32 colour) noexcept
    {
        if (x < 0 || x >= kWidth || y < 0 || y >= kHeight) {
            return;
        }
        pixels[static_cast<std::size_t>(y) * kWidth + static_cast<std::size_t>(x)] = colour;
    }

    [[nodiscard]] u32 at(int x, int y) const noexcept
    {
        if (x < 0 || x >= kWidth || y < 0 || y >= kHeight) {
            return 0;
        }
        return pixels[static_cast<std::size_t>(y) * kWidth + static_cast<std::size_t>(x)];
    }
};

} // namespace fc::nes
