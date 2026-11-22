<p align="center">
  <a href="README.md">English</a> | 简体中文
</p>

# OCR 文字识别（ocr）

> <sub>本文档由 Anthropic 的 Claude Fable 5.2 生成。</sub>

这个仓库是 Miyu 的扩展包，可以在本机提取图片里的中文和英文。安装以后就能使用，不需要另外安装 Tesseract、Python 这些软件。

安装和使用的命令如下：

```sh
miyu pkg install ocr
miyu ocr 截图.png        # 在命令行识别一张图片
miyu ocr                 # 查看模型是否已经下载完成
```

AI 使用的是工具 `ocr`：传入图片的路径（相对于工作区的路径或者绝对路径），工具会从上到下返回一行一行的文字。PNG、JPEG、WebP、GIF、BMP、TIFF 都能识别，手机拍的照片会按照 EXIF 信息转正。`clicall`：AI 在命令里可以使用 `miyu call ocr` 批量识别，脚本会得到 `{"lines": [{"text", "box", "score"}]}`。

## 识别原理

这一节说明这个软件包使用的模型、下载模型的方式和占用的资源。

- 使用 [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) 的手机版模型：检测文字位置使用 PP-OCRv5，识别文字使用 PP-OCRv4。运行模型使用纯 Rust 编写的 [RTen](https://github.com/robertknight/rten)，因此程序不带 C 库，三个系统使用相同的方式编译，每个程序约 8 MB。
- 模型不随软件包发布：第一次使用时从 [oar-ocr 的发行版](https://github.com/GreatV/oar-ocr/releases/tag/v0.3.0) 下载（约 16 MB），按照固定的 SHA-256 校验通过以后才使用，存放在这个软件包的缓存目录里。程序启动后就会在后台下载，所以第一次调用不需要等待。下载使用系统的代理设置。如果连接不上、等不到回应，或者下载过程中 30 秒没有收到新数据，就会报错，下一次调用时重新下载；如果网速慢但一直在下载，则不限制时长。
- 没有识别任务的时候只占用约 8 MB 内存：每次识别图片都会另外启动一个子进程，当场读取模型（几十毫秒），识别完成后子进程就会退出，用过的内存全部还给系统。
- 大图会切成一条一条的横条来检测文字，所以再长的截图也只占用一条横条的内存。

以下数据是在一台 12 核的 Linux 电脑上测量的：

| 图片 | 用时 | 内存峰值 |
|---|---|---|
| 一小段截图（900×260，4 行） | 0.1 秒 | 约 70 MB |
| 整屏截图（1920×1080，20 行中英文混排） | 1.1 秒 | 约 180 MB |
| 很长的手机截图（1080×6000，60 条消息） | 2.5 秒 | 约 195 MB |

## 识别准确率

测试过的截图、文档、网页、深色模式、衬线字体、12 像素的小字，无论存成 PNG 还是 JPEG（质量 50 到 85），都能完整识别，或者只差个别标点、空格。识别效果不好的情况有：

- 斜着拍摄的文字、竖排的文字：因为检测文字的框是横平竖直的；
- 手写的文字；
- 日文假名、韩文：因为识别文字的模型只支持中文、英文、数字和常用符号。

### 为什么识别文字使用 v4

PP-OCRv5 的手机版识别模型遇到经过 JPEG 压缩的图片时，会把整个字漏掉：一张整屏截图存成 JPEG（质量 80）以后，「一」「行」「用」「文」这类常用字大片丢失，只识别对 18%；同一张图片存成 PNG 就全部正确。换用另一份转换（RapidOCR 的转换）或者换用 ONNX Runtime 运行，结果都一样，说明这是模型本身的问题。PP-OCRv4 的手机版在同一批图片上，无论 PNG 还是 JPEG 都能完整识别，而且小 6 MB。由于聊天里传的图片大多是 JPEG，所以使用 v4。

## 开发

开发时可以使用以下命令：

```sh
cargo test --workspace
cargo xtask licenses     # 检查第三方许可证是否兼容、THIRD-PARTY-LICENSES.txt 是否是最新的
cargo xtask notices      # 升级依赖以后重新生成清单
```

发布新版本时打标签 `v<版本>`，`.github/workflows/release.yml` 会为三种系统各打包一个 `.miyu` 文件。

## 许可证

这个软件包的许可证是 AGPL-3.0-or-later。第三方依赖和模型的许可证见 `THIRD-PARTY-LICENSES.txt`；模型的许可证是 Apache-2.0（PaddlePaddle Authors）。
