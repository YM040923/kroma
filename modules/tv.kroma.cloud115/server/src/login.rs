//! QR sign-in: 115's own mobile flow. Every 115 client kind can be online once,
//! so the operator picks a kind no other tool is using; confirming on the phone
//! returns the four cookies the rest of this module already speaks.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::Value;

use kroma_module_sdk::http::Fetch;

const BASE: &str = "https://qrcodeapi.115.com";
const TIMEOUT_SECS: u32 = 25;

/// The client kinds 115 offers. `alipaymini` is the default because it is the
/// one no other bridge tends to occupy.
pub const APPS: &[(&str, &str)] = &[
    ("alipaymini", "支付宝小程序"),
    ("wechatmini", "微信小程序"),
    ("tv", "电视端"),
    ("apple_tv", "Apple TV 端"),
    ("harmony", "鸿蒙端"),
    ("os_linux", "Linux 客户端"),
    ("os_mac", "Mac 客户端"),
    ("os_windows", "Windows 客户端"),
    ("web", "网页端"),
];

#[derive(Clone)]
struct Flow {
    app: String,
    uid: String,
    time: String,
    sign: String,
}

fn flows() -> &'static Mutex<HashMap<String, Flow>> {
    static FLOWS: OnceLock<Mutex<HashMap<String, Flow>>> = OnceLock::new();
    FLOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub flow: String,
    pub qr: String,
    pub svg: String,
    pub status: String,
    pub app: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub status: String,
    pub message: String,
    pub has_cookie: bool,
}

fn failure(message: impl Into<String>) -> String {
    message.into()
}

fn json(url: &str) -> Result<Value, String> {
    let response = Fetch::default()
        .purpose("cloud-drive")
        .header("User-Agent", crate::client::MODULE_UA)
        .header("Referer", "https://115.com/")
        .max_time(TIMEOUT_SECS)
        .get(url)
        .map_err(|error| failure(format!("连不上 115：{error}")))?;
    if !(200..300).contains(&response.status) {
        return Err(failure(format!("115 返回 HTTP {}", response.status)));
    }
    response
        .json()
        .map_err(|error| failure(format!("115 返回了无法解析的内容：{error}")))
}

fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|text| !text.trim().is_empty())
}

fn qr_svg(content: &str) -> String {
    match qrcode::QrCode::new(content.as_bytes()) {
        Ok(code) => code
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(220, 220)
            .build(),
        Err(_) => String::new(),
    }
}

/// Begin a scan: fetch a fresh token and remember what to poll.
pub fn begin(app: &str) -> Result<Started, String> {
    let app = APPS
        .iter()
        .find(|(value, _)| *value == app)
        .map(|(value, _)| *value)
        .unwrap_or("alipaymini");
    let answer = json(&format!("{BASE}/api/1.0/{app}/1.0/token/"))?;
    let data = answer
        .get("data")
        .filter(|data| data.is_object())
        .ok_or_else(|| failure("115 没有给出二维码，可能需要更新插件"))?;
    let uid = field(data, "uid").ok_or_else(|| failure("115 的二维码缺少 uid"))?.to_string();
    let time = field(data, "time").unwrap_or_default().to_string();
    let sign = field(data, "sign").unwrap_or_default().to_string();
    let qr = field(data, "qrcode")
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://115.com/scan/dg-{uid}"));
    let flow = format!("{uid}-{time}");
    flows()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            flow.clone(),
            Flow {
                app: app.to_string(),
                uid,
                time,
                sign,
            },
        );
    Ok(Started {
        flow,
        svg: qr_svg(&qr),
        qr,
        status: "pending".into(),
        app: app.to_string(),
    })
}

/// Poll one flow. Confirming on the phone lands the cookies in the module's
/// settings; the page then re-reads its settings like any other save would.
pub fn state(flow_id: &str) -> Result<State, String> {
    let flow = flows()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(flow_id)
        .cloned();
    let Some(flow) = flow else {
        return Ok(State {
            status: "expired".into(),
            message: "这次扫码已经结束，请重新开始".into(),
            has_cookie: false,
        });
    };
    let answer = json(&format!(
        "{BASE}/get/status/?uid={}&time={}&sign={}",
        flow.uid, flow.time, flow.sign
    ))?;
    let status = answer
        .get("data")
        .and_then(|data| data.get("status"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    match status {
        0 => Ok(State {
            status: "pending".into(),
            message: "用 115 App 扫码".into(),
            has_cookie: false,
        }),
        1 => Ok(State {
            status: "scanned".into(),
            message: "已扫码，请在手机上确认".into(),
            has_cookie: false,
        }),
        2 => {
            let cookie = exchange(&flow)?;
            crate::store::patch(|stored| {
                stored.cookie = cookie;
                stored.account = crate::store::AccountSnapshot::default();
            })
                .map_err(|error| failure(format!("保存 115 登录失败：{error}")))?;
            forget(flow_id);
            Ok(State {
                status: "confirmed".into(),
                message: "已登录".into(),
                has_cookie: true,
            })
        }
        -1 => {
            forget(flow_id);
            Ok(State {
                status: "expired".into(),
                message: "二维码已过期，请重新扫码".into(),
                has_cookie: false,
            })
        }
        -2 => {
            forget(flow_id);
            Ok(State {
                status: "failed".into(),
                message: "已在手机上取消登录".into(),
                has_cookie: false,
            })
        }
        other => Ok(State {
            status: "failed".into(),
            message: format!("扫码登录没成功（状态 {other}）"),
            has_cookie: false,
        }),
    }
}

pub fn cancel(flow_id: &str) {
    forget(flow_id);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieImported {
    pub ok: bool,
    pub message: String,
}

/// Import a browser session: keep the four cookies this module uses, prove
/// they still sign in, and only then store them.
pub fn import_cookie(raw: &str) -> CookieImported {
    let mut parts: Vec<String> = Vec::new();
    for key in ["UID", "CID", "SEID", "KID"] {
        let Some(value) = cookie_field(raw, key) else {
            return CookieImported {
                ok: false,
                message: format!("Cookie 里缺少 {key}"),
            };
        };
        parts.push(format!("{key}={value}"));
    }
    let cookie = parts.join("; ");
    let signed_in = Fetch::default()
        .purpose("cloud-drive")
        .header("Cookie", cookie.as_str())
        .header("User-Agent", crate::client::MODULE_UA)
        .header("Referer", "https://115.com/")
        .max_time(20)
        .get("https://my.115.com/?ct=guide&ac=status")
        .ok()
        .and_then(|response| response.json::<Value>().ok())
        .and_then(|body| body.get("state").and_then(Value::as_bool))
        .unwrap_or(false);
    if !signed_in {
        return CookieImported {
            ok: false,
            message: "这份 Cookie 已经失效，请在浏览器里重新登录 115".into(),
        };
    }
    match crate::store::patch(|stored| {
        stored.cookie = cookie;
        stored.account = crate::store::AccountSnapshot::default();
    }) {
        Ok(_) => CookieImported {
            ok: true,
            message: "已导入".into(),
        },
        Err(error) => CookieImported {
            ok: false,
            message: format!("保存失败：{error}"),
        },
    }
}

fn forget(flow_id: &str) {
    flows()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(flow_id);
}

/// Confirmed on the phone: trade the uid for the four cookies this module uses.
fn exchange(flow: &Flow) -> Result<String, String> {
    let response = Fetch::default()
        .purpose("cloud-drive")
        .header("User-Agent", crate::client::MODULE_UA)
        .header("Referer", "https://115.com/")
        .max_time(TIMEOUT_SECS)
        .post_form(
            &format!("{BASE}/app/1.0/{}/1.0/login/qrcode/", flow.app),
            &[("account", flow.uid.as_str())],
        )
        .map_err(|error| failure(format!("换取登录失败：{error}")))?;
    if !(200..300).contains(&response.status) {
        return Err(failure(format!("换取登录返回 HTTP {}", response.status)));
    }
    let answer: Value = response
        .json()
        .map_err(|error| failure(format!("登录响应无法解析：{error}")))?;
    if answer.get("state").and_then(Value::as_bool) == Some(false) {
        let message = field(&answer, "error")
            .or_else(|| field(&answer, "errmsg"))
            .unwrap_or("115 拒绝了这次登录");
        return Err(failure(message.to_string()));
    }
    let cookie = answer
        .get("data")
        .and_then(|data| data.get("cookie"))
        .ok_or_else(|| failure("115 没有返回登录 Cookie"))?;
    let mut parts: Vec<String> = Vec::new();
    for key in ["UID", "CID", "SEID", "KID"] {
        let value = match cookie {
            Value::Object(map) => map.get(key).and_then(Value::as_str).map(str::to_string),
            Value::String(text) => cookie_field(text, key),
            _ => None,
        };
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            parts.push(format!("{key}={value}"));
        }
    }
    if parts.len() < 4 {
        return Err(failure("115 扫码确认后没有给出完整的登录 Cookie"));
    }
    Ok(parts.join("; "))
}

fn cookie_field(header: &str, key: &str) -> Option<String> {
    header.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == key).then(|| value.trim().to_string())
    })
}
