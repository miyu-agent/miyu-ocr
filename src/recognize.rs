//! 认字（PP-OCRv4 的 CTC 模型）：一段字的框裁出来、缩到 48 高（宽照比例），归一到 −1～1；模型交回每一步各个字的
//! 概率，取每一步最像的那个，去掉空白（第 0 个）和连着重复的，照字表换成字。一段的分数是留下的那几步概率的平均。

use image::RgbImage;
use image::imageops::{FilterType, resize};
use rten_tensor::NdTensor;
use rten_tensor::prelude::*;

/// 模型要的高。
pub const HEIGHT: u32 = 48;
/// 最宽多少（特别长的一行压一压，不然一步太多）。
const MAX_WIDTH: u32 = 4000;
/// 模型分多少类：空白、字表里的 6623 个、空格。
pub const CLASSES: usize = 6625;
/// 分数低于它的一段不要（多半是图案、噪点被当成了字）。
pub const MIN_SCORE: f32 = 0.5;

/// 一段字裁出来以后模型的输入：`[1, 3, 48, 宽]`。
#[must_use]
pub fn input(crop: &RgbImage) -> NdTensor<f32, 4> {
    let (width, height) = crop.dimensions();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "宽在几千以内"
    )]
    let scaled = ((HEIGHT as f32 * width as f32 / height.max(1) as f32).ceil() as u32)
        .clamp(HEIGHT / 4, MAX_WIDTH);
    let small = resize(crop, scaled, HEIGHT, FilterType::Triangle);
    let mut tensor = NdTensor::zeros([1, 3, HEIGHT as usize, scaled as usize]);
    for (x, y, pixel) in small.enumerate_pixels() {
        for channel in 0..3 {
            tensor[[0, channel, y as usize, x as usize]] =
                (f32::from(pixel[channel]) / 255.0 - 0.5) / 0.5;
        }
    }
    tensor
}

/// 照字表解模型的输出（`[1, 步数, 字数]`）：交回认出的字和分数。字表里第 `i` 个字是第 `i + 1` 类，最后一类是空格。
#[must_use]
pub fn decode(output: &NdTensor<f32, 3>, dictionary: &[String]) -> (String, f32) {
    let [_, steps, classes] = output.shape();
    let mut text = String::new();
    let (mut previous, mut sum, mut kept) = (0usize, 0f32, 0usize);
    let mut row = vec![0f32; classes];
    for step in 0..steps {
        for (class, slot) in row.iter_mut().enumerate() {
            *slot = output[[0, step, class]];
        }
        let (best, probability) = best(&row);
        if best != 0 && best != previous {
            let character = if best == classes - 1 {
                Some(" ")
            } else {
                dictionary.get(best - 1).map(String::as_str)
            };
            if let Some(character) = character {
                text.push_str(character);
                sum += probability;
                kept += 1;
            }
        }
        previous = best;
    }
    #[expect(clippy::cast_precision_loss, reason = "步数在几千以内")]
    let score = if kept == 0 { 0.0 } else { sum / kept as f32 };
    (text.trim().to_string(), score)
}

/// 一步里最像的那一类和它的概率。模型的输出已经是概率（导出时带了 softmax）；不是的（有负数、大于 1）这里补算。
fn best(row: &[f32]) -> (usize, f32) {
    let (mut best, mut value) = (0usize, f32::MIN);
    for (class, &v) in row.iter().enumerate() {
        if v > value {
            (best, value) = (class, v);
        }
    }
    let probabilities = row.iter().all(|v| (0.0..=1.0).contains(v));
    if probabilities {
        return (best, value);
    }
    let total: f32 = row.iter().map(|v| (v - value).exp()).sum();
    (best, 1.0 / total)
}

#[cfg(test)]
mod tests;
