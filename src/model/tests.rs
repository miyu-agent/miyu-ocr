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
