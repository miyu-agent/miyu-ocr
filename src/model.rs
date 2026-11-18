//! 模型文件：PaddleOCR 的两个手机版模型和字表，转好的 ONNX 放在 oar-ocr 的 Release 里（Apache-2.0）。找字用 PP-OCRv5，
//! 认字用 PP-OCRv4：v5 的认字模型遇到 JPEG 压缩的图会整字漏掉（「一」「行」「至」这类常用字，同一张图存成 PNG 就认得全，
//! 两份不同的转换一样），v4 的在测的 PNG、JPEG 上都认得全，还小 6 MB（README「为什么认字用 v4」）。
//! 不随包发：第一次用时下到这个包的缓存目录（`MIYU_CACHE_DIR`），照钉死的 SHA-256 校验过才用。每次下载写自己的临时文件
//! （名字带进程号，CLI 和扩展服务同时下也不撞）、一边算哈希，对得上才改名成正式的；下到一半断了的下次从头来。走系统的
//! 代理设置（`HTTPS_PROXY` 这类）。连不上、等不到回应、下着下着 [`STALL`] 没有新数据就报错；慢但一直在动的不限总时长。

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// 三份文件从哪下。
const BASE: &str = "https://github.com/GreatV/oar-ocr/releases/download/v0.3.0";

/// 一份模型文件：文件名、SHA-256、字节数。
pub struct ModelFile {
    pub name: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

/// 找字（DB）、认字（CTC）、字表。
pub const FILES: [ModelFile; 3] = [
    ModelFile {
        name: "pp-ocrv5_mobile_det.onnx",
        sha256: "1eb7b4f7ab657ebd1c66d5f79bca7497f29768a2e3c15e52daecbba1a8e4a039",
        size: 4_826_518,
    },
    ModelFile {
        name: "pp-ocrv4_mobile_rec.onnx",
        sha256: "5d54b59bac0f49d4561f0462630d8a6822b5b495db064f58096fd3d2392fbc4e",
        size: 10_870_526,
    },
    ModelFile {
        name: "ppocr_keys_v1.txt",
        sha256: "a1c84d9bdb9ab29043c58896224d32941783eb821629618416dcb08f12886492",
        size: 26_250,
    },
];

/// 三份一共多少字节（说给人听要下多大）。
#[must_use]
pub fn total_size() -> u64 {
    FILES.iter().map(|file| file.size).sum()
}

/// 两个调用同时发现没下好时只下一次。
static DOWNLOADING: Mutex<()> = Mutex::new(());

/// 查域名、连上（含代理和 TLS 握手）、发出请求、等到回应头，每一步最多等这么久；下载中这么久没有新数据也算断了。
const STALL: Duration = Duration::from_secs(30);

/// 下载用的连接：每一步都限时（[`STALL`]），不设总时长。
fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_resolve(Some(STALL))
        .timeout_connect(Some(STALL))
        .timeout_send_request(Some(STALL))
        .timeout_recv_response(Some(STALL))
        .timeout_per_read(Some(STALL))
        .timeout_per_write(Some(STALL))
        .build();
    ureq::Agent::new_with_config(config)
}

/// 本进程里第几次装（临时文件名用）。
static INSTALLS: AtomicU64 = AtomicU64::new(0);

/// 放在 `dir` 里的模型。
pub struct Models {
    dir: PathBuf,
}

impl Models {
    /// 放在缓存目录 `cache` 下的 `paddleocr/`。
    #[must_use]
    pub fn in_cache(cache: &Path) -> Models {
        Models {
            dir: cache.join("paddleocr"),
        }
    }

    /// 某一份的路径。
    #[must_use]
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// 三份都在、大小对得上（哈希下载时验过，这里不再每次全读一遍）。
    #[must_use]
    pub fn ready(&self) -> bool {
        FILES.iter().all(|file| {
            fs::metadata(self.path(file.name)).is_ok_and(|meta| meta.len() == file.size)
        })
    }

    /// 没下好的下下来。
    ///
    /// # Errors
    ///
    /// 连不上、写不了、下来的哈希对不上。
    pub fn ensure(&self) -> Result<(), String> {
        let _one = DOWNLOADING.lock().unwrap_or_else(PoisonError::into_inner);
        fs::create_dir_all(&self.dir)
            .map_err(|error| format!("cannot create {}: {error}", self.dir.display()))?;
        let agent = agent();
        for file in &FILES {
            let path = self.path(file.name);
            if fs::metadata(&path).is_ok_and(|meta| meta.len() == file.size) {
                continue;
            }
            let url = format!("{BASE}/{}", file.name);
            let body = agent
                .get(&url)
                .call()
                .map_err(|error| format!("could not download {url}: {error}"))?
                .into_body()
                .into_reader();
            install(body, &path, file.sha256).map_err(|error| format!("{}: {error}", file.name))?;
        }
        Ok(())
    }
}

/// 这一次下载自己的临时文件：`path` 旁边的 `<名>.<进程号>-<第几次>-<纳秒>.partial`，两份同时装不撞名。
fn partial_path(path: &Path) -> PathBuf {
    let count = INSTALLS.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}-{count}-{nanos}.partial", std::process::id()));
    path.with_file_name(name)
}

/// 临时文件：没改名成正式的就删掉（出错、哈希不对、panic 都算）。
struct Partial(PathBuf);

impl Drop for Partial {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// 把 `body` 写进 `path` 旁边这一次自己的临时文件、一边算 SHA-256，对得上 `expected` 才改名成 `path`。
///
/// # Errors
///
/// 读写出错；哈希对不上（临时文件删掉）。
pub fn install(mut body: impl Read, path: &Path, expected: &str) -> Result<(), String> {
    let partial = Partial(partial_path(path));
    let got = (|| -> io::Result<String> {
        let file = File::options()
            .write(true)
            .create_new(true)
            .open(&partial.0)?;
        let mut out = BufWriter::new(file);
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = body.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            out.write_all(&buffer[..read])?;
        }
        out.into_inner()
            .map_err(io::IntoInnerError::into_error)?
            .sync_all()?;
        Ok(hex(&hasher.finalize()))
    })()
    .map_err(|error| error.to_string())?;
    if got != expected {
        return Err(format!(
            "the download's SHA-256 is {got}, expected {expected}"
        ));
    }
    fs::rename(&partial.0, path).map_err(|error| error.to_string())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests;
