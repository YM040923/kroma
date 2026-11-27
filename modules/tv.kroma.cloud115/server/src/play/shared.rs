//! Session-private rendezvous on a cold memory lookup. Only the elected caller
//! polls `sign`; no filesystem lock is held while pacing, signing or waiting.

use super::{valid_until_ms, ApiError, DownloadLink, Key, Resolved};
use crate::store::now_ms;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::future::Future;
use std::io::{self, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::{sleep, Instant};

const SHARDS: u8 = 16;
const ROWS: usize = 8;
const MAX_REPLY: usize = 16 * 1024;
const MAX_TABLE: usize = ROWS * (MAX_REPLY + 512);
const SHARE_MS: i64 = 15_000;
const LEASE_MS: i64 = 60_000;
const GATE_MS: i64 = 2_000; // only local snapshot I/O, never an await
const POLL: Duration = Duration::from_millis(35);

fn unavailable() -> ApiError {
    ApiError::network("播放签发临时目录不可用")
}
pub(super) fn interrupted() -> ApiError {
    ApiError::network("本次播放签发已中断，请重试")
}

#[derive(Serialize, Deserialize)]
struct Row {
    key: String,
    id: String,
    expires: i64,
    value: Value,
}

#[derive(Serialize, Deserialize)]
enum Value {
    Signing,
    Ready { link: DownloadLink, until: i64 },
    Failed(u16, i64, String),
}

struct Attempt {
    shard: PathBuf,
    key: String,
    id: String,
}

impl Attempt {
    fn new(root: &Path, key: &Key) -> Result<Self, ApiError> {
        let mut h = Sha256::new();
        for field in [&key.0, &key.1, &key.2] {
            h.update((field.len() as u64).to_le_bytes());
            h.update(field.as_bytes());
        }
        let digest = h.finalize();
        let shard = root.join(format!("{:02x}", digest[0] % SHARDS));
        fs::create_dir_all(&shard).map_err(|_| unavailable())?;
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|_| unavailable())?;
        Ok(Self {
            shard,
            key: hex::encode(digest),
            id: hex::encode(nonce),
        })
    }

    fn try_gate(&self) -> io::Result<Option<Gate>> {
        let lock = self.shard.join("gate");
        match fs::create_dir(&lock) {
            Ok(()) => {
                let name = format!("{}-{}", now_ms(), self.id);
                let marker = lock.join(&name);
                if let Err(e) = fs::File::create_new(&marker) {
                    let _ = fs::remove_dir(&lock);
                    return Err(e);
                }
                Ok(Some(Gate {
                    rows: self.shard.join("rows"),
                    stage: lock.join(format!("{name}.tmp")),
                    lock,
                    marker,
                }))
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                recover_gate(&lock)?;
                Ok(None)
            }
            Err(e) if e.kind() == ErrorKind::PermissionDenied => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn gate(&self) -> Result<Gate, ApiError> {
        let stop = Instant::now() + Duration::from_millis(2 * GATE_MS as u64);
        loop {
            if let Some(g) = self.try_gate().map_err(|_| unavailable())? {
                return Ok(g);
            }
            if Instant::now() >= stop {
                return Err(unavailable());
            }
            sleep(Duration::from_millis(8)).await;
        }
    }

    fn inspect(&self, gate: &Gate, joined: Option<&str>) -> Result<Step, ApiError> {
        let mut rows = gate.read()?;
        let now = now_ms();
        rows.retain(|r| r.expires > now);
        let row = rows.iter().find(|r| {
            r.key == self.key
                && match joined {
                    Some(id) => r.id == id,
                    None => !matches!(r.value, Value::Failed(..)),
                }
        });
        if let Some(row) = row {
            return match &row.value {
                Value::Signing => Ok(Step::Wait(row.id.clone())),
                Value::Ready { link, until } => Ok(Step::Hit(link.clone(), *until)),
                Value::Failed(http, errno, message) => Err(ApiError {
                    http: *http,
                    errno: *errno,
                    message: message.clone(),
                }),
            };
        }
        if joined.is_some() {
            return Err(interrupted());
        } // never take over/re-sign a joined flight
        if rows.len() == ROWS {
            // Never evict a signer (even if another key hashes to our shard).
            let oldest = rows
                .iter()
                .enumerate()
                .filter(|(_, r)| !matches!(r.value, Value::Signing))
                .min_by_key(|(_, r)| r.expires)
                .map(|(i, _)| i)
                .ok_or_else(|| ApiError::network("播放签发正忙，请重试"))?;
            rows.swap_remove(oldest);
        }
        rows.push(Row {
            key: self.key.clone(),
            id: self.id.clone(),
            expires: now + LEASE_MS,
            value: Value::Signing,
        });
        gate.save(&rows)?;
        Ok(Step::Lead)
    }

    fn update(&self, gate: &Gate, value: Value) -> Result<(), ApiError> {
        let mut rows = gate.read()?;
        let now = now_ms();
        let row = rows
            .iter_mut()
            .find(|r| {
                r.key == self.key
                    && r.id == self.id
                    && r.expires > now
                    && matches!(r.value, Value::Signing)
            })
            .ok_or_else(interrupted)?;
        row.expires = match &value {
            Value::Signing => now + LEASE_MS,
            Value::Ready { until, .. } => (*until).min(now + SHARE_MS),
            Value::Failed(..) => now + SHARE_MS, // only visible to waiters for this id
        };
        row.value = value;
        gate.save(&rows)
    }
}

fn recover_gate(lock: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(lock) {
        Ok(entries) => entries,
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::PermissionDenied) => {
            return Ok(())
        }
        Err(e) => return Err(e),
    };
    let mut empty = true;
    for entry in entries.flatten() {
        empty = false;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some((since, _)) = name.split_once('-') else {
            continue;
        };
        if since
            .parse::<i64>()
            .ok()
            .is_some_and(|ms| now_ms().saturating_sub(ms) >= GATE_MS)
        {
            let _ = fs::remove_file(entry.path());
            let _ = fs::remove_dir(lock);
        }
    }
    if empty
        && fs::metadata(lock)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age >= Duration::from_millis(GATE_MS as u64))
    {
        let _ = fs::remove_dir(lock);
    }
    Ok(())
}

struct Gate {
    lock: PathBuf,
    marker: PathBuf,
    stage: PathBuf,
    rows: PathBuf,
}
impl Drop for Gate {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.stage);
        if fs::remove_file(&self.marker).is_ok() {
            let _ = fs::remove_dir(&self.lock);
        }
    }
}
impl Gate {
    fn read(&self) -> Result<Vec<Row>, ApiError> {
        let file = match fs::File::open(&self.rows) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(unavailable()),
        };
        let mut data = Vec::new();
        file.take(MAX_TABLE as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|_| unavailable())?;
        if data.len() > MAX_TABLE {
            return Err(unavailable());
        }
        serde_json::from_slice::<Vec<Row>>(&data)
            .ok()
            .filter(|rows| rows.len() <= ROWS)
            .ok_or_else(unavailable)
    }

    fn save(&self, rows: &[Row]) -> Result<(), ApiError> {
        let data = serde_json::to_vec(rows).map_err(|_| unavailable())?;
        if data.len() > MAX_TABLE {
            return Err(unavailable());
        }
        if !self.marker.is_file() {
            return Err(unavailable());
        }
        fs::write(&self.stage, data).map_err(|_| unavailable())?;
        if !self.marker.is_file() {
            return Err(unavailable());
        }
        fs::rename(&self.stage, &self.rows).map_err(|_| unavailable())
    }
}

enum Step {
    Lead,
    Wait(String),
    Hit(DownloadLink, i64),
}

struct Owner<'a>(Option<&'a Attempt>);
impl Drop for Owner<'_> {
    fn drop(&mut self) {
        if let Some(a) = self.0 {
            // Cancellation normally wakes peers on their next poll. If the gate
            // is busy or the guest traps (no Drop), the renewable lease bounds it.
            if let Ok(Some(g)) = a.try_gate() {
                let _ = a.update(&g, Value::Failed(0, 0, interrupted().message));
            }
        }
    }
}

pub(super) async fn resolve(
    root: &Path,
    key: &Key,
    ttl_secs: u64,
    sign: impl Future<Output = Result<DownloadLink, ApiError>>,
) -> Result<(Resolved, i64), ApiError> {
    let attempt = Attempt::new(root, key)?;
    let mut joined = None;
    loop {
        let step = attempt.inspect(&attempt.gate().await?, joined.as_deref())?;
        match step {
            Step::Lead => break,
            Step::Hit(link, until) => return Ok((Resolved { link, cached: true }, until)),
            Step::Wait(id) => {
                joined = Some(id);
                sleep(POLL).await;
            }
        }
    }
    let mut owner = Owner(Some(&attempt));
    tokio::pin!(sign);
    let result = loop {
        // Check ownership first if a suspended guest wakes after its lease.
        tokio::select! { biased;
            _ = sleep(Duration::from_millis(LEASE_MS as u64 / 3)) => {
                attempt.update(&attempt.gate().await?, Value::Signing)?;
            }
            res = &mut sign => break res,
        }
    };
    let result = result.and_then(|link| {
        let now = now_ms();
        let until = valid_until_ms(&link.url, ttl_secs, now);
        if until <= now {
            return Err(ApiError::network("115 返回的播放链接已到期"));
        }
        Ok((
            Resolved {
                link,
                cached: false,
            },
            until,
        ))
    });
    let mut value = match &result {
        Ok((r, until)) => Value::Ready {
            link: r.link.clone(),
            until: *until,
        },
        Err(e) => Value::Failed(e.http, e.errno, e.message.clone()),
    };
    let mut result = result;
    if serde_json::to_vec(&value).map_err(|_| unavailable())?.len() > MAX_REPLY {
        let error = ApiError::network("115 播放答复过大");
        value = Value::Failed(error.http, error.errno, error.message.clone());
        result = Err(error);
    }
    attempt.update(&attempt.gate().await?, value)?;
    owner.0 = None;
    result
}
