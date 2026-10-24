//! Rotating log file with API keys redacted, plus a local crash log. Nothing is uploaded.
//! Default level is info; transcripts may only be logged at debug (RUST_LOG=yappaflow_lib=debug).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const LOG_FILE: &str = "yappaflow.log";
const MAX_BYTES: u64 = 1024 * 1024;

/// Replaces anything that looks like an API key or bearer token with `[redacted]`.
pub fn redact(s: &str) -> String {
    const PREFIXES: [&str; 6] = ["sk-", "sk_", "gsk_", "xai-", "Bearer ", "Token "];
    let tok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    'outer: while i < s.len() {
        let rest = &s[i..];
        let at_boundary = !s[..i].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric());
        if at_boundary {
            for p in PREFIXES {
                if let Some(after) = rest.strip_prefix(p) {
                    let n = after.find(|c: char| !tok(c)).unwrap_or(after.len());
                    if n >= 8 {
                        out.push_str("[redacted]");
                        i += p.len() + n;
                        continue 'outer;
                    }
                }
            }
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Appends to `dir/yappaflow.log`; at 1 MB it becomes `yappaflow.log.1` (one backup kept).
pub struct RotatingFile {
    path: PathBuf,
    file: Option<File>,
    size: u64,
}

impl RotatingFile {
    pub fn new(dir: &Path) -> Self {
        let path = dir.join(LOG_FILE);
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        let size = file.as_ref().and_then(|f| f.metadata().ok()).map_or(0, |m| m.len());
        Self { path, file, size }
    }

    fn rotate(&mut self) {
        self.file = None;
        let _ = std::fs::rename(&self.path, self.path.with_extension("log.1"));
        self.file = OpenOptions::new().create(true).append(true).open(&self.path).ok();
        self.size = 0;
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let line = redact(&String::from_utf8_lossy(buf));
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        if self.size + line.len() as u64 > MAX_BYTES {
            self.rotate();
        }
        if let Some(f) = self.file.as_mut() {
            // A full disk must never take the app down: drop the line instead.
            if f.write_all(line.as_bytes()).is_ok() {
                self.size += line.len() as u64;
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().map_or(Ok(()), |f| f.flush())
    }
}

/// Call once, first thing in setup.
pub fn init(log_dir: &Path) {
    let _ = std::fs::create_dir_all(log_dir);
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(RotatingFile::new(log_dir))))
        .try_init();
    install_panic_hook(log_dir.join("crash.log"));
}

/// Appends each panic (message, location, backtrace) to `crash.log`, then runs the default hook.
fn install_panic_hook(path: PathBuf) {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let report = redact(&format!(
            "=== crash at unix {secs}, v{} thread '{thread}'\n{info}\n{}\n",
            env!("CARGO_PKG_VERSION"),
            std::backtrace::Backtrace::force_capture()
        ));
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(report.as_bytes());
        }
        log::error!("panic in thread '{thread}': {info}");
        prev(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_keys_and_tokens() {
        assert_eq!(redact("key sk-proj-abcdEFGH1234 ok"), "key [redacted] ok");
        assert_eq!(redact("groq gsk_0123456789abcdef"), "groq [redacted]");
        assert_eq!(redact("Authorization: Bearer abc.def-123456"), "Authorization: [redacted]");
        assert_eq!(redact("Token 0123456789abcdef0123"), "[redacted]");
        assert!(!redact("x=\"sk-1234567890\"").contains("sk-"));
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        for s in ["task-list ready", "ask-me", "sk-short", "Token count 5", "مرحبا sk-", "desk_lamp_on"] {
            assert_eq!(redact(s), s);
        }
    }

    #[test]
    fn rotates_at_limit() {
        let dir = std::env::temp_dir().join(format!("yf-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut f = RotatingFile::new(&dir);
        let line = vec![b'a'; 4096];
        for _ in 0..(MAX_BYTES / 4096 + 2) {
            f.write_all(&line).unwrap();
        }
        assert!(dir.join("yappaflow.log.1").exists());
        assert!(std::fs::metadata(dir.join(LOG_FILE)).unwrap().len() < MAX_BYTES);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
