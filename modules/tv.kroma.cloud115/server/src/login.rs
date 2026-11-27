//! 115 device authorization. The settings/export file is never used for secrets.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use kroma_module_sdk::http::Fetch;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{ErrorKind, Write},
    path::PathBuf,
    time::Duration,
};

const PASSPORT: &str = "https://passportapi.115.com/open";
const BAD: &str = "115 授权响应无效";
const STORE: &str = "无法读写授权记录";
const DAY: i64 = 86400;
type R<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub flow: String,
    pub qr: String,
    pub svg: String,
    pub status: String,
    pub app_id: String,
}
#[derive(Serialize)]
pub struct State {
    pub status: String,
    pub message: String,
    pub authorized: bool,
}
fn status(s: &str, msg: &str) -> State {
    State {
        status: s.into(),
        message: msg.into(),
        authorized: s == "confirmed",
    }
}

#[derive(Serialize, Deserialize)]
struct Pending {
    id: String,
    app: String,
    uid: String,
    time: u64,
    sign: String,
    verifier: String,
    until: i64,
}
#[derive(Serialize, Deserialize)]
struct Tokens {
    access: String,
    refresh: String,
    expires: i64,
}
#[derive(Serialize, Deserialize)]
struct Grant {
    id: String,
    flow: String,
    app: String,
    tokens: Tokens,
    refresh_until: i64,
}
#[derive(Serialize, Deserialize, Default)]
struct Records {
    active: Option<Grant>,
    pending: Option<Pending>,
}
fn now() -> i64 {
    crate::store::now_ms() / 1000
}
fn file() -> PathBuf {
    crate::store::path().with_file_name("cloud115.oauth.json")
}
fn load() -> R<Records> {
    match fs::read(file()) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| STORE.into()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Records::default()),
        Err(_) => Err(STORE.into()),
    }
}
struct Lock {
    _guard: crate::store::FileLock,
}
impl Lock {
    fn acquire() -> R<Self> {
        let guard = crate::store::FileLock::acquire("cloud115-oauth.lock", Duration::from_secs(22))
            .map_err(|_| "授权记录正忙或无法锁定")?;
        Ok(Self { _guard: guard })
    }
    fn save(&self, records: &Records) -> R<()> {
        let target = file();
        fs::create_dir_all(target.parent().ok_or(STORE)?).map_err(|_| STORE)?;
        let tmp = target.with_extension("staged");
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut out = opts.open(&tmp).map_err(|_| STORE)?;
        let result = (|| {
            out.write_all(&serde_json::to_vec(records).map_err(|_| STORE)?)
                .map_err(|_| STORE)?;
            out.sync_all().map_err(|_| STORE)?;
            drop(out);
            fs::rename(&tmp, target).map_err(|_| STORE.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        result
    }
}
fn random() -> R<String> {
    let mut b = [0; 32];
    getrandom::fill(&mut b).map_err(|_| "安全随机源不可用")?;
    Ok(URL_SAFE_NO_PAD.encode(b))
}
fn challenge(v: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(v.as_bytes()))
}
fn fetch() -> Fetch {
    Fetch::default()
        .purpose("cloud-drive")
        .no_redirects()
        .max_bytes(128 * 1024)
        .max_time(20)
}
fn success(v: &Value) -> R<bool> {
    match &v["state"] {
        Value::Bool(b) => Ok(*b),
        n if n.as_i64() == Some(1) => Ok(true),
        n if n.as_i64() == Some(0) => Ok(false),
        _ => Err(BAD.into()),
    }
}
fn post(method: &str, fields: &[(&str, &str)]) -> R<Value> {
    let r = fetch()
        .post_form(&format!("{PASSPORT}/{method}"), fields)
        .map_err(|_| "无法连接 115 授权服务")?;
    let mut v: Value = r.json().unwrap_or(Value::Null);
    if !(200..300).contains(&r.status) || success(&v) != Ok(true) {
        let code = [&v["code"], &v["errno"]]
            .iter()
            .filter_map(|n| n.as_i64().or_else(|| n.as_str()?.parse().ok()))
            .find(|n| *n != 0)
            .unwrap_or(0);
        if method == "refreshToken" {
            crate::pacer::pacer().note(r.status, code);
        }
        let reason = match code {
            40140100 | 40140106 | 40140107 => "AppID 无效或应用不存在",
            40140108 => "应用尚未通过审核",
            40140109 => "应用已停用",
            40140110 | 40140118 => "应用或开发者认证已过期",
            40140116 | 40140119 | 40140120 => "授权已失效，请重新扫码",
            40140117 => "刷新太频繁，请稍后重试",
            40140122 => "账号已达到授权应用数量上限",
            _ => "授权请求未成功",
        };
        return Err(format!(
            "115 授权：{reason}（HTTP {}，代码 {code}）",
            r.status
        ));
    }
    Ok(v["data"].take())
}
fn text(v: &Value, k: &str) -> R<String> {
    v[k].as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 16384 && !s.chars().any(char::is_control))
        .map(String::from)
        .ok_or_else(|| BAD.into())
}
fn tokens(v: &Value, issued: i64) -> R<Tokens> {
    let ttl = v["expires_in"]
        .as_i64()
        .filter(|t| *t > 0)
        .ok_or(BAD)?
        .min(30 * DAY);
    Ok(Tokens {
        access: text(v, "access_token")?,
        refresh: text(v, "refresh_token")?,
        expires: issued + ttl,
    })
}
impl Records {
    fn current(&self) -> Option<&Grant> {
        self.active
            .as_ref()
            .filter(|g| g.refresh_until > now() || g.tokens.expires > now())
    }
    fn cancel(&mut self, id: &str) -> bool {
        if self.pending.as_ref().is_some_and(|p| p.id == id) {
            self.pending = None;
            true
        } else {
            false
        }
    }
    fn ended(&self, flow: &str) -> State {
        if self.current().is_some_and(|g| g.flow == flow) {
            status("confirmed", "已授权")
        } else {
            status("expired", "二维码已失效，请重新扫码")
        }
    }
}
pub fn begin(app_id: &str) -> R<Started> {
    let app = app_id.trim();
    if app.is_empty() || app.len() > 32 || !app.bytes().all(|b| b.is_ascii_digit()) {
        return Err("请输入数字 AppID".into());
    }
    let lock = Lock::acquire()?;
    let mut records = load()?;
    let verifier = random()?;
    let flow = random()?;
    let issued = now();
    let d = post(
        "authDeviceCode",
        &[
            ("client_id", app),
            ("code_challenge", &challenge(&verifier)),
            ("code_challenge_method", "sha256"),
        ],
    )?;
    let qr = text(&d, "qrcode")?;
    let svg = qrcode::QrCode::new(qr.as_bytes())
        .map_err(|_| BAD)?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .build();
    records.pending = Some(Pending {
        id: flow.clone(),
        app: app.into(),
        uid: text(&d, "uid")?,
        time: d["time"].as_u64().ok_or(BAD)?,
        sign: text(&d, "sign")?,
        verifier,
        until: issued + 300,
    });
    lock.save(&records)?;
    Ok(Started {
        flow,
        qr,
        svg,
        status: "pending".into(),
        app_id: app.into(),
    })
}
fn poll(p: &Pending) -> R<i64> {
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("uid", &p.uid)
        .append_pair("time", &p.time.to_string())
        .append_pair("sign", &p.sign)
        .finish();
    // This is a long poll: an ordinary transport timeout without an event is
    // not a reason to destroy a valid QR. The flow has its own absolute expiry.
    let r = match fetch().get(&format!("https://qrcodeapi.115.com/get/status/?{query}")) {
        Ok(r) => r,
        Err(e)
            if {
                let m = e.to_string().to_ascii_lowercase();
                m.contains("timed out") || m.contains("timeout") || m.contains("curl exited 28")
            } =>
        {
            return Ok(0)
        }
        Err(_) => return Err("扫码状态暂不可用，请重试".into()),
    };
    if !(200..300).contains(&r.status) {
        return Err("扫码状态请求失败".into());
    }
    let v: Value = r.json().map_err(|_| BAD)?;
    if !success(&v)? {
        return Ok(-1);
    }
    match &v["data"]["status"] {
        Value::Null => Ok(0),
        n => n.as_i64().ok_or_else(|| BAD.into()),
    }
}
pub fn state(flow: &str) -> R<State> {
    let snapshot = load()?;
    let Some(p) = snapshot.pending.as_ref().filter(|p| p.id == flow) else {
        return Ok(snapshot.ended(flow));
    };
    // Long poll outside the lock. Revalidate after waiting, even on network errors.
    let outcome = if p.until > now() { poll(p) } else { Ok(-1) };
    let lock = Lock::acquire()?;
    let mut records = load()?;
    let Some(p) = records.pending.as_ref().filter(|p| p.id == flow) else {
        return Ok(records.ended(flow));
    };
    let result = match if p.until <= now() { -1 } else { outcome? } {
        0 => return Ok(status("pending", "请用 115 App 扫码")),
        1 => return Ok(status("scanned", "已扫码，请在手机上确认")),
        2 => {
            let id = random()?;
            let issued = now();
            let d = post(
                "deviceCodeToToken",
                &[("uid", &p.uid), ("code_verifier", &p.verifier)],
            )?;
            records.active = Some(Grant {
                id,
                flow: flow.into(),
                app: p.app.clone(),
                tokens: tokens(&d, issued)?,
                refresh_until: issued + 365 * DAY,
            });
            status("confirmed", "已授权")
        }
        -1 => status("expired", "二维码已失效，请重新扫码"),
        -2 => status("failed", "已在手机上取消授权"),
        _ => status("failed", "未知扫码状态，请重新扫码"),
    };
    records.pending = None;
    lock.save(&records)?;
    Ok(result)
}
pub fn cancel(flow: &str) {
    let _ = (|| -> R<()> {
        let lock = Lock::acquire()?;
        let mut r = load()?;
        if r.cancel(flow) {
            lock.save(&r)?;
        }
        Ok(())
    })();
}
pub fn disconnect() -> R<()> {
    Lock::acquire()?.save(&Records::default())
}
pub fn authorization_id() -> String {
    load()
        .ok()
        .and_then(|r| r.current().map(|g| g.id.clone()))
        .unwrap_or_default()
}
pub fn app_id() -> String {
    load()
        .ok()
        .and_then(|r| {
            r.current()
                .map(|g| g.app.clone())
                .or_else(|| r.pending.filter(|p| p.until > now()).map(|p| p.app))
        })
        .unwrap_or_default()
}
impl Grant {
    fn needs_refresh(&self, previous: Option<&str>, at: i64) -> bool {
        let forced = previous == Some(self.tokens.access.as_str());
        forced
            || self.tokens.expires <= at
            || (self.tokens.expires <= at + 60 && self.refresh_until > at && previous.is_none())
    }
    fn resolve(
        &mut self,
        previous: Option<&str>,
        at: i64,
        refresh: impl FnOnce(&str) -> R<Value>,
    ) -> R<bool> {
        let forced = previous == Some(self.tokens.access.as_str());
        if !forced
            && self.tokens.expires > at
            && (self.tokens.expires > at + 60 || self.refresh_until <= at || previous.is_some())
        {
            return Ok(false);
        }
        if self.refresh_until <= at {
            return Err("授权已到期，请重新扫码".into());
        }
        self.tokens = tokens(&refresh(&self.tokens.refresh)?, at)?;
        Ok(true)
    }
}
async fn resolve(id: &str, previous: Option<&str>) -> R<String> {
    // Reserve before taking the OAuth lock. Re-read under lock: another instance
    // may have rotated the token or disconnected while this request was waiting.
    let r = load()?;
    let g = r
        .current()
        .filter(|g| g.id == id)
        .ok_or("授权已断开或切换")?;
    let at = now();
    if !g.needs_refresh(previous, at) {
        return Ok(g.tokens.access.clone());
    }
    if g.refresh_until <= at {
        return Err("授权已到期，请重新扫码".into());
    }
    crate::pacer::pacer().acquire().await?;
    let lock = Lock::acquire()?;
    let mut r = load()?;
    let g = r
        .active
        .as_mut()
        .filter(|g| g.id == id)
        .ok_or("授权已断开或切换")?;
    if !g.needs_refresh(previous, now()) {
        return Ok(g.tokens.access.clone());
    }
    let changed = g.resolve(previous, now(), |t| {
        post("refreshToken", &[("refresh_token", t)])
    })?;
    let access = g.tokens.access.clone();
    if changed {
        lock.save(&r)?;
    }
    Ok(access)
}
pub async fn token(grant: &str) -> R<String> {
    resolve(grant, None).await
}
pub async fn renew(grant: &str, previous_token: &str) -> R<String> {
    resolve(grant, Some(previous_token)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn grant() -> Grant {
        Grant {
            id: "grant".into(),
            flow: "first".into(),
            app: "1".into(),
            refresh_until: now() + DAY,
            tokens: Tokens {
                access: "a".into(),
                refresh: "r".into(),
                expires: now() + 100,
            },
        }
    }
    #[test]
    fn pkce_rfc7636() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
    #[test]
    fn rotation_and_duplicate_expired_response() {
        let mut g = grant();
        let deadline = g.refresh_until;
        assert!(g
            .resolve(Some("a"), now(), |r| {
                assert_eq!(r, "r");
                Ok(serde_json::json!({"access_token":"b","refresh_token":"s","expires_in":7200}))
            })
            .unwrap());
        assert_eq!(g.tokens.refresh, "s");
        assert_eq!(g.refresh_until, deadline);
        assert!(!g
            .resolve(Some("a"), now(), |_| panic!("duplicate refresh"))
            .unwrap());
        assert_eq!(g.tokens.access, "b");
    }
    #[test]
    fn an_expired_access_token_is_never_returned_after_a_concurrent_refresh() {
        let mut g = grant();
        let at = now();
        g.tokens.expires = at - 1;
        assert!(g.needs_refresh(Some("an-older-access-token"), at));
        g.tokens.expires = at + 600;
        assert!(!g.needs_refresh(Some("an-older-access-token"), at));
    }

    #[test]
    fn cancel_only_matching_pending() {
        let mut r = Records {
            active: Some(grant()),
            pending: Some(Pending {
                id: "next".into(),
                app: "2".into(),
                uid: "uid".into(),
                time: 1,
                sign: "sign".into(),
                verifier: "v".into(),
                until: now() + 30,
            }),
        };
        assert!(r.ended("first").authorized);
        assert!(!r.cancel("first"));
        assert!(r.pending.is_some());
        assert!(r.cancel("next"));
        assert!(r.pending.is_none());
        assert_eq!(r.current().unwrap().id, "grant");
        assert!(!r.ended("next").authorized);
    }
}
