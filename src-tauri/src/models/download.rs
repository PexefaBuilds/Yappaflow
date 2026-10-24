//! Resumable, verified model downloads.
//! `<file>.part` holds the bytes so far and `<file>.part.json` the url, expected size and ETag,
//! so a download survives pause, network loss and app restart (HTTP Range + If-Range).
//! sha256 is computed while streaming; the file is renamed into place only on a match,
//! and deleted on a mismatch.

use super::Model;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

pub const RUN: u8 = 0;
pub const PAUSE: u8 = 1;
pub const CANCEL: u8 = 2;

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Done,
    Paused,
    Cancelled,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct PartMeta {
    url: String,
    size: u64,
    etag: Option<String>,
    /// Paused by the user: not resumed automatically at startup.
    #[serde(default)]
    paused: bool,
}

fn with_ext(dest: &Path, ext: &str) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

fn read_meta(dest: &Path) -> Option<PartMeta> {
    serde_json::from_slice(&std::fs::read(with_ext(dest, ".part.json")).ok()?).ok()
}

fn set_paused(dest: &Path, paused: bool) {
    if let Some(mut m) = read_meta(dest).filter(|m| m.paused != paused) {
        m.paused = paused;
        if let Ok(b) = serde_json::to_vec(&m) {
            let _ = std::fs::write(with_ext(dest, ".part.json"), b);
        }
    }
}

fn clear(dest: &Path) {
    let _ = std::fs::remove_file(with_ext(dest, ".part"));
    let _ = std::fs::remove_file(with_ext(dest, ".part.json"));
}

/// Downloads `url` into `dest`, resuming from `dest.part` when its metadata still matches.
/// `ctl` is checked between chunks (PAUSE keeps the part file, CANCEL deletes it).
pub async fn fetch(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    size: u64,
    sha256: &str,
    ctl: &AtomicU8,
    mut progress: impl FnMut(u64),
) -> Result<Outcome> {
    let (part, meta_path) = (with_ext(dest, ".part"), with_ext(dest, ".part.json"));
    let mut meta = read_meta(dest).filter(|m| m.url == url && m.size == size);
    if meta.is_none() {
        clear(dest);
    }
    let mut have = part.metadata().map(|m| m.len()).unwrap_or(0);
    if have > size {
        clear(dest);
        have = 0;
        meta = None;
    }
    set_paused(dest, false);
    let mut hasher = Sha256::new();
    if have > 0 {
        // Up to 3 GB of reading: keep it off the async workers.
        let p = part.clone();
        hasher = tauri::async_runtime::spawn_blocking(move || {
            let mut h = Sha256::new();
            hash_file(&p, &mut h).map(|_| h)
        })
        .await??;
    }

    let mut rb = client.get(url);
    if have > 0 {
        rb = rb.header("Range", format!("bytes={have}-"));
        if let Some(etag) = meta.as_ref().and_then(|m| m.etag.clone()) {
            rb = rb.header("If-Range", etag); // file changed upstream → server sends 200 and we restart
        }
    }
    let mut resp = rb.send().await.context("download request failed")?;
    let status = resp.status().as_u16();
    let append = match status {
        206 if content_range_start(&resp) == Some(have) => true,
        200 | 206 => false,
        416 if have == size => true, // nothing left to fetch
        _ => bail!("download failed: HTTP {status}"),
    };
    if !append {
        have = 0;
        hasher = Sha256::new();
    }
    let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
    if !append || meta.is_none() {
        let m = PartMeta { url: url.into(), size, etag: etag.or(meta.and_then(|m| m.etag)), paused: false };
        std::fs::write(&meta_path, serde_json::to_vec(&m)?)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&part)
        .context("can't write the model file")?;
    progress(have);

    if status != 416 {
        loop {
            match ctl.load(Ordering::Relaxed) {
                PAUSE => {
                    set_paused(dest, true);
                    return Ok(Outcome::Paused);
                }
                CANCEL => {
                    drop(file);
                    clear(dest);
                    return Ok(Outcome::Cancelled);
                }
                _ => {}
            }
            let Some(chunk) = resp.chunk().await.context("download interrupted")? else { break };
            have += chunk.len() as u64;
            if have > size {
                drop(file);
                clear(dest);
                bail!("download is larger than expected; deleted it");
            }
            file.write_all(&chunk).context("can't write the model file (disk full?)")?;
            hasher.update(&chunk);
            progress(have);
        }
    }
    file.flush()?;
    drop(file);
    if have < size {
        bail!("download interrupted at {have} of {size} bytes");
    }
    let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !got.eq_ignore_ascii_case(sha256) {
        clear(dest);
        bail!("The downloaded file is damaged (checksum mismatch), so it was deleted. Download it again.");
    }
    std::fs::rename(&part, dest).context("can't move the model into place")?;
    let _ = std::fs::remove_file(&meta_path);
    Ok(Outcome::Done)
}

fn content_range_start(resp: &reqwest::Response) -> Option<u64> {
    // "bytes 100-199/200"
    let v = resp.headers().get("content-range")?.to_str().ok()?;
    v.strip_prefix("bytes ")?.split('-').next()?.trim().parse().ok()
}

fn hash_file(p: &Path, h: &mut Sha256) -> Result<()> {
    let mut f = std::fs::File::open(p)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        h.update(&buf[..n]);
    }
}

// ── Manager: one task per model, controlled from the UI. ──

fn active() -> &'static Mutex<HashMap<String, Arc<AtomicU8>>> {
    static A: OnceLock<Mutex<HashMap<String, Arc<AtomicU8>>>> = OnceLock::new();
    A.get_or_init(Default::default)
}

/// Own client: follows redirects (HF resolve → CDN; no keys are sent here) and uses the
/// system proxy (reqwest reads it by default). A 30 s read timeout turns a dead network into an error.
fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(concat!("Yappaflow/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default()
    })
}

pub fn is_active(id: &str) -> bool {
    active().lock().map(|a| a.contains_key(id)).unwrap_or(false)
}

/// Signals a running download. False when none is running.
pub fn stop(id: &str, how: u8) -> bool {
    let a = active().lock().ok();
    match a.as_ref().and_then(|a| a.get(id)) {
        Some(c) => {
            c.store(how, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

pub fn has_part(m: &Model) -> bool {
    super::path_of(m).is_some_and(|p| with_ext(&p, ".part.json").exists())
}
pub fn part_len(m: &Model) -> u64 {
    super::path_of(m).and_then(|p| with_ext(&p, ".part").metadata().ok()).map_or(0, |md| md.len())
}
pub fn progress_pct(m: &Model) -> Option<u64> {
    (is_active(&m.id) || has_part(m)).then(|| part_len(m) * 100 / m.size_bytes.max(1))
}
pub fn remove_part(m: &Model) {
    if let Some(p) = super::path_of(m) {
        clear(&p);
    }
}

fn free_space(dir: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

#[derive(Clone, Serialize)]
struct Progress<'a> {
    id: &'a str,
    done: u64,
    total: u64,
    /// downloading | paused | installed | none | failed
    state: &'a str,
    error: Option<String>,
}

pub fn start(app: AppHandle, m: &'static Model) -> Result<(), String> {
    let sha = m.sha256.clone().ok_or("This model has no checksum in the catalogue, so it can't be downloaded safely.")?;
    if super::installed(m) {
        return Ok(());
    }
    let dest = super::path_of(m).ok_or("models folder not set")?;
    let need = m.size_bytes.saturating_sub(part_len(m)) + 200 * 1024 * 1024;
    if let Some(free) = dest.parent().and_then(free_space) {
        if free < need {
            return Err(format!("Not enough disk space: {} needs {:.1} GB free.", m.display_name, need as f64 / 1e9));
        }
    }
    let ctl = Arc::new(AtomicU8::new(RUN));
    {
        let mut a = active().lock().map_err(|_| "download state poisoned")?;
        if a.contains_key(&m.id) {
            return Ok(());
        }
        a.insert(m.id.clone(), ctl.clone());
    }
    tauri::async_runtime::spawn(async move {
        let emit = |done, state, error| {
            let _ = app.emit("model_download", Progress { id: &m.id, done, total: m.size_bytes, state, error });
        };
        let mut last = Instant::now() - Duration::from_secs(1);
        let r = fetch(client(), &m.url, &dest, m.size_bytes, &sha, &ctl, |done| {
            if last.elapsed() >= Duration::from_millis(250) {
                last = Instant::now();
                emit(done, "downloading", None);
            }
        })
        .await;
        if let Ok(mut a) = active().lock() {
            a.remove(&m.id);
        }
        match r {
            Ok(Outcome::Done) => {
                log::info!("model {} downloaded and verified", m.id);
                emit(m.size_bytes, "installed", None)
            }
            Ok(Outcome::Paused) => emit(part_len(m), "paused", None),
            Ok(Outcome::Cancelled) => emit(0, "none", None),
            Err(e) => {
                log::warn!("model {} download failed: {e:#}", m.id);
                emit(part_len(m), if has_part(m) { "paused" } else { "none" }, Some(format!("{e:#}")));
            }
        }
    });
    Ok(())
}

/// After a restart, pick up every download that was still in progress.
pub fn resume_all(app: &AppHandle) {
    for m in &super::catalogue().models {
        let paused = super::path_of(m).and_then(|p| read_meta(&p)).is_some_and(|x| x.paused);
        if has_part(m) && !paused && !super::installed(m) {
            if let Err(e) = start(app.clone(), m) {
                log::warn!("can't resume {}: {e}", m.id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    /// Tiny HTTP server: honours `Range: bytes=N-` and `If-Range`, sends ETag "v1",
    /// and stops after `cut` body bytes on the first `cut_once` connections (a dropped network).
    fn serve(body: Vec<u8>, cut: Option<usize>, cut_once: usize) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.bin", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let mut ranges = vec![];
            for (i, s) in l.incoming().enumerate() {
                let mut s = s.unwrap();
                let mut r = std::io::BufReader::new(s.try_clone().unwrap());
                let (mut from, mut if_range) = (0usize, None::<String>);
                loop {
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    let l = line.trim().to_lowercase();
                    if l.is_empty() {
                        break;
                    }
                    if let Some(v) = l.strip_prefix("range: bytes=") {
                        from = v.trim_end_matches('-').parse().unwrap();
                    }
                    if let Some(v) = l.strip_prefix("if-range: ") {
                        if_range = Some(v.to_string());
                    }
                }
                if if_range.is_some_and(|e| e != "\"v1\"") {
                    from = 0;
                }
                ranges.push(format!("{from}"));
                let rest = &body[from..];
                let head = if from > 0 {
                    format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{}\r\n", body.len() - 1, body.len())
                } else {
                    "HTTP/1.1 200 OK\r\n".to_string()
                };
                let _ = write!(s, "{head}ETag: \"v1\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", rest.len());
                let n = if i < cut_once { cut.unwrap_or(rest.len()).min(rest.len()) } else { rest.len() };
                let _ = s.write_all(&rest[..n]);
                let _ = s.flush();
                drop(s);
                if i >= cut_once {
                    break;
                }
            }
            ranges
        });
        (url, h)
    }

    fn sha(b: &[u8]) -> String {
        Sha256::digest(b).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yf-dl-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("model.bin")
    }

    fn body() -> Vec<u8> {
        (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn run(url: &str, dest: &Path, b: &[u8], want_sha: &str) -> Result<Outcome> {
        let c = reqwest::Client::new();
        tauri::async_runtime::block_on(fetch(&c, url, dest, b.len() as u64, want_sha, &AtomicU8::new(RUN), |_| {}))
    }

    #[test]
    fn resumes_after_a_dropped_connection_and_verifies() {
        let b = body();
        let (url, h) = serve(b.clone(), Some(120_000), 1);
        let dest = tmp("resume");
        // First attempt stops short: the part file and its metadata stay.
        assert!(run(&url, &dest, &b, &sha(&b)).is_err());
        assert_eq!(with_ext(&dest, ".part").metadata().unwrap().len(), 120_000);
        assert_eq!(
            read_meta(&dest).unwrap(),
            PartMeta { url: url.clone(), size: b.len() as u64, etag: Some("\"v1\"".into()), paused: false }
        );
        // Second attempt asks for the rest only.
        assert_eq!(run(&url, &dest, &b, &sha(&b)).unwrap(), Outcome::Done);
        assert_eq!(std::fs::read(&dest).unwrap(), b);
        assert!(!with_ext(&dest, ".part").exists() && !with_ext(&dest, ".part.json").exists());
        assert_eq!(h.join().unwrap(), vec!["0", "120000"]);
    }

    #[test]
    fn checksum_mismatch_deletes_the_file() {
        let b = body();
        let (url, h) = serve(b.clone(), None, 0);
        let dest = tmp("bad");
        let e = run(&url, &dest, &b, &sha(b"something else")).unwrap_err();
        assert!(e.to_string().contains("checksum"), "{e}");
        assert!(!dest.exists() && !with_ext(&dest, ".part").exists() && !with_ext(&dest, ".part.json").exists());
        h.join().unwrap();
    }

    #[test]
    fn stale_part_for_another_url_is_discarded() {
        let b = body();
        let (url, h) = serve(b.clone(), None, 0);
        let dest = tmp("stale");
        std::fs::write(with_ext(&dest, ".part"), vec![9u8; 5000]).unwrap();
        std::fs::write(with_ext(&dest, ".part.json"), r#"{"url":"http://old/x","size":1,"etag":null}"#).unwrap();
        assert_eq!(run(&url, &dest, &b, &sha(&b)).unwrap(), Outcome::Done);
        assert_eq!(std::fs::read(&dest).unwrap(), b);
        assert_eq!(h.join().unwrap(), vec!["0"]);
    }

    #[test]
    fn pause_keeps_part_and_cancel_deletes_it() {
        let b = body();
        for (how, want) in [(PAUSE, Outcome::Paused), (CANCEL, Outcome::Cancelled)] {
            let (url, h) = serve(b.clone(), None, 0);
            let dest = tmp(&format!("ctl{how}"));
            let c = reqwest::Client::new();
            let got =
                tauri::async_runtime::block_on(fetch(&c, &url, &dest, b.len() as u64, &sha(&b), &AtomicU8::new(how), |_| {})).unwrap();
            assert_eq!(got, want);
            assert_eq!(with_ext(&dest, ".part.json").exists(), how == PAUSE);
            assert_eq!(read_meta(&dest).is_some_and(|m| m.paused), how == PAUSE);
            assert!(!dest.exists());
            h.join().unwrap();
        }
    }
}
