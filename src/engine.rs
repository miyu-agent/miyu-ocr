//! 认一张图：读图（照 EXIF 转正，限大小）、找字、一段段认、排成行。模型每次现读（两个加起来 15 MB，读一次几十毫秒）；扩展
//! 认图时另起子进程跑这一套，认完就退（`main.rs` 开头）。

use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageReader, Limits, RgbImage};
use rten::Model;
use rten_tensor::NdTensor;
use rten_tensor::prelude::*;

use crate::detect;
use crate::layout::{self, Line, Piece};
use crate::model::Models;
use crate::recognize;

/// 图最多多少个像素（解出来约 4 字节一个）：再大的拒掉，免得一张图吃掉几个 GB。
const MAX_IMAGE_PIXELS: u64 = 80_000_000;
/// 图文件最大多少字节。
pub const MAX_FILE_BYTES: u64 = 50 << 20;

/// 加载好的两个模型和字表。
pub struct Engine {
    detector: Model,
    recognizer: Model,
    dictionary: Vec<String>,
}

impl Engine {
    /// 从下好的模型读。
    ///
    /// # Errors
    ///
    /// 文件读不了、不是模型。
    pub fn load(models: &Models) -> Result<Engine, String> {
        let [det, rec, dict] = &crate::model::FILES;
        let load = |name: &str| {
            Model::load_file(models.path(name))
                .map_err(|error| format!("cannot load the model {name}: {error}"))
        };
        let dictionary: Vec<String> = std::fs::read_to_string(models.path(dict.name))
            .map_err(|error| format!("cannot read {}: {error}", dict.name))?
            .lines()
            .map(str::to_string)
            .collect();
        if dictionary.len() != recognize::CLASSES - 2 {
            return Err(format!(
                "{} has {} entries, the recognition model expects {}",
                dict.name,
                dictionary.len(),
                recognize::CLASSES - 2
            ));
        }
        Ok(Engine {
            detector: load(det.name)?,
            recognizer: load(rec.name)?,
            dictionary,
        })
    }

    /// 认一张图，交回一行行。
    ///
    /// # Errors
    ///
    /// 模型跑不了（模型文件坏了这类）。
    pub fn read(&self, image: &RgbImage) -> Result<Vec<Line>, String> {
        let rects = self.find(image)?;
        let mut pieces = Vec::new();
        for rect in rects {
            let Some(crop) = crop(image, rect) else {
                continue;
            };
            let output = self
                .recognizer
                .run_one(recognize::input(&crop).view().into(), None)
                .map_err(|error| format!("text recognition failed: {error}"))?;
            let scores: NdTensor<f32, 3> = output
                .try_into()
                .map_err(|_| "text recognition gave an unexpected output".to_string())?;
            let (text, score) = recognize::decode(&scores, &self.dictionary);
            if !text.is_empty() && score >= recognize::MIN_SCORE {
                pieces.push(Piece { rect, text, score });
            }
        }
        Ok(layout::lines(pieces))
    }
}

impl Engine {
    /// 找字：一条条送进模型（[`detect::strips`]），框换回原图坐标，只留中线落在这一条自己那段里的。
    fn find(&self, image: &RgbImage) -> Result<Vec<detect::Rect>, String> {
        let scale = detect::scale(image.width(), image.height());
        #[expect(clippy::cast_precision_loss, reason = "图的边长在几万以内")]
        let (width, height) = (image.width() as f32, image.height() as f32);
        let input_width = detect::stride(width * scale);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "几万以内的正数"
        )]
        let scaled_height = (height * scale).round() as u32;
        let mut found = Vec::new();
        for strip in detect::strips(input_width, scaled_height) {
            // 这一条在原图上是哪几行。
            #[expect(clippy::cast_precision_loss, reason = "行号在几万以内")]
            let row = |scaled: u32| (scaled as f32 / scale).min(height);
            let (top, bottom) = (row(strip.top), row(strip.bottom));
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "行号在几万以内"
            )]
            let (first, rows) = (top as u32, ((bottom - top).round() as u32).max(1));
            let rows = rows.min(image.height() - first.min(image.height() - 1));
            let part = image::imageops::crop_imm(image, 0, first, image.width(), rows).to_image();
            #[expect(clippy::cast_precision_loss, reason = "行数在几万以内")]
            let input_height = detect::stride(rows as f32 * scale);
            let output = self
                .detector
                .run_one(
                    detect::input(&part, input_width, input_height)
                        .view()
                        .into(),
                    None,
                )
                .map_err(|error| format!("text detection failed: {error}"))?;
            let probability: NdTensor<f32, 4> = output
                .try_into()
                .map_err(|_| "text detection gave an unexpected output".to_string())?;
            let [_, _, map_height, map_width] = probability.shape();
            #[expect(clippy::cast_precision_loss, reason = "行号在几万以内")]
            let (own_top, own_bottom, offset) =
                (row(strip.own_top), row(strip.own_bottom), first as f32);
            for mut rect in detect::boxes(
                &probability.to_vec(),
                map_width,
                map_height,
                part.width(),
                part.height(),
            ) {
                rect.y0 += offset;
                rect.y1 += offset;
                let middle = f32::midpoint(rect.y0, rect.y1);
                if middle >= own_top && middle < own_bottom {
                    found.push(rect);
                }
            }
        }
        Ok(found)
    }
}

/// 裁出一个框；太小的（不到 2 像素）不要。
fn crop(image: &RgbImage, rect: detect::Rect) -> Option<RgbImage> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "框在图里，坐标是几万以内的正数"
    )]
    let (x, y, width, height) = (
        rect.x0.floor() as u32,
        rect.y0.floor() as u32,
        rect.width().ceil() as u32,
        rect.height().ceil() as u32,
    );
    let width = width.min(image.width().saturating_sub(x));
    let height = height.min(image.height().saturating_sub(y));
    (width >= 2 && height >= 2)
        .then(|| image::imageops::crop_imm(image, x, y, width, height).to_image())
}

/// 读一张图：照内容认格式（扩展名不算数），照 EXIF 转正，转成 RGB。
///
/// # Errors
///
/// 读不了、不是认得的图、太大。
pub fn open(path: &Path) -> Result<RgbImage, String> {
    let size = std::fs::metadata(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(format!(
            "the image is larger than {} MiB",
            MAX_FILE_BYTES >> 20
        ));
    }
    let reader = ImageReader::open(path)
        .and_then(ImageReader::with_guessed_format)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let mut decoder = reader.into_decoder().map_err(|_| {
        "not an image this tool reads (PNG, JPEG, WebP, GIF, BMP, TIFF)".to_string()
    })?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(format!("the image is too large ({width}×{height})"));
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_IMAGE_PIXELS * 8);
    decoder
        .set_limits(limits)
        .map_err(|error| format!("the image is too large: {error}"))?;
    let orientation = decoder.orientation().ok();
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("cannot decode the image: {error}"))?;
    if let Some(orientation) = orientation {
        image.apply_orientation(orientation);
    }
    Ok(image.to_rgb8())
}
