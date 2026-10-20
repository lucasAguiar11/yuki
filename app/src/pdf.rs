//! PDF pages → BGRA bitmaps through CoreGraphics (no PDF library to ship).

use image::{Frame, RgbaImage};
use objc2_core_foundation::{CFData, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGDataProvider, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGPDFBox, CGPDFDocument, CGPDFPage,
};

/// A rendered page and its width in points (what it's drawn at).
pub struct Page {
    pub frame: Frame,
    pub width: f32,
}

/// The first `max` pages at `scale` pixels per point. Bytes come out BGRA
/// premultiplied, gpui's `RenderImage` layout. ponytail: `/Rotate` ignored,
/// landscape-by-rotation pages show sideways; read `rotation_angle` if it bites.
pub fn pages(bytes: &[u8], scale: f64, max: usize) -> Vec<Page> {
    let data = CFData::from_bytes(bytes);
    let Some(provider) = CGDataProvider::with_cf_data(Some(&data)) else {
        return vec![];
    };
    let Some(doc) = CGPDFDocument::with_provider(Some(&provider)) else {
        return vec![];
    };
    let space = CGColorSpace::new_device_rgb();
    let count = CGPDFDocument::number_of_pages(Some(&doc)).min(max);
    (1..=count)
        .filter_map(|n| {
            let page = CGPDFDocument::page(Some(&doc), n)?;
            let rect = CGPDFPage::box_rect(Some(&page), CGPDFBox::CropBox);
            let w = (rect.size.width * scale).ceil() as usize;
            let h = (rect.size.height * scale).ceil() as usize;
            if w == 0 || h == 0 {
                return None;
            }
            let mut buf = vec![0u8; w * h * 4];
            let info =
                CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
            // SAFETY: `buf` holds w*h*4 bytes and outlives `ctx` (dropped below).
            let ctx = unsafe {
                CGBitmapContextCreate(
                    buf.as_mut_ptr().cast(),
                    w,
                    h,
                    8,
                    w * 4,
                    space.as_deref(),
                    info,
                )
            }?;
            CGContext::set_rgb_fill_color(Some(&ctx), 1.0, 1.0, 1.0, 1.0);
            let full = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64));
            CGContext::fill_rect(Some(&ctx), full);
            CGContext::scale_ctm(Some(&ctx), scale, scale);
            CGContext::translate_ctm(Some(&ctx), -rect.origin.x, -rect.origin.y);
            CGContext::draw_pdf_page(Some(&ctx), Some(&page));
            drop(ctx);
            Some(Page {
                frame: Frame::new(RgbaImage::from_raw(w as u32, h as u32, buf)?),
                width: rect.size.width as f32,
            })
        })
        .collect()
}
