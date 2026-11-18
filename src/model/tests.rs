use std::io::{self, Read};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::{FILES, Models, install, total_size};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("miyu-ocr-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// SHA-256 of "hello".
const HELLO: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

#[test]
fn a_matching_download_is_kept() {
    let dir = scratch("match");
    let path = dir.join("model.onnx");
    install(&b"hello"[..], &path, HELLO).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"hello");
    assert!(!dir.join("model.partial").exists());
}

#[test]
fn a_mismatched_download_is_thrown_away() {
    let dir = scratch("mismatch");
    let path = dir.join("model.onnx");
    let why = install(&b"hellp"[..], &path, HELLO).unwrap_err();
    assert!(why.contains("SHA-256"), "{why}");
    assert!(!path.exists());
    assert!(!dir.join("model.partial").exists());
}

#[test]
fn ready_only_when_every_file_has_its_size() {
    let dir = scratch("ready");
    let models = Models::in_cache(&dir);
    assert!(!models.ready());
    std::fs::create_dir_all(models.path("")).unwrap();
    for file in &FILES {
        std::fs::write(
            models.path(file.name),
            vec![0u8; usize::try_from(file.size).unwrap()],
        )
        .unwrap();
    }
    assert!(models.ready());
    std::fs::write(models.path(FILES[1].name), b"cut short").unwrap();
    assert!(!models.ready());
    assert_eq!(total_size(), 15_723_294);
}

/// 照给的函数一次次交字节的「下载」。
struct Scripted<F: FnMut(&mut [u8]) -> io::Result<usize>>(F);

impl<F: FnMut(&mut [u8]) -> io::Result<usize>> Read for Scripted<F> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        (self.0)(buffer)
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    super::hex(&Sha256::digest(bytes))
}

#[test]
fn audit_133_two_installs_of_one_file_do_not_truncate_each_other() {
    // 审计 #133：临时文件名固定（`<名>.partial`），`File::create` 会截断别人正写着的那份；`DOWNLOADING` 只锁本进程，
    // CLI 和扩展服务各自下载时会撞上。这里照这个先后：甲写完一块，乙开同名临时文件（截断成 0 字节）后断线；
    // 甲读完、哈希（算的是流）对上、改名——装上去的是被截断的空文件。修好后乙应写自己的临时文件，装上的就是甲校验过的。
    let dir = scratch("audit133-race");
    let path = dir.join("model.onnx");
    let data: Vec<u8> = (0..10 * 1024_u32)
        .map(|at| u8::try_from(at % 251).unwrap_or(0))
        .collect();
    let expected = sha256_hex(&data);
    let (start_b, b_may_start) = mpsc::channel::<()>();
    let (b_created, a_may_finish) = mpsc::channel::<()>();
    let (a_done, b_may_fail) = mpsc::channel::<()>();
    let wait = Duration::from_secs(10);

    let b_path = path.clone();
    let b_expected = expected.clone();
    let b = std::thread::spawn(move || {
        b_may_start.recv_timeout(wait).expect("甲叫乙开始");
        let body = Scripted(move |_: &mut [u8]| {
            let _ = b_created.send(());
            let _ = b_may_fail.recv_timeout(wait);
            Err(io::Error::other("connection dropped"))
        });
        install(body, &b_path, &b_expected)
    });

    let mut step = 0;
    let a_data = data.clone();
    let body = Scripted(move |buffer: &mut [u8]| {
        step += 1;
        match step {
            1 => {
                buffer[..a_data.len()].copy_from_slice(&a_data);
                Ok(a_data.len())
            }
            2 => {
                start_b.send(()).expect("乙在等");
                // 乙开了它的临时文件才往下走；修好以后乙的文件名不同，照样会走到这里。
                a_may_finish.recv_timeout(wait).expect("乙开了临时文件");
                Ok(0)
            }
            _ => Ok(0),
        }
    });
    let a = install(body, &path, &expected);
    let _ = a_done.send(());
    let b = b.join().expect("乙没崩");
    assert!(b.is_err(), "乙断线了，应失败");
    let installed = std::fs::read(&path).unwrap_or_default();
    assert!(
        a.is_ok() && sha256_hex(&installed) == expected,
        "甲报 {a:?}，装上的文件 {} 字节、SHA-256 {}，应是校验过的 {} 字节",
        installed.len(),
        sha256_hex(&installed),
        data.len()
    );
}

/// 收了连接、读了请求就一直不回的假代理，交回端口和收到的连接数。
fn hanging_proxy() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("能占本机端口");
    let port = listener.local_addr().expect("有端口").port();
    let seen = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&seen);
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming().flatten() {
            counter.fetch_add(1, Ordering::SeqCst);
            held.push(stream);
        }
    });
    (port, seen)
}

#[test]
fn audit_133_a_download_that_never_answers_gives_up_in_time() {
    // 审计 #133：`ureq::get(...).call()` 没配超时（ureq 3 默认全是 None）；代理或服务器收了请求不回时 `ensure()` 永远
    // 不返回，还一直攥着 `DOWNLOADING`，之后每次 `ocr` 调用都卡在锁上。修好以后应在有限时间内报错。
    // 不连外网：子进程里所有代理变量指向 127.0.0.1 上的假代理；父进程最多等 LIMIT，到了就杀掉子进程。
    const LIMIT: Duration = Duration::from_secs(90);
    if let Some(cache) = std::env::var_os("MIYU_OCR_AUDIT_133_CHILD") {
        let models = Models::in_cache(std::path::Path::new(&cache));
        let got = models.ensure();
        assert!(got.is_err(), "假代理不回，不该下载成功");
        println!("AUDIT133 ensure returned {got:?}");
        return;
    }
    let (port, seen) = hanging_proxy();
    let proxy = format!("http://127.0.0.1:{port}");
    let cache = scratch("audit133-hang");
    let exe = std::env::current_exe().expect("找得到测试程序");
    let mut child = std::process::Command::new(exe)
        .args([
            "audit_133_a_download_that_never_answers",
            "--test-threads=1",
            "--nocapture",
        ])
        .env("MIYU_OCR_AUDIT_133_CHILD", &cache)
        .env("HTTPS_PROXY", &proxy)
        .env("https_proxy", &proxy)
        .env("HTTP_PROXY", &proxy)
        .env("http_proxy", &proxy)
        .env("ALL_PROXY", &proxy)
        .env("all_proxy", &proxy)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("起得来子进程");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("等得了子进程") {
            break Some(status);
        }
        if started.elapsed() > LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    assert!(
        seen.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "请求要真的发到假代理上（不然测的不是这条）：{out}"
    );
    let status = status.unwrap_or_else(|| {
        panic!("ensure() 等了 {LIMIT:?} 还没返回：下载没有超时，DOWNLOADING 一直攥着")
    });
    assert!(status.success(), "子进程失败：{status:?}\n{out}");
    assert!(out.contains("1 passed"), "子进程要真跑到那条测试：{out}");
}
