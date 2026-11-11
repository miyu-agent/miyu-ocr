# OCR文字识别（miyu-ocr）

Miyu 的扩展包：在本机提取图片里的中文、英文。装上就能用，不用另装 Tesseract、Python 这些。

```sh
miyu pkg install miyu-ocr
miyu ocr 截图.png        # 在命令行认一张图
miyu ocr                 # 看模型下好没有
```

AI 用的是工具 `ocr`：给图片的路径（相对工作区或绝对路径），交回一行行字，从上到下。PNG、JPEG、WebP、GIF、BMP、TIFF 都认，
手机照片照 EXIF 转正。`clicall`：AI 在命令里能用 `miyu call ocr` 批量认，脚本拿到 `{"lines": [{"text", "box", "score"}]}`。

## 怎么认的

- 用 [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) 的手机版模型：找字用 PP-OCRv5，认字用 PP-OCRv4。跑模型用纯 Rust 的
  [RTen](https://github.com/robertknight/rten)，程序不带 C 库，三个系统一样编，一个约 8 MB。
- 模型不随包发：第一次用时从 [oar-ocr 的 Release](https://github.com/GreatV/oar-ocr/releases/tag/v0.3.0) 下（约 16 MB），照钉死的
  SHA-256 校验过才用，放在这个包的缓存目录里。程序起来就在后台下，第一次调用不用等。走系统的代理设置。
- 不认的时候只占约 8 MB：每次认图另起一个子进程，模型现读（几十毫秒），认完子进程就退，用过的内存全还给系统。
- 大图切成一条条横的来找字，再长的截图也只占一条的内存。

在一台 12 核的 Linux 上量的：

| 图 | 用时 | 内存峰值 |
|---|---|---|
| 一小段截图（900×260，4 行） | 0.1 秒 | 约 70 MB |
| 整屏截图（1920×1080，20 行中英混排） | 1.1 秒 | 约 180 MB |
| 很长的手机截图（1080×6000，60 条消息） | 2.5 秒 | 约 195 MB |

## 认得准不准

测过的截图、文档、网页、深色模式、衬线字、12 像素的小字，存成 PNG 和 JPEG（质量 50 到 85）都认得全或者只差个别标点、空格。认不好的：

- 斜着拍的、竖排的字：找字的框是横平竖直的；
- 手写的；
- 日文假名、韩文：认字的模型只有中文、英文、数字和常用符号。

### 为什么认字用 v4

PP-OCRv5 的手机版认字模型遇到 JPEG 压缩过的图会整字漏掉：一张整屏截图存成 JPEG（质量 80）以后，「一」「行」「用」「文」这类常用字
大片丢，只认对 18%；同一张存成 PNG 就全对。换了另一份转换（RapidOCR 的）、换 ONNX Runtime 跑都一样，是模型本身的毛病。PP-OCRv4 的
手机版在同一批图上 PNG、JPEG 都认得全，还小 6 MB。聊天里传的图多半是 JPEG，所以用 v4。

## 开发

```sh
cargo test --workspace
cargo xtask licenses     # 第三方许可证合不合、THIRD-PARTY-LICENSES.txt 是不是最新的
cargo xtask notices      # 升级依赖以后重新生成清单
```

打标签 `v<版本>` 发版：三种系统各打一个 `.miyu`（`.github/workflows/release.yml`）。

## 许可证

AGPL-3.0-or-later。第三方依赖和模型的许可证见 `THIRD-PARTY-LICENSES.txt`；模型是 Apache-2.0（PaddlePaddle Authors）。
