//! Reading just enough of a remote container to describe it.
//!
//! KROMA's own scanner cannot probe a `.strm`: this server's ffprobe accepts
//! only the `file` protocol, so its probe pass can only ever fail on a URL. The
//! bridge side therefore writes `.info.json` beside the entry — and to fill it
//! with more than a size, this module reads the container header over HTTP:
//! Matroska (EBML) keeps its duration and tracks near the head; MP4 may keep its
//! `moov` at the end, so a second, suffix range is read when the head has none.
//!
//! Everything here is best-effort. A container this does not understand, a
//! fetch the CDN refuses, a header cut in half: the entry still plays, it just
//! arrives undescribed rather than mis-described.

use serde::Serialize;

use kroma_module_sdk::domain::{AudioTechnical, VideoTechnical};
use kroma_module_sdk::http::Fetch;
pub use kroma_module_sdk::domain::MEDIA_PROBE_VERSION;

use crate::client::{self, ApiError, MODULE_UA};

const HEAD_BYTES: u64 = 2 * 1024 * 1024;
const TAIL_BYTES: u64 = 8 * 1024 * 1024;
const MAX_READ: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Video {
    pub technical: Option<VideoTechnical>,
    pub codec: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub hdr: bool,
    #[serde(rename = "bitDepth")]
    pub bit_depth: Option<u32>,
    #[serde(rename = "hdrFormat", skip_serializing_if = "Option::is_none")]
    pub hdr_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Color {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primaries: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matrix: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Audio {
    pub technical: Option<AudioTechnical>,
    pub index: u32,
    pub codec: String,
    pub channels: Option<u32>,
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub default: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Subtitle {
    pub language: Option<String>,
    pub codec: String,
    pub forced: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Probed {
    pub duration_ms: Option<u64>,
    pub video: Option<Video>,
    pub audio: Option<Audio>,
    pub audio_tracks: Vec<Audio>,
    pub subtitles: Vec<Subtitle>,
}

impl Probed {
    pub fn described(&self) -> bool {
        self.duration_ms.is_some() || self.video.is_some()
    }
}

/// Sign a link and read the container. `size` is from the listing, used for the
/// suffix range an MP4 needs when its `moov` sits at the end.
pub async fn probe(cookie: &str, pick_code: &str, size: i64) -> Result<Option<Probed>, ApiError> {
    let link = client::sign(cookie, MODULE_UA, pick_code).await?;
    if link.needs_cookie {
        return Ok(None);
    }
    let head = match read_range(&link.url, &format!("bytes=0-{}", HEAD_BYTES - 1)) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };

    let mut probed = if head.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        mkv::parse(&head)
    } else if head.len() >= 8 && &head[4..8] == b"ftyp" {
        let from_head = mp4::parse(&head);
        if from_head.as_ref().is_some_and(|p| p.duration_ms.is_some() && p.video.is_some())
            || size <= (HEAD_BYTES as i64) * 2
        {
            from_head
        } else {
            // `moov` after the media data: read the tail and look there.
            let start = (size as u64).saturating_sub(TAIL_BYTES).max(HEAD_BYTES);
            match read_range(&link.url, &format!("bytes={start}-{}", (size as u64).saturating_sub(1)))
            {
                Ok(tail) => mp4::parse_moov_scan(&tail).or(from_head),
                Err(_) => from_head,
            }
        }
    } else {
        None
    };

    if let Some(ref mut probed) = probed {
        if let Some(first) = probed.audio_tracks.first().cloned() {
            probed.audio = Some(first);
        }
    }
    Ok(probed.filter(|p| p.described()))
}

fn read_range(url: &str, range: &str) -> Result<Vec<u8>, String> {
    let response = Fetch::default()
        .purpose("cloud-drive")
        .header("User-Agent", MODULE_UA)
        .header("Range", range)
        .max_time(60)
        .max_bytes(MAX_READ)
        .get(url)
        .map_err(|error| error.to_string())?;
    if !(200..300).contains(&response.status) {
        return Err(format!("range read answered {}", response.status));
    }
    Ok(response.body)
}

// ---------------------------------------------------------------------------
// Matroska / EBML

mod mkv {
    use super::{Audio, AudioTechnical, Color, Probed, Subtitle, Video, VideoTechnical};

    const SEGMENT: u64 = 0x1853_8067;
    const INFO: u64 = 0x1549_a966;
    const TRACKS: u64 = 0x1654_ae6b;
    const TRACK_ENTRY: u64 = 0xae;
    const VIDEO: u64 = 0xe0;
    const AUDIO: u64 = 0xe1;
    const COLOUR: u64 = 0x55b0;

    #[derive(Default)]
    struct Track {
        number: u64,
        kind: u64,
        codec: String,
        language: Option<String>,
        title: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
        display_width: Option<u32>,
        display_height: Option<u32>,
        channels: Option<u32>,
        sample_rate: Option<u32>,
        default_duration_ns: Option<u64>,
        frame_rate: Option<f64>,
        bit_depth: Option<u32>,
        primaries: Option<u64>,
        transfer: Option<u64>,
        matrix: Option<u64>,
        default: bool,
        forced: bool,
    }

    /// Read one variable-length integer. `keep_marker` leaves the length marker
    /// in the value, which is how an element id is spelled; sizes drop it.
    fn vint(data: &[u8], at: usize, keep_marker: bool) -> Option<(u64, usize)> {
        let first = *data.get(at)?;
        if first == 0 {
            return None;
        }
        let len = first.leading_zeros() as usize + 1;
        if len > 8 || at + len > data.len() {
            return None;
        }
        let mut value = if keep_marker {
            first as u64
        } else {
            (first & (0xff >> len)) as u64
        };
        for i in 1..len {
            value = (value << 8) | data[at + i] as u64;
        }
        Some((value, len))
    }

    fn uint(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0u64, |acc, byte| (acc << 8) | *byte as u64)
    }

    fn float(bytes: &[u8]) -> Option<f64> {
        match bytes.len() {
            4 => Some(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64),
            8 => Some(f64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ])),
            _ => None,
        }
    }

    pub(super) fn parse(data: &[u8]) -> Option<Probed> {
        let mut probed = Probed::default();
        let mut timecode_scale = 1_000_000f64;
        let mut duration: Option<f64> = None;
        let mut tracks: Vec<Track> = Vec::new();
        walk(data, 0, data.len(), 0, &mut timecode_scale, &mut duration, &mut tracks);

        if let Some(seconds) = duration {
            let ms = (seconds * timecode_scale / 1_000_000.0) as u64;
            probed.duration_ms = (ms > 0).then_some(ms);
        }
        let mut audio_index = 0u32;
        for track in tracks {
            match track.kind {
                1 => probed.video = Some(video_of(&track)),
                2 => {
                    probed.audio_tracks.push(Audio {
                        technical: Some(AudioTechnical {
                            stream_index: Some(track.number.saturating_sub(1) as u32),
                            sample_rate: track.sample_rate,
                            ..AudioTechnical::default()
                        }),
                        index: audio_index,
                        codec: codec_name(&track.codec),
                        channels: track.channels.map(|c| c as u32),
                        language: track.language.clone(),
                        title: track.title.clone(),
                        default: track.default,
                    });
                    audio_index += 1;
                }
                17 => probed.subtitles.push(Subtitle {
                    language: track.language.clone(),
                    codec: codec_name(&track.codec),
                    forced: track.forced,
                }),
                _ => {}
            }
        }
        Some(probed)
    }

    fn video_of(track: &Track) -> Video {
        let transfer = transfer_name(track.transfer);
        let hdr_format = match transfer.as_deref() {
            Some("smpte2084") => Some("hdr10".to_string()),
            Some("arib-std-b67") => Some("hlg".to_string()),
            _ => None,
        };
        let color = Color {
            primaries: primaries_name(track.primaries),
            transfer,
            matrix: matrix_name(track.matrix),
        };
        let frame_rate = track.default_duration_ns
            .filter(|ns| *ns > 0)
            .map(|ns| 1_000_000_000.0 / ns as f64)
            .filter(|fps| fps.is_finite() && *fps > 0.0);
        let aspect_ratio = match (track.display_width.or(track.width), track.display_height.or(track.height)) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some(reduced_ratio(w, h)),
            _ => None,
        };
        Video {
            technical: Some(VideoTechnical {
                stream_index: Some(track.number.saturating_sub(1) as u32),
                aspect_ratio,
                frame_rate: frame_rate.or(track.frame_rate),
                ..VideoTechnical::default()
            }),
            codec: codec_name(&track.codec),
            width: track.width,
            height: track.height,
            hdr: hdr_format.is_some(),
            bit_depth: track.bit_depth.map(|d| d as u32),
            hdr_format,
            color: (!color.primaries.is_none() || !color.transfer.is_none() || !color.matrix.is_none())
                .then_some(color),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        data: &[u8],
        mut at: usize,
        end: usize,
        depth: usize,
        timecode_scale: &mut f64,
        duration: &mut Option<f64>,
        tracks: &mut Vec<Track>,
    ) {
        if depth > 12 {
            return;
        }
        while at < end {
            let Some((id, id_len)) = vint(data, at, true) else { return };
            let Some((size, size_len)) = vint(data, at + id_len, false) else { return };
            let unknown_size = size == (1u64 << (7 * size_len)) - 1;
            let body = at + id_len + size_len;
            // An unknown-size element runs to the end of what was read.
            let body_end = if unknown_size {
                end
            } else {
                body.saturating_add(size as usize).min(end)
            };
            if body > end {
                return;
            }
            if id == TRACK_ENTRY {
                tracks.push(Track::default());
            }
            match id {
                SEGMENT | INFO | TRACKS | TRACK_ENTRY | VIDEO | AUDIO | COLOUR => {
                    walk(data, body, body_end, depth + 1, timecode_scale, duration, tracks)
                }
                0x2ad7b1 => *timecode_scale = uint(&data[body..body_end]).max(1) as f64,
                0x4489 => *duration = float(&data[body..body_end]),
                _ => fill_leaf(data, id, body, body_end, tracks),
            }
            at = body_end;
        }
    }

    fn fill_leaf(data: &[u8], id: u64, body: usize, end: usize, tracks: &mut Vec<Track>) {
        let bytes = &data[body..end];
        let Some(track) = tracks.last_mut() else {
            return;
        };
        match id {
            0xd7 => track.number = uint(bytes),
            0x83 => track.kind = uint(bytes),
            0x86 => track.codec = String::from_utf8_lossy(bytes).trim_end_matches('\0').to_string(),
            0x22b59c => {
                let value = String::from_utf8_lossy(bytes).trim_end_matches('\0').to_string();
                if !value.is_empty() && value != "und" {
                    track.language = Some(value);
                }
            }
            0x536e => track.title = Some(String::from_utf8_lossy(bytes).trim_end_matches('\0').to_string()).filter(|s| !s.is_empty()),
            0x88 => track.default = uint(bytes) == 1,
            0xb0 => track.width = Some(uint(bytes) as u32),
            0xba => track.height = Some(uint(bytes) as u32),
            0x54b0 => track.display_width = Some(uint(bytes) as u32),
            0x54ba => track.display_height = Some(uint(bytes) as u32),
            0x23e383 => track.default_duration_ns = Some(uint(bytes)),
            0x9f => track.channels = Some(uint(bytes) as u32),
            0xb5 => track.sample_rate = Some(float(bytes).map(|f| f as u32).unwrap_or(uint(bytes) as u32)),
            0x6264 => track.bit_depth = Some(uint(bytes) as u32),
            0x55b1 => track.primaries = Some(uint(bytes)),
            0x55ba => track.transfer = Some(uint(bytes)),
            0x55bb => track.matrix = Some(uint(bytes)),
            0x55ab => track.forced = uint(bytes) == 1,
            _ => {}
        }
    }

    pub(super) fn reduced_ratio(width: u32, height: u32) -> String {
        fn gcd(a: u32, b: u32) -> u32 {
            if b == 0 { a } else { gcd(b, a % b) }
        }
        let divisor = gcd(width, height).max(1);
        format!("{}:{}", width / divisor, height / divisor)
    }

    fn codec_name(id: &str) -> String {
        match id {
            "V_MPEG4/ISO/AVC" => "h264",
            "V_MPEGH/ISO/HEVC" => "hevc",
            "V_AV1" => "av1",
            "V_VP9" => "vp9",
            "V_VP8" => "vp8",
            "V_MPEG2" => "mpeg2",
            "A_AAC" | "A_AAC/MPEG4/LC" => "aac",
            "A_AC3" => "ac3",
            "A_EAC3" => "eac3",
            "A_DTS" => "dts",
            "A_TRUEHD" => "truehd",
            "A_FLAC" => "flac",
            "A_OPUS" => "opus",
            "A_VORBIS" => "vorbis",
            "A_MPEG/L3" => "mp3",
            "S_TEXT/UTF8" | "S_TEXT/ASCII" => "subrip",
            "S_TEXT/ASS" | "S_ASS" => "ass",
            "S_HDMV/PGS" => "hdmv_pgs_subtitle",
            _ => id,
        }
        .to_string()
    }

    fn primaries_name(value: Option<u64>) -> Option<String> {
        match value? {
            1 => Some("bt709"),
            9 => Some("bt2020"),
            _ => None,
        }
        .map(str::to_string)
    }

    fn transfer_name(value: Option<u64>) -> Option<String> {
        match value? {
            1 => Some("bt709"),
            16 => Some("smpte2084"),
            18 => Some("arib-std-b67"),
            _ => None,
        }
        .map(str::to_string)
    }

    fn matrix_name(value: Option<u64>) -> Option<String> {
        match value? {
            1 => Some("bt709"),
            9 => Some("bt2020nc"),
            _ => None,
        }
        .map(str::to_string)
    }
}

// ---------------------------------------------------------------------------
// MP4 / ISO base media

mod mp4 {
    use super::{Audio, Probed, Subtitle, Video};

    fn be32(bytes: &[u8]) -> u32 {
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Walk boxes between `at` and `end`, answering each (type, payload).
    fn boxes<'a>(data: &'a [u8], at: usize, end: usize) -> Vec<([u8; 4], &'a [u8])> {
        let mut out = Vec::new();
        let mut cursor = at.min(end);
        while cursor + 8 <= end {
            let size = be32(&data[cursor..cursor + 4]) as usize;
            let mut kind = [0u8; 4];
            kind.copy_from_slice(&data[cursor + 4..cursor + 8]);
            let (header, size) = if size == 1 {
                if cursor + 16 > end {
                    break;
                }
                let large = u64::from_be_bytes(data[cursor + 8..cursor + 16].try_into().unwrap());
                (16, large as usize)
            } else if size == 0 {
                (8, end - cursor - 8)
            } else {
                (8, size.saturating_sub(8))
            };
            if size == 0 || cursor + header + size > end {
                break;
            }
            out.push((kind, &data[cursor + header..cursor + header + size]));
            cursor += header + size;
        }
        out
    }

    fn find<'a>(list: &[([u8; 4], &'a [u8])], kind: &[u8; 4]) -> Option<&'a [u8]> {
        list.iter().find(|(k, _)| k == kind).map(|(_, body)| *body)
    }

    fn parse_moov(moov: &[u8]) -> Probed {
        let mut probed = Probed::default();
        let children = boxes(moov, 0, moov.len());
        if let Some(mvhd) = find(&children, b"mvhd") {
            probed.duration_ms = mvhd_duration(mvhd);
        }
        let mut audio_index = 0u32;
        for (kind, body) in &children {
            if kind != b"trak" {
                continue;
            }
            if let Some(track) = track_of(body) {
                match track.kind {
                    Kind::Video => probed.video = Some(track.video),
                    Kind::Audio => {
                        let mut audio = track.audio;
                        audio.index = audio_index;
                        audio_index += 1;
                        probed.audio_tracks.push(audio);
                    }
                    Kind::Subtitle => probed.subtitles.push(Subtitle {
                        language: track.language,
                        codec: track.codec,
                        forced: false,
                    }),
                    Kind::Other => {}
                }
            }
        }
        probed
    }

    pub(super) fn parse(data: &[u8]) -> Option<Probed> {
        let top = boxes(data, 0, data.len());
        let moov = find(&top, b"moov")?;
        let probed = parse_moov(moov);
        probed.described().then_some(probed)
    }

    /// The head held no `moov` (it follows the media data): find the box by its
    /// type in the tail bytes and parse from its payload.
    pub(super) fn parse_moov_scan(tail: &[u8]) -> Option<Probed> {
        let at = find_bytes(tail, b"moov")?;
        let start = at.checked_sub(4)?;
        let size = be32(&tail[start..start + 4]) as usize;
        if size < 8 || start + size > tail.len() {
            return None;
        }
        let probed = parse_moov(&tail[start + 8..start + size]);
        probed.described().then_some(probed)
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|window| window == needle)
    }

    fn mvhd_duration(mvhd: &[u8]) -> Option<u64> {
        let version = *mvhd.first()?;
        let (timescale, duration) = if version == 1 {
            (
                be32(mvhd.get(20..24)?),
                u64::from_be_bytes(mvhd.get(24..32)?.try_into().ok()?),
            )
        } else {
            (be32(mvhd.get(12..16)?), be32(mvhd.get(16..20)?) as u64)
        };
        if timescale == 0 {
            return None;
        }
        let ms = duration.saturating_mul(1000) / timescale as u64;
        (ms > 0).then_some(ms)
    }

    enum Kind {
        Video,
        Audio,
        Subtitle,
        Other,
    }

    struct Track {
        kind: Kind,
        codec: String,
        language: Option<String>,
        video: Video,
        audio: Audio,
    }

    fn track_of(trak: &[u8]) -> Option<Track> {
        let children = boxes(trak, 0, trak.len());
        let (width, height) = find(&children, b"tkhd").map(tkhd_size).unwrap_or((None, None));
        let mdia = find(&children, b"mdia")?;
        let inner = boxes(mdia, 0, mdia.len());
        let language = find(&inner, b"mdhd").and_then(mdhd_language);
        let handler = find(&inner, b"hdlr").and_then(handler_kind)?;
        let stbl = find(&inner, b"minf")
            .and_then(|minf| boxes(minf, 0, minf.len()).into_iter().find(|(k, _)| k == b"stbl"))
            .map(|(_, body)| body)?;
        let stsd = find(&boxes(stbl, 0, stbl.len()), b"stsd")?;
        let entries = boxes(stsd, 8, stsd.len());
        let (codec, channels) = entries
            .first()
            .map(|(kind, body)| (sample_codec(kind), sample_channels(kind, body)))
            .unwrap_or(("unknown".to_string(), None));

        let kind = match &handler {
            b"vide" => Kind::Video,
            b"soun" => Kind::Audio,
            b"subt" | b"sbtl" => Kind::Subtitle,
            _ => Kind::Other,
        };
        Some(Track {
            kind,
            codec: codec.clone(),
            language: language.clone(),
            video: Video {
                codec: codec.clone(),
                width,
                height,
                ..Video::default()
            },
            audio: Audio {
                index: 0,
                codec,
                channels,
                language: language.clone(),
                ..Audio::default()
            },
        })
    }

    fn tkhd_size(tkhd: &[u8]) -> (Option<u32>, Option<u32>) {
        let at = tkhd.len().checked_sub(8);
        match at {
            Some(at) => (
                Some((be32(&tkhd[at..at + 4]) >> 16) as u32),
                Some((be32(&tkhd[at + 4..at + 8]) >> 16) as u32),
            ),
            None => (None, None),
        }
    }

    fn mdhd_language(mdhd: &[u8]) -> Option<String> {
        let version = *mdhd.first()?;
        let at = if version == 1 { 32 } else { 20 };
        let packed = u16::from_be_bytes(mdhd.get(at..at + 2)?.try_into().ok()?);
        let mut code = String::new();
        for shift in [10, 5, 0] {
            let c = ((packed >> shift) & 0x1f) as u8;
            if !(1..=26).contains(&c) {
                return None;
            }
            code.push((c + 0x60) as char);
        }
        (code != "und").then_some(code)
    }

    fn handler_kind(hdlr: &[u8]) -> Option<[u8; 4]> {
        // version/flags (4), pre_defined (4), handler_type (4)
        let mut kind = [0u8; 4];
        kind.copy_from_slice(hdlr.get(8..12)?);
        Some(kind)
    }

    fn sample_codec(kind: &[u8; 4]) -> String {
        match kind {
            b"avc1" | b"avc3" => "h264",
            b"hev1" | b"hvc1" => "hevc",
            b"av01" => "av1",
            b"vp09" => "vp9",
            b"mp4v" => "mpeg4",
            b"mp4a" => "aac",
            b"ac-3" => "ac3",
            b"ec-3" => "eac3",
            b"Opus" => "opus",
            b"fLaC" => "flac",
            b"alac" => "alac",
            _ => return String::from_utf8_lossy(kind).to_string(),
        }
        .to_string()
    }

    fn sample_channels(kind: &[u8; 4], body: &[u8]) -> Option<u32> {
        if !matches!(kind, b"mp4a" | b"ac-3" | b"ec-3" | b"Opus" | b"fLaC" | b"alac") {
            return None;
        }
        // 8 reserved + 2 data_reference_index, then 8 more, then channelcount.
        let at = 16;
        body.get(at..at + 2)
            .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]) as u32)
            .filter(|count| *count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mkv_vint_size(mut value: u64) -> Vec<u8> {
        let mut out = vec![0u8];
        for len in 1..=8usize {
            if value < (1u64 << (7 * len)) {
                let mut bytes = vec![0u8; len];
                for i in (0..len).rev() {
                    bytes[i] = (value & 0xff) as u8;
                    value >>= 8;
                }
                bytes[0] |= 0x80 >> (len - 1);
                out = bytes;
                break;
            }
        }
        out
    }

    fn mkv_element(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend(mkv_vint_size(body.len() as u64));
        out.extend_from_slice(body);
        out
    }

    fn mkv_uint(id: &[u8], value: u64) -> Vec<u8> {
        let mut bytes = value.to_be_bytes().to_vec();
        while bytes.len() > 1 && bytes[0] == 0 {
            bytes.remove(0);
        }
        mkv_element(id, &bytes)
    }

    fn mkv_string(id: &[u8], value: &str) -> Vec<u8> {
        mkv_element(id, value.as_bytes())
    }

    #[test]
    fn a_matroska_header_describes_duration_video_and_audio() {
        let info = mkv_element(&[0x15, 0x49, 0xa9, 0x66], &[
            mkv_uint(&[0x2a, 0xd7, 0xb1], 1_000_000),
            mkv_element(&[0x44, 0x89], &3_600_000.0f64.to_be_bytes()),
        ].concat());
        let video = mkv_element(&[0xe0], &[
            mkv_uint(&[0xb0], 3840),
            mkv_uint(&[0xba], 2160),
        ].concat());
        let entry_video = mkv_element(&[0xae], &[
            mkv_uint(&[0xd7], 1),
            mkv_uint(&[0x83], 1),
            mkv_string(&[0x86], "V_MPEGH/ISO/HEVC"),
            video,
        ].concat());
        let entry_audio = mkv_element(&[0xae], &[
            mkv_uint(&[0xd7], 2),
            mkv_uint(&[0x83], 2),
            mkv_string(&[0x86], "A_AC3"),
            mkv_string(&[0x22, 0xb5, 0x9c], "jpn"),
            mkv_uint(&[0x9f], 6),
        ].concat());
        let tracks = mkv_element(&[0x16, 0x54, 0xae, 0x6b], &[entry_video, entry_audio].concat());
        let segment = mkv_element(&[0x18, 0x53, 0x80, 0x67], &[info, tracks].concat());
        let header = mkv_element(&[0x1a, 0x45, 0xdf, 0xa3], &[]);
        let file = [header, segment].concat();

        let probed = mkv::parse(&file).expect("parsed");
        assert_eq!(probed.duration_ms, Some(3_600_000));
        let video = probed.video.expect("video");
        assert_eq!(video.codec, "hevc");
        assert_eq!(video.width, Some(3840));
        assert_eq!(video.height, Some(2160));
        assert_eq!(probed.audio_tracks.len(), 1);
        assert_eq!(probed.audio_tracks[0].codec, "ac3");
        assert_eq!(probed.audio_tracks[0].language.as_deref(), Some("jpn"));
        assert_eq!(probed.audio_tracks[0].channels, Some(6));
    }

    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn an_mp4_moov_describes_duration_and_dimensions() {
        let mvhd = mp4_box(b"mvhd", &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0x00, 0x00, 0x03, 0xe8, // timescale 1000
            0x00, 0x0d, 0xbb, 0xa0, // duration 900000
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let tkhd = mp4_box(b"tkhd", &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0x0f, 0x00, 0x00, 0x00, // 3840.0
            0x08, 0x70, 0x00, 0x00]); // 2160.0
        let mdhd = mp4_box(b"mdhd", &[0, 0, 0, 0, 0, 0, 0, 0,
            0x00, 0x00, 0x03, 0xe8, 0x00, 0x0d, 0xbb, 0xa0,
            0x15, 0xc7, 0, 0]); // language "eng"
        let hdlr = mp4_box(b"hdlr", &[0, 0, 0, 0, 0, 0, 0, 0, b'v', b'i', b'd', b'e',
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let avc1 = mp4_box(b"avc1", &[0, 0, 0, 0, 0, 0, 0, 1,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0x0f, 0x00, 0x08, 0x70]);
        let stsd = mp4_box(b"stsd", &[0, 0, 0, 0, 0, 0, 0, 1].iter().copied().chain(avc1).collect::<Vec<_>>());
        let stbl = mp4_box(b"stbl", &stsd);
        let minf = mp4_box(b"minf", &stbl);
        let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
        let trak = mp4_box(b"trak", &[tkhd, mdia].concat());
        let moov = mp4_box(b"moov", &[mvhd, trak].concat());
        let ftyp = mp4_box(b"ftyp", b"isomisom");

        let file = [ftyp, moov].concat();
        let probed = mp4::parse(&file).expect("moov in head");
        assert_eq!(probed.duration_ms, Some(900_000));
        let video = probed.video.expect("video");
        assert_eq!(video.codec, "h264");
        assert_eq!(video.width, Some(3840));
        assert_eq!(video.height, Some(2160));
    }
}
