//! miyu-ocr：Miyu 的 OCR 文字识别。登记一件工具 `ocr`，在本机认图片里的中文、英文（PaddleOCR 的手机版模型），跑模型用纯
//! Rust 的 RTen，不带 C 库。模型不随包发，第一次用时下（约 16 MB，[`model`]）。
//!
//! - `miyu-ocr serve`：核心拉起的扩展。起来就在后台把没下的模型下好，工具被调到时还没下完的等它下完。每次认图另起一个
//!   `miyu-ocr read-json <图>` 子进程来认，认完它就退：模型和认图时用的一两百 MB 都还给系统，常驻的只有几 MB（在同一个
//!   进程里认，放掉的内存分配器留着不还，常驻会停在一百多 MB）。
//! - `miyu ocr <图>`：在命令行认一张图，印出字；不带图的印模型下好没有、在哪。

mod detect;
mod engine;
mod layout;
mod model;
mod recognize;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use miyu_ext::{Call, Extension, Reply, Tool};
use serde_json::{Value, json};

use crate::engine::Engine;
use crate::layout::Line;
use crate::model::Models;

const DESCRIPTION: &str = "Read the Chinese and English text in an image with OCR on this computer (PaddleOCR). Returns the text line by line, top to bottom. Printed horizontal text reads best; handwriting, rotated or vertical text and other scripts may be missed.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cache) = std::env::var_os("MIYU_CACHE_DIR").map(PathBuf::from) else {
        eprintln!("miyu-ocr: MIYU_CACHE_DIR is not set (run it through Miyu: miyu ocr <image>)");
        return ExitCode::FAILURE;
    };
    let models = Arc::new(Models::in_cache(&cache));
    match args.first().map(String::as_str) {
        Some("serve") => {
            serve(models);
            ExitCode::SUCCESS
        }
        Some("read-json") => {
            let path = args.get(1).map(PathBuf::from).unwrap_or_default();
            let answer = match read_file(&models, &path) {
                Ok(lines) => json!({"lines": lines_json(&lines)}),
                Err(why) => json!({"error": why}),
            };
            println!("{answer}");
            ExitCode::SUCCESS
        }
        Some("-h" | "--help") => {
            println!(
                "usage: miyu ocr <image>    print the text in an image\n       miyu ocr            whether the model is downloaded"
            );
            ExitCode::SUCCESS
        }
        Some(path) => match read_file(&models, Path::new(path)) {
            Ok(lines) => {
                println!("{}", text(&lines));
                ExitCode::SUCCESS
            }
            Err(why) => {
                eprintln!("miyu ocr: {why}");
                ExitCode::FAILURE
            }
        },
        None => {
            if models.ready() {
                println!("model: ready ({})", models.path("").display());
            } else {
                println!(
                    "model: not downloaded yet ({} MB, downloaded the first time it is used)",
                    (model::total_size() + 500_000) / 1_000_000
                );
            }
            ExitCode::SUCCESS
        }
    }
}

fn serve(models: Arc<Models>) {
    let mut ext = Extension::new("miyu-ocr", env!("CARGO_PKG_VERSION"));
    let for_tool = Arc::clone(&models);
    ext.tool(
        Tool::new("ocr", DESCRIPTION)
            .schema(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "The image file, relative to the workspace or absolute. PNG, JPEG, WebP, GIF, BMP or TIFF."}
                },
                "required": ["path"]
            }))
            .access("read")
            .paths(json!([{"parameter": "path"}]))
            .display_name(json!({"en": "OCR", "zh": "OCR文字识别"}))
            .subject("path")
            .timeout_ms(180_000)
            .clicall(true),
        move |args, call| call_tool(&for_tool, args, call),
    );
    // 起来就把模型下好，第一次调用不用等下载。
    ext.ready(move |_| {
        if !models.ready() {
            eprintln!(
                "miyu-ocr: downloading the OCR model ({} MB)",
                (model::total_size() + 500_000) / 1_000_000
            );
            if let Err(why) = models.ensure() {
                eprintln!("miyu-ocr: {why}");
            }
        }
    });
    ext.run();
}

fn call_tool(models: &Models, args: &Value, call: &Call) -> Reply {
    // 核心判过的真实位置；老的核心不给的，照工作区自己拼。
    let path = if let Some(checked) = call.checked().first() {
        PathBuf::from(checked)
    } else {
        Path::new(call.cwd()).join(args["path"].as_str().unwrap_or_default())
    };
    // 模型在这里下（两个调用同时来也只下一次），子进程只读。
    if let Err(why) = models.ensure() {
        return Reply::failed(why);
    }
    match read_in_child(&path) {
        Ok(lines) => {
            let texts: Vec<&str> = lines
                .iter()
                .filter_map(|line| line["text"].as_str())
                .collect();
            let shown = if texts.is_empty() {
                "No text found in the image.".to_string()
            } else {
                texts.join("\n")
            };
            Reply::text(shown).data(json!({"lines": lines}))
        }
        Err(why) => Reply::failed(why),
    }
}

/// 另起一个子进程认（见开头）：交回 `lines`。
fn read_in_child(path: &Path) -> Result<Vec<Value>, String> {
    let program =
        std::env::current_exe().map_err(|error| format!("cannot find myself: {error}"))?;
    let output = std::process::Command::new(program)
        .arg("read-json")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("cannot start the reader: {error}"))?;
    let answer: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        let why = String::from_utf8_lossy(&output.stderr);
        format!("the reader stopped ({}): {}", output.status, why.trim())
    })?;
    match answer["error"].as_str() {
        Some(why) => Err(why.to_string()),
        None => Ok(answer["lines"].as_array().cloned().unwrap_or_default()),
    }
}

/// 交给脚本的一行行：字、框（原图像素，左上右下）、分数。
fn lines_json(lines: &[Line]) -> Vec<Value> {
    lines
        .iter()
        .map(|line| {
            json!({
                "text": line.text,
                "box": [line.rect.x0.round(), line.rect.y0.round(), line.rect.x1.round(), line.rect.y1.round()],
                "score": (line.score * 100.0).round() / 100.0,
            })
        })
        .collect()
}

/// 认一个图文件：模型没下的先下（子进程里调到时已经下好了）。
fn read_file(models: &Models, path: &Path) -> Result<Vec<Line>, String> {
    if !path.is_file() {
        return Err(format!("no such file: {}", path.display()));
    }
    let image = engine::open(path)?;
    models.ensure()?;
    Engine::load(models)?.read(&image)
}

fn text(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
