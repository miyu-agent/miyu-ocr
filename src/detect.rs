//! 找字（PP-OCRv5 的 DB 模型）：图缩到模型吃得下的大小、照 `ImageNet` 的均值方差归一，模型交回每个点是字的概率；概率过
//! [`THRESHOLD`] 的点连成一块块，平均概率够 [`BOX_SCORE`] 的一块是一段字，照 DB 的办法往外扩一圈（模型认的是缩进去的
//! 字心），换回原图的坐标。
//!
//! 大图切成一条条横的来找（[`strips`]）：模型占的内存跟送进去的像素数走，一次送一整张长截图要好几百 MB；切成每条不过
//! [`STRIP_PIXELS`] 的，再长的图也只占一条的。相邻两条叠 [`OVERLAP`]，一段字算在它中线落在的那一条里，不重、不漏。
//!
//! 框是横平竖直的：截图、文档、网页这类横排的字够用；斜着拍的、竖排的认不全（README 写明）。

use image::RgbImage;
use image::imageops::{FilterType, resize};
use rten_tensor::NdTensor;

/// 送进模型的图最宽多少：再宽的整张缩小（很宽的全景图这类）。
const MAX_WIDTH: f32 = 2560.0;
/// 最短边最少多少：太小的图放大到这么大再找。
const MIN_SIDE: f32 = 64.0;
/// 一条最多送多少个像素进模型（约占 130 MB）。
const STRIP_PIXELS: u32 = 1_000_000;
/// 一条最矮多少（很宽的图也不切得太碎）。
const MIN_STRIP: u32 = 256;
/// 相邻两条叠多高（模型输入上的像素）：比它一半还高的一行字才可能被切开。
pub const OVERLAP: u32 = 128;
/// 宽高都得是它的倍数（模型下采样 32 倍）。
const STRIDE: f32 = 32.0;
/// 过这个概率的点算字。
pub const THRESHOLD: f32 = 0.3;
/// 一块的平均概率过这个才算一段字。
pub const BOX_SCORE: f32 = 0.6;
/// 往外扩多少：DB 的 unclip，扩出去的距离是 面积 × 它 ÷ 周长。
const UNCLIP: f32 = 1.5;
/// 比这还窄、还矮的块不要（模型输出上的点数）。
const MIN_SIZE: usize = 3;

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// 原图上的一个框。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    #[must_use]
    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    #[must_use]
    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }
}

/// 原图缩多少倍送进模型：太宽的缩到 [`MAX_WIDTH`]，太小的放大到最短边 [`MIN_SIDE`]，别的照原样。
#[must_use]
pub fn scale(width: u32, height: u32) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "图的边长在几万以内")]
    let (w, h) = (width as f32, height as f32);
    let scale = (MAX_WIDTH / w).min(1.0);
    if w.min(h) * scale < MIN_SIDE {
        MIN_SIDE / w.min(h)
    } else {
        scale
    }
}

/// 取到 32 的倍数（至少 32）。
#[must_use]
pub fn stride(length: f32) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "取整以后是几万以内的正数"
    )]
    let rounded = ((length / STRIDE).round().max(1.0) * STRIDE) as u32;
    rounded
}

/// 切出来的一条：都是缩过以后的行号。`top..bottom` 送进模型，中线落在 `own_top..own_bottom` 的字算这一条的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strip {
    pub top: u32,
    pub bottom: u32,
    pub own_top: u32,
    pub own_bottom: u32,
}

/// 缩过以后 `width` × `height` 的图切成几条：一条够装下的不切。
#[must_use]
pub fn strips(width: u32, height: u32) -> Vec<Strip> {
    let tall = (STRIP_PIXELS / width.max(1) / 32 * 32).max(MIN_STRIP);
    if height <= tall {
        return vec![Strip {
            top: 0,
            bottom: height,
            own_top: 0,
            own_bottom: height,
        }];
    }
    let step = tall - OVERLAP;
    let mut found = Vec::new();
    let mut top = 0;
    loop {
        let bottom = (top + tall).min(height);
        let last = bottom == height;
        found.push(Strip {
            top,
            bottom,
            own_top: if top == 0 { 0 } else { top + OVERLAP / 2 },
            own_bottom: if last { height } else { bottom - OVERLAP / 2 },
        });
        if last {
            return found;
        }
        top += step;
    }
}

/// 模型的输入：一块图缩成 `width` × `height`（32 的倍数），`[1, 3, 高, 宽]`，归一过的。
#[must_use]
pub fn input(image: &RgbImage, width: u32, height: u32) -> NdTensor<f32, 4> {
    let small = resize(image, width, height, FilterType::Triangle);
    let mut tensor = NdTensor::zeros([1, 3, height as usize, width as usize]);
    for (x, y, pixel) in small.enumerate_pixels() {
        for channel in 0..3 {
            tensor[[0, channel, y as usize, x as usize]] =
                (f32::from(pixel[channel]) / 255.0 - MEAN[channel]) / STD[channel];
        }
    }
    tensor
}

/// 从概率图（`width` × `height`，一行行排）里找出一段段字，换算到原图（`image_width` × `image_height`）上。
#[must_use]
pub fn boxes(
    probability: &[f32],
    width: usize,
    height: usize,
    image_width: u32,
    image_height: u32,
) -> Vec<Rect> {
    #[expect(clippy::cast_precision_loss, reason = "边长在几万以内")]
    let (scale_x, scale_y) = (
        image_width as f32 / width as f32,
        image_height as f32 / height as f32,
    );
    #[expect(clippy::cast_precision_loss, reason = "边长在几万以内")]
    let (right, bottom) = (image_width as f32, image_height as f32);
    let mut seen = vec![false; width * height];
    let mut found = Vec::new();
    let mut stack = Vec::new();
    for start in 0..probability.len().min(width * height) {
        if seen[start] || probability[start] < THRESHOLD {
            continue;
        }
        // 一块：上下左右相连的点。
        seen[start] = true;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1) = (width, height, 0, 0);
        let (mut count, mut sum) = (0usize, 0f32);
        while let Some(at) = stack.pop() {
            let (x, y) = (at % width, at / width);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            count += 1;
            sum += probability[at];
            let mut visit = |next: usize| {
                if !seen[next] && probability[next] >= THRESHOLD {
                    seen[next] = true;
                    stack.push(next);
                }
            };
            if x > 0 {
                visit(at - 1);
            }
            if x + 1 < width {
                visit(at + 1);
            }
            if y > 0 {
                visit(at - width);
            }
            if y + 1 < height {
                visit(at + width);
            }
        }
        let (box_width, box_height) = (x1 - x0 + 1, y1 - y0 + 1);
        #[expect(clippy::cast_precision_loss, reason = "点数在几千万以内")]
        let mean = sum / count as f32;
        if box_width.min(box_height) < MIN_SIZE || mean < BOX_SCORE {
            continue;
        }
        #[expect(clippy::cast_precision_loss, reason = "边长在几万以内")]
        let (w, h) = (box_width as f32, box_height as f32);
        let grow = w * h * UNCLIP / (2.0 * (w + h));
        #[expect(clippy::cast_precision_loss, reason = "边长在几万以内")]
        let (left, top) = (x0 as f32, y0 as f32);
        found.push(Rect {
            x0: ((left - grow) * scale_x).max(0.0),
            y0: ((top - grow) * scale_y).max(0.0),
            x1: ((left + w + grow) * scale_x).min(right),
            y1: ((top + h + grow) * scale_y).min(bottom),
        });
    }
    found
}

#[cfg(test)]
mod tests;
