//! The resource inspector — methods on [`super::App`].
//!
//! The app reads the running core's memory regions (published through libretro's
//! `SET_MEMORY_MAPS`), hands the bytes to the pure decoders in
//! [`crate::inspect`] and uploads the result as one texture. The view only lays
//! out what the model carries.

use super::*;

use crate::inspect::InspectImage;
use crate::inspect::{gba, hex, nes};
use crate::ui::INSPECTOR_HEX_VIEW;

/// One decoded image plus the caption shown beside it.
type Decoded = (InspectImage, String);

impl super::App {
    /// Rebuild the inspector's region list, re-decode the selected view and
    /// upload it. Called on entry, on a view change and on an explicit refresh.
    pub(super) fn refresh_inspector(&mut self) {
        let regions = self.regions();
        // Keep the hex view's region index in range as the core's map changes.
        if regions.is_empty() {
            self.inspector_region = 0;
        } else {
            self.inspector_region = self.inspector_region.min(regions.len() - 1);
        }
        self.model.inspector_region = self.inspector_region;
        self.model.inspector_hex_offset = self.inspector_hex_offset;
        self.model.inspector_hex_total = regions
            .get(self.inspector_region)
            .map(|region| region.len)
            .unwrap_or(0);
        self.model.inspector_regions = region_rows(&regions);
        self.model.inspector_ready = self.inspector_ready(&regions);

        match self.decode_inspector(&regions) {
            Some((image, caption)) => {
                self.model.inspector_caption = caption;
                self.model.inspector_image = self.upload_inspector(&image);
            }
            None => {
                self.model.inspector_caption = if self.session.is_some() {
                    "此视图需要该机种的图形内存（试试「内存」）".to_string()
                } else {
                    "没有正在运行的游戏".to_string()
                };
                self.model.inspector_image = None;
            }
        }
        self.dirty = true;
    }

    /// Re-decode and stream the texture *without* rebuilding the tree. The draw
    /// list references the texture id, so its new contents show at the next
    /// paint. The hex view is left out: it only changes on an explicit action.
    pub(super) fn sync_inspector(&mut self) {
        if self.inspector_view == INSPECTOR_HEX_VIEW {
            return;
        }
        let regions = self.regions();
        if let Some((image, _)) = self.decode_inspector(&regions) {
            if let Some(handle) = self.upload_inspector(&image) {
                self.model.inspector_image = Some(handle);
            }
        }
    }

    /// The running core's memory regions, owned (empty when nothing runs).
    fn regions(&self) -> Vec<MemoryRegion> {
        self.session
            .as_ref()
            .map(|session| session.memory_regions())
            .unwrap_or_default()
    }

    /// Whether the core published anything the inspector can show. The hex
    /// view works for any region, so even a core without 2D graphics memory is
    /// selectable.
    fn inspector_ready(&self, regions: &[MemoryRegion]) -> bool {
        !regions.is_empty()
    }

    /// Read the regions the running system's decoder needs and decode the
    /// selected view. `None` when none of them are published.
    fn decode_inspector(&self, regions: &[MemoryRegion]) -> Option<Decoded> {
        // The hex dump is system-agnostic: it renders whichever region the
        // user picked, so it works for any core that publishes a map.
        if self.inspector_view == INSPECTOR_HEX_VIEW {
            let region = regions.get(self.inspector_region)?;
            let data = read_region(region);
            let start = self.inspector_hex_offset.min(data.len());
            let end = (start + hex::PAGE_BYTES).min(data.len());
            let caption = format!(
                "{:#08X}–{:#08X} · 共 {}",
                region.start + start,
                region.start + end,
                format_size(data.len())
            );
            return Some((hex::dump(&data, start), caption));
        }

        match self.active_system {
            SystemId::Gba => {
                let vram = read_start(regions, 0x0600_0000);
                let palette = read_start(regions, 0x0500_0000);
                let oam = read_start(regions, 0x0700_0000);
                let io = read_start(regions, 0x0400_0000);
                if vram.is_empty() && palette.is_empty() && oam.is_empty() {
                    return None;
                }
                Some(gba::view(self.inspector_view, &vram, &palette, &oam, &io))
            }
            _ => {
                let chr = read_name(regions, "CHR");
                let palette = read_name(regions, "PAL");
                let nametable = read_name(regions, "NT");
                let oam = read_name(regions, "OAM");
                if chr.is_empty() && palette.is_empty() && nametable.is_empty() && oam.is_empty() {
                    return None;
                }
                Some(nes::view(
                    self.inspector_view,
                    &chr,
                    &palette,
                    &nametable,
                    &oam,
                ))
            }
        }
    }

    /// Upload a decoded image into the inspector's texture slot.
    fn upload_inspector(&mut self, image: &InspectImage) -> Option<TextureHandle> {
        let backend = self.backend.clone()?;
        let texture = TextureId::new(INSPECTOR_TEXTURE_BASE);
        let mut backend = backend.borrow_mut();
        let result = if self.inspector_registered {
            backend.update_texture(texture, image.width, image.height, &image.rgba)
        } else {
            backend.register_texture_with_filter(
                texture,
                image.width,
                image.height,
                &image.rgba,
                TextureFilter::Nearest,
            )
        };
        result.ok()?;
        self.inspector_registered = true;
        Some(TextureHandle {
            texture,
            width: image.width,
            height: image.height,
        })
    }
}

/// Copy a region's bytes out of the core.
fn read_region(region: &MemoryRegion) -> Vec<u8> {
    let mut bytes = vec![0u8; region.len];
    region.copy_into(0, &mut bytes);
    bytes
}

/// Read the first region whose emulated start address matches.
fn read_start(regions: &[MemoryRegion], start: usize) -> Vec<u8> {
    regions
        .iter()
        .find(|region| region.start == start)
        .map(read_region)
        .unwrap_or_default()
}

/// Read the first region whose address-space name matches.
fn read_name(regions: &[MemoryRegion], name: &str) -> Vec<u8> {
    regions
        .iter()
        .find(|region| region.addrspace == name)
        .map(read_region)
        .unwrap_or_default()
}

/// Project memory regions into the inspector's view rows.
fn region_rows(regions: &[MemoryRegion]) -> Vec<crate::ui::InspectorRegionRow> {
    regions
        .iter()
        .map(|region| {
            let (start, end) = region.address_range();
            let label = region.label();
            crate::ui::InspectorRegionRow {
                label: if label.is_empty() {
                    "—".to_string()
                } else {
                    label
                },
                range: format!("{start:#06X}–{end:#06X}"),
                size: format_size(region.len),
            }
        })
        .collect()
}

/// A byte count as a short human string.
fn format_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_kilobytes() {
        assert_eq!(format_size(256), "256 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(96 * 1024), "96 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3 MB");
    }
}
