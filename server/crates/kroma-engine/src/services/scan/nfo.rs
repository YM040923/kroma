//! Read the NFO a media folder already carries.
//!
//! The scan still derives identity from the filename, but an NFO is the
//! operator's explicit answer and must win: title, year, show, season/episode,
//! genres, certification and provider ids. The writer in `enrich::nfo` leaves
//! an existing NFO alone, so these values survive a later TMDB pass.

use std::path::Path;

use crate::domain::naming::Parsed;

const MAX_NFO_BYTES: u64 = 256 * 1024;

#[derive(Debug, Default, Clone)]
pub(crate) struct Nfo {
    pub(crate) title: Option<String>,
    pub(crate) original_title: Option<String>,
    pub(crate) show_title: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) year: Option<u32>,
    pub(crate) season: Option<u32>,
    pub(crate) episode: Option<u32>,
    pub(crate) plot: Option<String>,
    pub(crate) genres: Vec<String>,
    pub(crate) studios: Vec<String>,
    pub(crate) certification: Option<String>,
    pub(crate) premiered: Option<String>,
    pub(crate) rating: Option<f32>,
    pub(crate) tmdb_id: Option<u64>,
    pub(crate) imdb_id: Option<String>,
    pub(crate) set: Option<String>,
}

impl Nfo {
    pub(crate) fn metadata(&self, is_show: bool) -> crate::model::Metadata {
        crate::model::Metadata {
            provider: "tmdb",
            tmdb_id: self.tmdb_id.unwrap_or(0),
            imdb_id: self.imdb_id.clone(),
            title: self.title.clone().or_else(|| self.original_title.clone()),
            tagline: None,
            overview: self.plot.clone(),
            release_date: self.premiered.clone(),
            genres: self.genres.clone(),
            studios: self.studios.clone(),
            backdrops: Vec::new(),
            tmdb_genre_ids: Vec::new(),
            rating: self.rating,
            certification: self.certification.clone(),
            collection_id: None,
            collection_name: self.set.clone(),
            poster_url: None,
            backdrop_url: None,
            logo_url: None,
            theme_url: None,
            cast: Vec::new(),
            crew: Vec::new(),
            keywords: Vec::new(),
            tvdb_id: None,
            tmdb_url: self
                .tmdb_id
                .map(|id| {
                    format!(
                        "https://www.themoviedb.org/{}/{id}",
                        if is_show { "tv" } else { "movie" }
                    )
                })
                .unwrap_or_default(),
        }
    }

    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.original_title.is_none()
            && self.show_title.is_none()
            && self.language.is_none()
            && self.year.is_none()
            && self.season.is_none()
            && self.episode.is_none()
            && self.plot.is_none()
            && self.genres.is_empty()
            && self.studios.is_empty()
            && self.certification.is_none()
            && self.premiered.is_none()
            && self.rating.is_none()
            && self.tmdb_id.is_none()
            && self.imdb_id.is_none()
            && self.set.is_none()
    }
}

/// Apply the NFO beside `media` to a parsed identity. A malformed or absent NFO
/// leaves the filename parse untouched.
pub(super) fn apply(parsed: Parsed, media: &Path) -> Parsed {
    let Some(episode) = read_episode(media) else {
        return parsed;
    };
    let show = show_nfo(media);
    match parsed {
        Parsed::Movie { title, year } => {
            let show_title = episode
                .show_title
                .clone()
                .or_else(|| show.as_ref().and_then(|nfo| nfo.title.clone()));
            match (show_title, episode.season, episode.episode) {
                (Some(show_title), Some(season), Some(number)) => Parsed::Episode {
                    show_title,
                    show_year: show.as_ref().and_then(|nfo| nfo.year).or(episode.year),
                    season,
                    episode: number,
                    episode_end: None,
                    episode_title: episode.title.or(episode.original_title),
                },
                _ => Parsed::Movie {
                    title: episode.title.or(episode.original_title).unwrap_or(title),
                    year: episode.year.or(year),
                },
            }
        }
        Parsed::Episode {
            show_title,
            show_year,
            season,
            episode: number,
            episode_end,
            episode_title,
        } => Parsed::Episode {
            show_title: episode
                .show_title
                .or_else(|| show.as_ref().and_then(|nfo| nfo.title.clone()))
                .unwrap_or(show_title),
            show_year: show
                .as_ref()
                .and_then(|nfo| nfo.year)
                .or(episode.year)
                .or(show_year),
            season: episode.season.unwrap_or(season),
            episode: episode.episode.unwrap_or(number),
            episode_end,
            episode_title: episode.title.or(episode.original_title).or(episode_title),
        },
    }
}

fn read_episode(media: &Path) -> Option<Nfo> {
    let stem = media.with_extension("nfo");
    let parent = media.parent()?;
    for candidate in [stem, parent.join("movie.nfo"), parent.join("episode.nfo")] {
        if let Some(nfo) = parse_file(&candidate) {
            if !nfo.is_empty() {
                return Some(nfo);
            }
        }
    }
    None
}

fn show_nfo(media: &Path) -> Option<Nfo> {
    let parent = media.parent()?;
    let show_dir = if parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(crate::domain::naming::is_season_folder)
    {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    parse_file(&show_dir.join("tvshow.nfo")).filter(|nfo| !nfo.is_empty())
}

fn parse_file(path: &Path) -> Option<Nfo> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_NFO_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    parse(&String::from_utf8_lossy(&bytes))
}

pub(crate) fn read_for_media(media: &Path, is_show: bool) -> Option<Nfo> {
    if is_show {
        show_nfo(media)
    } else {
        read_episode(media)
    }
}

pub(crate) fn overlay_metadata(meta: &mut crate::model::Metadata, nfo: &Nfo) {
    if let Some(title) = nfo.title.as_ref().or(nfo.original_title.as_ref()) {
        meta.title = Some(title.clone());
    }
    if let Some(plot) = &nfo.plot {
        meta.overview = Some(plot.clone());
    }
    if !nfo.genres.is_empty() {
        meta.genres.clone_from(&nfo.genres);
    }
    if !nfo.studios.is_empty() {
        meta.studios.clone_from(&nfo.studios);
    }
    if let Some(certification) = &nfo.certification {
        meta.certification = Some(certification.clone());
    }
    if let Some(set) = &nfo.set {
        // The operator's set name wins; a provider id pointing at a different
        // series would then be a lie, so it goes.
        if meta.collection_name.as_deref() != Some(set.as_str()) {
            meta.collection_id = None;
        }
        meta.collection_name = Some(set.clone());
    }
    if let Some(rating) = nfo.rating {
        meta.rating = Some(rating);
    }
    if let Some(premiered) = &nfo.premiered {
        meta.release_date = Some(premiered.clone());
    }
    if let Some(tmdb_id) = nfo.tmdb_id {
        meta.tmdb_id = tmdb_id;
    }
    if let Some(imdb_id) = &nfo.imdb_id {
        meta.imdb_id = Some(imdb_id.clone());
    }
}

fn parse(xml: &str) -> Option<Nfo> {
    let lower = xml.to_ascii_lowercase();
    let nfo = Nfo {
        title: text(xml, &lower, "title"),
        original_title: text(xml, &lower, "originaltitle"),
        show_title: text(xml, &lower, "showtitle"),
        language: text(xml, &lower, "language"),
        year: number(text(xml, &lower, "year")),
        season: number(text(xml, &lower, "season")),
        episode: number(text(xml, &lower, "episode")),
        plot: text(xml, &lower, "plot"),
        genres: texts(xml, &lower, "genre"),
        studios: texts(xml, &lower, "studio"),
        certification: text(xml, &lower, "mpaa")
            .or_else(|| text(xml, &lower, "certification")),
        premiered: text(xml, &lower, "premiered").or_else(|| text(xml, &lower, "releasedate")),
        rating: text(xml, &lower, "rating").and_then(|value| value.parse().ok()),
        tmdb_id: unique_id(xml, &lower, "tmdb").and_then(|id| id.parse().ok()),
        imdb_id: unique_id(xml, &lower, "imdb"),
        set: nested_text(xml, &lower, "set", "name"),
    };
    if nfo.title.is_none() && nfo.original_title.is_none() && nfo.show_title.is_none() {
        // A `<movie>`/`<tvshow>` wrapper with no title is still a useful
        // provider-id source; keep it. An entirely empty document is not.
        if nfo.tmdb_id.is_none() && nfo.imdb_id.is_none() && nfo.plot.is_none() {
            return None;
        }
    }
    Some(nfo)
}

fn text(xml: &str, lower: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = lower.find(&open)?;
    let after = start + open.len();
    let end = lower[after..].find(&close)? + after;
    let raw = &xml[after..end];
    let raw = raw.trim();
    let raw = raw
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
        .unwrap_or(raw)
        .trim();
    (!raw.is_empty()).then(|| unescape(raw))
}

fn texts(xml: &str, lower: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find(&open) {
        let start = at + found;
        let after = start + open.len();
        let Some(end) = lower[after..].find(&close).map(|n| after + n) else {
            break;
        };
        let value = unescape(xml[after..end].trim());
        if !value.is_empty() {
            out.push(value);
        }
        at = end + close.len();
    }
    out
}

fn nested_text(xml: &str, lower: &str, outer: &str, inner: &str) -> Option<String> {
    let start = lower.find(&format!("<{outer}"))?;
    let end = lower[start..].find(&format!("</{outer}>"))? + start;
    text(&xml[start..end], &lower[start..end], inner)
}

fn unique_id(xml: &str, lower: &str, kind: &str) -> Option<String> {
    let open = "<uniqueid";
    let close = "</uniqueid>";
    let mut at = 0;
    while let Some(found) = lower[at..].find(open) {
        let start = at + found;
        let after = lower[start..].find('>')? + start + 1;
        let end = lower[after..].find(close)? + after;
        let attributes = &lower[start..after];
        if attributes.contains(&format!("type=\"{kind}\"")) {
            let id = xml[after..end].trim();
            return (!id.is_empty()).then(|| id.to_string());
        }
        at = end + close.len();
    }
    None
}

fn number<T: std::str::FromStr>(value: Option<String>) -> Option<T> {
    value?.trim().parse().ok()
}

fn unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed_movie() -> Parsed {
        Parsed::Movie {
            title: "Wrong Name".into(),
            year: Some(1999),
        }
    }

    #[test]
    fn a_movie_nfo_wins_over_the_filename() {
        let dir = kroma_testing::temp_dir("nfo-movie");
        let media = dir.path().join("Wrong.Name.mkv");
        std::fs::write(dir.path().join("Wrong.Name.nfo"), r#"<movie><title>Right Name</title><year>2001</year><genre>Drama</genre><mpaa>PG-13</mpaa><uniqueid type="tmdb" default="true">42</uniqueid><set><name>A Collection</name></set></movie>"#).unwrap();
        let parsed = apply(parsed_movie(), &media);
        assert_eq!(
            parsed,
            Parsed::Movie {
                title: "Right Name".into(),
                year: Some(2001)
            }
        );
        let nfo = read_episode(&media).unwrap();
        assert_eq!(nfo.genres, vec!["Drama"]);
        assert_eq!(nfo.certification.as_deref(), Some("PG-13"));
        assert_eq!(nfo.tmdb_id, Some(42));
        assert_eq!(nfo.set.as_deref(), Some("A Collection"));
    }

    #[test]
    fn an_episode_takes_the_show_nfo_and_its_own_number() {
        let dir = kroma_testing::temp_dir("nfo-episode");
        let show = dir.path().join("Show");
        let season = show.join("Season 1");
        std::fs::create_dir_all(&season).unwrap();
        std::fs::write(
            show.join("tvshow.nfo"),
            "<tvshow><title>Show Name</title><year>2019</year></tvshow>",
        )
        .unwrap();
        let media = season.join("whatever.mkv");
        std::fs::write(
            season.join("whatever.nfo"),
            "<episodedetails><title>Pilot</title><season>2</season><episode>3</episode></episodedetails>",
        )
        .unwrap();
        let parsed = apply(
            Parsed::Episode {
                show_title: "wrong".into(),
                show_year: None,
                season: 1,
                episode: 1,
                episode_end: None,
                episode_title: None,
            },
            &media,
        );
        assert_eq!(
            parsed,
            Parsed::Episode {
                show_title: "Show Name".into(),
                show_year: Some(2019),
                season: 2,
                episode: 3,
                episode_end: None,
                episode_title: Some("Pilot".into()),
            }
        );
    }

    #[test]
    fn an_absent_or_empty_nfo_changes_nothing() {
        let dir = kroma_testing::temp_dir("nfo-absent");
        let media = dir.path().join("Film.mkv");
        assert_eq!(apply(parsed_movie(), &media), parsed_movie());
        std::fs::write(dir.path().join("Film.nfo"), "not xml").unwrap();
        assert_eq!(apply(parsed_movie(), &media), parsed_movie());
    }

    #[test]
    fn xml_entities_and_cdata_are_read_as_text() {
        let xml = "<movie><title><![CDATA[A & B]]></title><plot>x &amp; y</plot></movie>";
        let nfo = parse(xml).unwrap();
        assert_eq!(nfo.title.as_deref(), Some("A & B"));
        assert_eq!(nfo.plot.as_deref(), Some("x & y"));
    }
}
