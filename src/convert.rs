use yuvutils_rs::{
    BufferStoreMut, YuvChromaSubsampling, YuvConversionMode, YuvPackedImageMut, YuvPlanarImageMut, YuvRange,
    YuvStandardMatrix,
};

use crate::rect::Rect;

/// SIMD-accelerated BGRx-to-YUYV converter with pre-allocated buffers.
pub struct Converter {
    planar: YuvPlanarImageMut<'static, u8>,
    width: u32,
    height: u32,
}

impl Converter {
    pub fn new(width: u32, height: u32) -> Self {
        let planar = YuvPlanarImageMut::<u8>::alloc(width, height, YuvChromaSubsampling::Yuv422);
        Self { planar, width, height }
    }

    /// Convert a BGRx buffer to YUYV using SIMD-accelerated yuvutils-rs.
    /// Two-step path: BGRx -> YUV 422 planar -> YUYV packed.
    pub fn bgrx_to_yuyv(&mut self, src: &[u8], stride: u32, dst: &mut [u8]) {
        // Step 1: BGRA/BGRx -> YUV 422 planar (SIMD)
        // BGRx has identical byte layout to BGRA; the 4th byte is unused padding
        if let Err(e) = yuvutils_rs::bgra_to_yuv422(
            &mut self.planar,
            src,
            stride,
            YuvRange::Limited,
            YuvStandardMatrix::Bt601,
            YuvConversionMode::Balanced,
        ) {
            tracing::error!("bgra_to_yuv422 failed: {:?}", e);
            return;
        }

        // Step 2: YUV 422 planar -> YUYV packed (SIMD)
        let yuyv_stride = self.width * 2;
        let mut packed = YuvPackedImageMut {
            yuy: BufferStoreMut::Borrowed(dst),
            yuy_stride: yuyv_stride,
            width: self.width,
            height: self.height,
        };

        if let Err(e) = yuvutils_rs::yuv422_to_yuyv422(&mut packed, &self.planar.to_fixed()) {
            tracing::error!("yuv422_to_yuyv422 failed: {:?}", e);
        }
    }
}

/// Crop a BGRx buffer to the given rect, clamped to source bounds.
/// Returns the cropped buffer and its (width, height).
pub fn crop_bgrx(src: &[u8], src_width: u32, src_height: u32, src_stride: u32, rect: &Rect) -> (Vec<u8>, u32, u32) {
    // Clamp crop rect to source bounds
    let x0 = (rect.x.max(0) as u32).min(src_width);
    let y0 = (rect.y.max(0) as u32).min(src_height);
    let x1 = ((rect.x.max(0) as u32).saturating_add(rect.width)).min(src_width);
    let y1 = ((rect.y.max(0) as u32).saturating_add(rect.height)).min(src_height);

    let crop_w = x1.saturating_sub(x0);
    let crop_h = y1.saturating_sub(y0);

    if crop_w == 0 || crop_h == 0 {
        return (vec![0u8; 4], 1, 1);
    }

    let dst_stride = crop_w as usize * 4;
    let mut dst = vec![0u8; dst_stride * crop_h as usize];

    for row in 0..crop_h as usize {
        let src_offset = (y0 as usize + row) * src_stride as usize + x0 as usize * 4;
        let dst_offset = row * dst_stride;
        dst[dst_offset..dst_offset + dst_stride].copy_from_slice(&src[src_offset..src_offset + dst_stride]);
    }

    (dst, crop_w, crop_h)
}

/// Nearest-neighbor resize of a BGRx buffer.
pub fn resize_bgrx_nearest(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_stride: u32,
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
) {
    let dst_stride = dst_width as usize * 4;

    for dy in 0..dst_height as usize {
        let sy = (dy * src_height as usize) / dst_height as usize;
        let src_row = &src[sy * src_stride as usize..];
        let dst_row = &mut dst[dy * dst_stride..(dy + 1) * dst_stride];

        for dx in 0..dst_width as usize {
            let sx = (dx * src_width as usize) / dst_width as usize;
            dst_row[dx * 4..dx * 4 + 4].copy_from_slice(&src_row[sx * 4..sx * 4 + 4]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bgrx_to_yuyv_black() {
        // Black pixels: B=0, G=0, R=0, X=0
        let src = [0u8; 4 * 4]; // 4 pixels wide, 1 row
        let mut dst = [0u8; 2 * 4]; // YUYV: 2 bytes/pixel
        let mut converter = Converter::new(4, 1);
        converter.bgrx_to_yuyv(&src, 16, &mut dst);

        // Black in BT.601 limited range: Y=16, U=128, V=128
        assert!((14..=18).contains(&dst[0]), "Y0={}", dst[0]); // Y0
        assert!((126..=130).contains(&dst[1]), "U={}", dst[1]); // U
        assert!((14..=18).contains(&dst[2]), "Y1={}", dst[2]); // Y1
        assert!((126..=130).contains(&dst[3]), "V={}", dst[3]); // V
    }

    #[test]
    fn test_bgrx_to_yuyv_white() {
        // White pixels: B=255, G=255, R=255
        let mut src = [0u8; 4 * 2]; // 2 pixels
        for i in 0..2 {
            src[i * 4] = 255; // B
            src[i * 4 + 1] = 255; // G
            src[i * 4 + 2] = 255; // R
        }
        let mut dst = [0u8; 4]; // 2 pixels in YUYV
        let mut converter = Converter::new(2, 1);
        converter.bgrx_to_yuyv(&src, 8, &mut dst);

        // White: Y should be ~235 (limited range), U~128, V~128
        assert!((230..=240).contains(&dst[0]), "Y0={}", dst[0]);
        assert!((124..=132).contains(&dst[1]), "U={}", dst[1]);
        assert!((230..=240).contains(&dst[2]), "Y1={}", dst[2]);
        assert!((124..=132).contains(&dst[3]), "V={}", dst[3]);
    }

    #[test]
    fn test_bgrx_to_yuyv_dimensions() {
        let width = 8u32;
        let height = 4u32;
        let stride = width * 4;
        let src = vec![128u8; (stride * height) as usize];
        let mut dst = vec![0u8; (width * height * 2) as usize];
        let mut converter = Converter::new(width, height);
        converter.bgrx_to_yuyv(&src, stride, &mut dst);

        // Just verify it doesn't panic and output is non-zero
        assert!(dst.iter().any(|&b| b > 0));
    }

    #[test]
    fn test_resize_identity() {
        let width = 4u32;
        let height = 2u32;
        let stride = width * 4;
        let src: Vec<u8> = (0..stride * height).map(|i| (i % 256) as u8).collect();
        let mut dst = vec![0u8; src.len()];

        resize_bgrx_nearest(&src, width, height, stride, &mut dst, width, height);
        assert_eq!(src, dst);
    }

    #[test]
    fn test_resize_downscale() {
        // 4x2 -> 2x1
        let src = vec![
            // Row 0: 4 pixels
            10, 20, 30, 0, 40, 50, 60, 0, 70, 80, 90, 0, 100, 110, 120, 0, // Row 1: 4 pixels
            11, 21, 31, 0, 41, 51, 61, 0, 71, 81, 91, 0, 101, 111, 121, 0,
        ];
        let mut dst = vec![0u8; 8]; // 2x1 BGRx = 2 pixels * 4 bytes

        resize_bgrx_nearest(&src, 4, 2, 16, &mut dst, 2, 1);

        // Pixel 0: maps to src(0,0) = [10,20,30,0]
        assert_eq!(&dst[0..4], &[10, 20, 30, 0]);
        // Pixel 1: maps to src(2,0) = [70,80,90,0]
        assert_eq!(&dst[4..8], &[70, 80, 90, 0]);
    }

    #[test]
    fn test_crop_bgrx_basic() {
        // 4x2 source, crop center 2x1 at (1,0)
        let src = vec![
            // Row 0: pixels [A, B, C, D]
            10, 10, 10, 0, 20, 20, 20, 0, 30, 30, 30, 0, 40, 40, 40, 0, // Row 1: pixels [E, F, G, H]
            50, 50, 50, 0, 60, 60, 60, 0, 70, 70, 70, 0, 80, 80, 80, 0,
        ];
        let rect = Rect {
            x: 1,
            y: 0,
            width: 2,
            height: 1,
        };
        let (cropped, w, h) = crop_bgrx(&src, 4, 2, 16, &rect);
        assert_eq!(w, 2);
        assert_eq!(h, 1);
        // Should get pixels B and C
        assert_eq!(&cropped[0..4], &[20, 20, 20, 0]);
        assert_eq!(&cropped[4..8], &[30, 30, 30, 0]);
    }

    #[test]
    fn test_crop_bgrx_clamped_to_bounds() {
        // 4x2 source, crop rect extends beyond source
        let src = vec![0u8; 4 * 4 * 2]; // 4x2 BGRx
        let rect = Rect {
            x: 3,
            y: 1,
            width: 10,
            height: 10,
        };
        let (_, w, h) = crop_bgrx(&src, 4, 2, 16, &rect);
        // Clamped: x0=3, y0=1, x1=4, y1=2 -> 1x1
        assert_eq!(w, 1);
        assert_eq!(h, 1);
    }

    #[test]
    fn test_crop_bgrx_zero_size() {
        let src = vec![0u8; 16]; // 1x1 BGRx
        let rect = Rect {
            x: 5,
            y: 5,
            width: 10,
            height: 10,
        };
        let (_, w, h) = crop_bgrx(&src, 1, 1, 4, &rect);
        // Fully out of bounds -> returns 1x1 black
        assert_eq!(w, 1);
        assert_eq!(h, 1);
    }
}
