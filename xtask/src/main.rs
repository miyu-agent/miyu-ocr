//! 仓库的检查（照 miyu-voice 的 xtask 搬过来，许可证那两项）：
//!
//! - `cargo xtask licenses`：发布的三个平台上用得到的第三方依赖，许可证都能和 AGPL-3.0-or-later 合在一起发（[`licenses`]），
//!   第三方许可证清单 `THIRD-PARTY-LICENSES.txt` 是最新的（[`notices`]）。CI 跑它。
//! - `cargo xtask notices`：重新生成那份清单，升级依赖以后跑。

mod licenses;
mod notices;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    match std::env::args().nth(1).as_deref() {
        Some("licenses") => {
            let mut problems = licenses::check(&root(), &cargo);
            problems.extend(notices::check(&root(), &cargo));
            if problems.is_empty() {
                println!("许可证：都合得到一起，{} 是最新的", notices::PATH);
                ExitCode::SUCCESS
            } else {
                for problem in problems {
                    eprintln!("{problem}");
                }
                ExitCode::FAILURE
            }
        }
        Some("notices") => match notices::write(&root(), &cargo) {
            Ok(()) => {
                println!("写好了 {}", notices::PATH);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("用法：cargo xtask licenses | cargo xtask notices");
            ExitCode::from(2)
        }
    }
}

/// 仓库的根目录。
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}
