#![allow(unsafe_op_in_unsafe_fn)]

use super::shell::{failure, wide};
use image::{
    DynamicImage, Rgba, RgbaImage,
    imageops::{self, FilterType},
};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    mem::size_of,
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::PCWSTR,
};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Spec {
    pub paths: Vec<String>,
    pub composite: bool,
    pub mosaic: bool,
    pub grayscale: bool,
    pub size: u32,
}

impl Spec {
    pub fn key(&self) -> u64 {
        let mut hash = DefaultHasher::new();
        self.hash(&mut hash);
        hash.finish()
    }
}

pub struct Icon(pub usize);

impl Icon {
    pub fn handle(&self) -> HICON {
        HICON(self.0 as *mut _)
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyIcon(self.handle());
        }
    }
}

pub fn brand() -> Result<Icon, String> {
    unsafe {
        let module = GetModuleHandleW(None).map_err(failure)?;
        let icon = LoadIconW(Some(module.into()), PCWSTR(std::ptr::without_provenance(1)))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .map_err(failure)?;
        CopyIcon(icon)
            .map(|icon| Icon(icon.0 as usize))
            .map_err(failure)
    }
}

pub fn cache_path(spec: &Spec, root: &Path) -> PathBuf {
    let mut hash = DefaultHasher::new();
    2u32.hash(&mut hash);
    crate::LOGO_REVISION.hash(&mut hash);
    spec.hash(&mut hash);
    for path in &spec.paths {
        if let Ok(metadata) = std::fs::metadata(path) {
            metadata.len().hash(&mut hash);
            if let Ok(modified) = metadata.modified() {
                modified.hash(&mut hash);
            }
        }
    }
    root.join(format!("{:016x}.ico", hash.finish()))
}

pub fn load(spec: &Spec, root: &Path) -> Result<(Icon, PathBuf), String> {
    let path = cache_path(spec, root);
    if path.exists() {
        if let Ok(icon) = load_file(&path, spec.size) {
            return Ok((icon, path));
        }
    }
    std::fs::create_dir_all(root).map_err(failure)?;
    let size = spec.size.clamp(16, 256);
    let mut canvas = RgbaImage::new(size, size);
    let paths: Vec<_> = spec
        .paths
        .iter()
        .filter(|path| !path.is_empty())
        .take(if spec.composite { 4 } else { 1 })
        .collect();
    if paths.is_empty() {
        canvas = render_icon(brand()?.handle(), size)?;
    } else {
        for (index, source) in paths.iter().enumerate() {
            let cell = if spec.composite && paths.len() > 1 {
                (size - 4) / 2
            } else {
                size
            };
            // Mosaic shrinks the top-right/bottom-left pair: large, small, small, large.
            let tile = if spec.mosaic && cell < size && (index == 1 || index == 2) {
                cell * 2 / 3
            } else {
                cell
            };
            let decoded =
                decode_icon(source, tile).or_else(|_| render_icon(brand()?.handle(), tile))?;
            let (left, top) = mosaic_position(index as u32, cell, tile, size);
            imageops::overlay(&mut canvas, &decoded, left as i64, top as i64);
        }
    }
    if spec.grayscale {
        for pixel in canvas.pixels_mut() {
            let luminance =
                ((pixel[0] as u32 * 54 + pixel[1] as u32 * 183 + pixel[2] as u32 * 19) / 256) as u8;
            *pixel = Rgba([luminance, luminance, luminance, pixel[3]]);
        }
    }
    let temporary = path.with_extension(format!("{}.tmp", appstack::model::new_id()));
    let file = std::fs::File::create(&temporary).map_err(failure)?;
    let mut writer = std::io::BufWriter::new(file);
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut writer, image::ImageFormat::Ico)
        .map_err(failure)?;
    use std::io::Write;
    writer.flush().map_err(failure)?;
    drop(writer);
    if std::fs::rename(&temporary, &path).is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    Ok((load_file(&path, size)?, path))
}

/// Top-left corner of a tile centered in its 2x2 grid cell; a full-size tile fills the canvas.
fn mosaic_position(index: u32, cell: u32, tile: u32, size: u32) -> (u32, u32) {
    if cell == size {
        return (0, 0);
    }
    let center = (cell - tile) / 2;
    (
        (index % 2) * (cell + 4) + center,
        (index / 2) * (cell + 4) + center,
    )
}

fn load_file(path: &Path, size: u32) -> Result<Icon, String> {
    unsafe {
        LoadImageW(
            None,
            PCWSTR(wide(path).as_ptr()),
            IMAGE_ICON,
            size as i32,
            size as i32,
            LR_LOADFROMFILE,
        )
        .map(|handle| Icon(handle.0 as usize))
        .map_err(failure)
    }
}

fn decode_icon(path: &str, size: u32) -> Result<RgbaImage, String> {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if ["png", "jpg", "jpeg", "bmp", "ico"]
        .iter()
        .any(|value| extension.eq_ignore_ascii_case(value))
    {
        let mut reader = image::ImageReader::open(path).map_err(failure)?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(failure)?
            .resize(size, size, FilterType::Lanczos3)
            .to_rgba8();
        let mut canvas = RgbaImage::new(size, size);
        imageops::overlay(
            &mut canvas,
            &image,
            ((size - image.width()) / 2) as i64,
            ((size - image.height()) / 2) as i64,
        );
        return Ok(canvas);
    }
    unsafe {
        let mut info = SHFILEINFOW::default();
        if SHGetFileInfoW(
            PCWSTR(wide(path).as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        ) != 0
            && !info.hIcon.is_invalid()
        {
            let icon = Icon(info.hIcon.0 as usize);
            return render_icon(icon.handle(), size);
        }
        if let Ok(factory) = SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(
            PCWSTR(wide(path).as_ptr()),
            None,
        ) {
            if let Ok(bitmap) = factory.GetImage(
                SIZE {
                    cx: size as i32,
                    cy: size as i32,
                },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            ) {
                let result = bitmap_pixels(bitmap, size);
                let _ = DeleteObject(bitmap.into());
                return result;
            }
        }
    }
    Err("Windows could not extract this icon".into())
}

unsafe fn bitmap_pixels(bitmap: HBITMAP, size: u32) -> Result<RgbaImage, String> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let dc = CreateCompatibleDC(None);
    let mut pixels = vec![0u8; size as usize * size as usize * 4];
    let count = GetDIBits(
        dc,
        bitmap,
        0,
        size,
        Some(pixels.as_mut_ptr().cast()),
        &mut info,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(dc);
    if count == 0 {
        return Err("Could not read icon bitmap".into());
    }
    convert_pixels(pixels, size)
}

fn convert_pixels(mut pixels: Vec<u8>, size: u32) -> Result<RgbaImage, String> {
    let has_alpha = pixels.chunks_exact(4).any(|pixel| pixel[3] != 0);
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        if !has_alpha {
            pixel[3] = if pixel[..3].iter().any(|value| *value != 0) {
                255
            } else {
                0
            };
        } else if pixel[3] != 0 && pixel[3] != 255 {
            let alpha = pixel[3] as u32;
            for value in &mut pixel[..3] {
                *value = (*value as u32 * 255 / alpha).min(255) as u8;
            }
        }
    }
    RgbaImage::from_raw(size, size, pixels).ok_or_else(|| "Invalid icon bitmap".into())
}

pub fn render_icon(icon: HICON, size: u32) -> Result<RgbaImage, String> {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(failure)?;
        let previous = SelectObject(dc, bitmap.into());
        std::ptr::write_bytes(bits, 0, size as usize * size as usize * 4);
        let result = DrawIconEx(dc, 0, 0, icon, size as i32, size as i32, 0, None, DI_NORMAL)
            .map_err(failure);
        let _ = GdiFlush();
        let pixels =
            std::slice::from_raw_parts(bits.cast::<u8>(), size as usize * size as usize * 4)
                .to_vec();
        SelectObject(dc, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        result?;
        convert_pixels(pixels, size)
    }
}

pub fn prune_cache(root: &Path) {
    let Ok(files) = std::fs::read_dir(root) else {
        return;
    };
    let mut files: Vec<_> = files
        .filter_map(Result::ok)
        .filter_map(|file| {
            let metadata = file.metadata().ok()?;
            (metadata.is_file()
                && file
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "ico"))
            .then(|| (metadata.modified().ok(), file.path(), metadata.len()))
        })
        .collect();
    files.sort_by_key(|(modified, _, _)| *modified);
    let mut size: u64 = files.iter().map(|(_, _, size)| size).sum();
    let mut count = files.len();
    for (_, path, length) in files {
        if count <= 1024 && size <= 64 * 1024 * 1024 {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            count -= 1;
            size = size.saturating_sub(length);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mosaic_alternates_large_and_small_tiles_without_overlap() {
        let (size, cell) = (64, 30);
        let small = cell * 2 / 3;
        assert_eq!(mosaic_position(0, cell, cell, size), (0, 0));
        assert_eq!(mosaic_position(3, cell, cell, size), (34, 34));
        // Small tiles stay centered inside their own cells.
        assert_eq!(mosaic_position(1, cell, small, size), (34 + 5, 5));
        assert_eq!(mosaic_position(2, cell, small, size), (5, 34 + 5));
        assert_eq!(mosaic_position(0, size, size, size), (0, 0));
    }
}
