//! Rewrite the base address inside STRM files this module generated. Only a
//! file whose whole content is one play URL is touched: anything hand-edited
//! fails the shape check and is left alone.

use std::path::PathBuf;

use serde::Serialize;

use crate::settings::Config;
use crate::MODULE_ID;

const MAX_FILE_BYTES: u64 = 64 * 1024;
const MAX_FILES: usize = 50_000;
const SAMPLES: usize = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub path: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceReport {
    pub count: usize,
    pub samples: Vec<Sample>,
}

fn marker() -> String {
    format!("/api/module/{MODULE_ID}/play?pc=")
}

fn plan(config: &Config) -> Vec<(PathBuf, String)> {
    let base = config.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Vec::new();
    }
    let marker = marker();
    let mut out = Vec::new();
    for library in &config.libraries {
        let root = PathBuf::from(library.local.trim());
        if !root.is_dir() {
            continue;
        }
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let path = entry.path();
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|ext| ext.to_str()) != Some("strm") {
                    continue;
                }
                if path.metadata().map(|meta| meta.len()).unwrap_or(0) > MAX_FILE_BYTES {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let trimmed = content.trim();
                let Some(at) = trimmed.find(&marker) else {
                    continue;
                };
                let prefix = trimmed[..at].trim_end_matches('/');
                let suffix = trimmed[at + marker.len()..].trim();
                let http = prefix.starts_with("http://") || prefix.starts_with("https://");
                let mut parts = suffix.splitn(2, "&next=");
                let pick = parts.next().unwrap_or_default();
                let pick_code = !pick.is_empty()
                    && pick.len() <= 64
                    && pick.chars().all(|c| c.is_ascii_hexdigit());
                let next_ok = match parts.next() {
                    None => true,
                    Some(list) => {
                        !list.is_empty()
                            && list.split(',').all(|code| {
                                !code.is_empty()
                                    && code.len() <= 64
                                    && code.chars().all(|c| c.is_ascii_hexdigit())
                            })
                    }
                };
                if !http || !pick_code || !next_ok || prefix == base {
                    continue;
                }
                let next = format!("{base}{marker}{suffix}\n");
                out.push((path, next));
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out
}

fn report_for(planned: &[(PathBuf, String)], samples: bool) -> ReplaceReport {
    ReplaceReport {
        count: planned.len(),
        samples: if samples {
            planned
                .iter()
                .take(SAMPLES)
                .map(|(path, next)| Sample {
                    path: path.display().to_string(),
                    old: std::fs::read_to_string(path)
                        .map(|text| text.trim().to_string())
                        .unwrap_or_default(),
                    new: next.trim().to_string(),
                })
                .collect()
        } else {
            Vec::new()
        },
    }
}

pub fn preview(config: &Config) -> ReplaceReport {
    report_for(&plan(config), true)
}

pub fn run(config: &Config) -> Result<ReplaceReport, String> {
    let planned = plan(config);
    let mut written = 0;
    for (path, next) in &planned {
        let staged = path.with_extension("strm.tmp");
        std::fs::write(&staged, next).map_err(|e| format!("写 {} 失败：{e}", staged.display()))?;
        std::fs::rename(&staged, path).map_err(|e| format!("替换 {} 失败：{e}", path.display()))?;
        written += 1;
    }
    Ok(report_for(&planned[..written], false))
}