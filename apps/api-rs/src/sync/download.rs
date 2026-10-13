
//! Parallel byte-range download to a local path — port of `portal.sync.download`.
//!
//! Memory stays bounded regardless of file size: ranges are downloaded a bounded window at a
//! time, written to their offsets, and hashed in index order during the download (so the file is
//! never re-read).

use std::io::Write;
use std::path::Path;

use futures::stream::{FuturesUnordered, StreamExt};
use sha2::{Digest, Sha256};

pub const CHUNK_SIZE: i64 = 16 * 1024 * 1024;
pub const DEFAULT_CONCURRENCY: usize = 4;

pub struct DownloadResult {
    pub bytes_written: i64,
    pub sha256: String,
}

fn ranges(size: i64, chunk: i64) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < size {
        out.push((start, (start + chunk).min(size) - 1));
        start += chunk;
    }
    out
}

pub async fn download_to_file(
    http: &reqwest::Client,
    url: &str,
    size: Option<i64>,
    tmp_path: &Path,
    concurrency: usize,
) -> std::io::Result<DownloadResult> {
    if let Some(parent) = tmp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let (total, sha) = match size {
        None => download_single(http, url, tmp_path).await?,
        Some(s) if s <= CHUNK_SIZE => download_single(http, url, tmp_path).await?,
        Some(s) => download_ranged(http, url, s, tmp_path, concurrency).await?,
    };
    if let Some(expected) = size {
        if total != expected {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("size mismatch: expected {expected}, wrote {total}"),
            ));
        }
    }
    Ok(DownloadResult { bytes_written: total, sha256: sha })
}

async fn download_single(http: &reqwest::Client, url: &str, tmp_path: &Path) -> std::io::Result<(i64, String)> {
    let resp = http.get(url).send().await.map_err(io_err)?;
    if !resp.status().is_success() {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("GET {url} -> {}", resp.status())));
    }
    let bytes = resp.bytes().await.map_err(io_err)?;
    let mut f = std::fs::File::create(tmp_path)?;
    f.write_all(&bytes)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok((bytes.len() as i64, hex::encode(hasher.finalize())))
}

async fn download_ranged(
    http: &reqwest::Client,
    url: &str,
    size: i64,
    tmp_path: &Path,
    concurrency: usize,
) -> std::io::Result<(i64, String)> {
    let rs = ranges(size, CHUNK_SIZE);
    let mut file = std::fs::File::create(tmp_path)?;
    file.set_len(size as u64)?;
    let mut hasher = Sha256::new();
    let mut total: i64 = 0;

    // Bounded window: download `concurrency` ranges at once, then write + hash them in order.
    for batch in rs.chunks(concurrency.max(1)) {
        let mut futs = FuturesUnordered::new();
        for (start, end) in batch {
            let http = http.clone();
            let url = url.to_string();
            let (s, e) = (*start, *end);
            futs.push(async move {
                let resp = http
                    .get(&url)
                    .header(reqwest::header::RANGE, format!("bytes={s}-{e}"))
                    .send()
                    .await
                    .map_err(io_err)?;
                let status = resp.status();
                if !status.is_success() {
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("range GET -> {status}")));
                }
                let bytes = resp.bytes().await.map_err(io_err)?;
                Ok::<(i64, bytes::Bytes), std::io::Error>((s, bytes))
            });
        }
        let mut batch_results: Vec<(i64, bytes::Bytes)> = Vec::new();
        while let Some(r) = futs.next().await {
            batch_results.push(r?);
        }
        batch_results.sort_by_key(|(s, _)| *s);
        for (start, data) in batch_results {
            use std::io::Seek;
            file.seek(std::io::SeekFrom::Start(start as u64))?;
            file.write_all(&data)?;
            hasher.update(&data);
            total += data.len() as i64;
        }
    }
    Ok((total, hex::encode(hasher.finalize())))
}

fn io_err(e: reqwest::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}
