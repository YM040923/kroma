use crate::domain::metadata::Metadata;

use super::super::common::{build_cast, build_crew};
use super::certification::country_of;
use super::details_json::{collect_keywords, pick_backdrops, pick_logo, Details};
use super::{api, tmdb_json, Target, IMG, MAX_CAST, MAX_CREW};

pub(super) fn fetch_details(
    api_key: &str,
    language: &str,
    household: &str,
    target: Target,
    id: u64,
) -> Result<Metadata, ()> {
    // Base language code (e.g. "fr" from "fr-FR") for picking a localized logo.
    let lang2 = language.split('-').next().unwrap_or("en");
    let detail_params = vec![
        ("language", language.to_string()),
        (
            "append_to_response",
            format!(
                "external_ids,credits,images,keywords,{}",
                target.certification_append()
            ),
        ),
        ("include_image_language", format!("{lang2},en,null")),
    ];
    let d: Details = tmdb_json(
        "metadata.details",
        &format!("{}/{}/{id}", api(), target.detail_path()),
        api_key,
        &detail_params,
    )?;
    let country = country_of(language, household);
    Ok(metadata_from(d, target, id, lang2, &country))
}

fn metadata_from(d: Details, target: Target, id: u64, lang2: &str, country: &str) -> Metadata {
    let ext = d.external_ids;
    let imdb_id = d
        .imdb_id
        .or_else(|| ext.as_ref().and_then(|e| e.imdb_id.clone()))
        .filter(|s| !s.is_empty());
    // TVDB series id (TV only) keys the theme-song lookup during enrichment.
    let tvdb_id = ext.as_ref().and_then(|e| e.tvdb_id).filter(|&id| id > 0);

    // Cast + crew share the appended `credits` block; TV creators come from
    // the top-level `created_by`.
    let (raw_cast, raw_crew) = d.credits.map(|c| (c.cast, c.crew)).unwrap_or_default();
    let cast = build_cast(raw_cast, MAX_CAST, false);
    let crew = build_crew(raw_crew, d.created_by, MAX_CREW);
    let mut studios = Vec::new();
    for company in d.production_companies.into_iter().chain(d.networks) {
        let name = company.name.trim();
        if !name.is_empty() && !studios.iter().any(|stored| stored == name) {
            studios.push(name.to_string());
        }
    }

    // A collection name is the provider's, in the language this lookup asked
    // for; an untitled collection is no collection.
    let collection = d.belongs_to_collection.and_then(|c| {
        let name = c.name.trim();
        (!name.is_empty()).then(|| (Some(c.id), Some(name.to_string())))
    });
    let (collection_id, collection_name) = collection.unwrap_or((None, None));

    let poster_path = d
        .images
        .as_ref()
        .and_then(|images| super::details_json::pick_poster(&images.posters, lang2))
        .or(d.poster_path);
    let backdrop_path = d
        .images
        .as_ref()
        .and_then(|images| super::details_json::pick_backdrop(&images.backdrops, lang2))
        .or(d.backdrop_path);

    // Gallery artwork rides beside the primary backdrop: best-voted first, and
    // never the hero image itself, so a client can show the whole set without
    // repeating its first slide.
    let backdrop_url = backdrop_path.map(|p| format!("{IMG}/w1280{p}"));
    let mut backdrops: Vec<String> = d
        .images
        .as_ref()
        .map(|i| {
            pick_backdrops(&i.backdrops)
                .into_iter()
                .map(|p| format!("{IMG}/w1280{p}"))
                .collect()
        })
        .unwrap_or_default();
    if let Some(primary) = backdrop_url.as_deref() {
        backdrops.retain(|url| url != primary);
    }

    Metadata {
        provider: "tmdb",
        tmdb_id: d.id,
        imdb_id,
        title: d.title.or(d.name),
        tagline: d.tagline.filter(|s| !s.is_empty()),
        overview: d.overview.filter(|s| !s.is_empty()),
        release_date: d
            .release_date
            .or(d.first_air_date)
            .filter(|s| !s.is_empty()),
        tmdb_genre_ids: d.genres.iter().map(|g| g.id).collect(),
        genres: d.genres.into_iter().map(|g| g.name).collect(),
        studios,
        backdrops,
        keywords: d.keywords.map(collect_keywords).unwrap_or_default(),
        rating: d.vote_average.filter(|v| *v > 0.0),
        certification: d
            .release_dates
            .or(d.content_ratings)
            .and_then(|c| c.preferring(country)),
        collection_id,
        collection_name,
        poster_url: poster_path.map(|p| format!("{IMG}/w500{p}")),
        backdrop_url,
        logo_url: d
            .images
            .as_ref()
            .and_then(|i| pick_logo(&i.logos, lang2))
            .map(|p| format!("{IMG}/w500{p}")),
        cast,
        crew,
        // Resolved later (a disk download) by `infra::theme`; this lookup only
        // carries the TVDB id it needs.
        theme_url: None,
        tvdb_id,
        tmdb_url: format!("https://www.themoviedb.org/{}/{id}", target.web_kind()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Network is not exercised in tests; we validate the JSON→Metadata mapping
    // against representative TMDB payloads instead.
    #[test]
    fn a_movie_keeps_the_genre_ids_beside_the_names_they_came_with() {
        let raw = r#"{
            "id": 542178,
            "title": "The French Dispatch",
            "genres": [{"id": 35, "name": "Comedy"}, {"id": 18, "name": "Drama"}]
        }"#;
        let d: Details = serde_json::from_str(raw).unwrap();

        let meta = metadata_from(d, Target::Movie, 542178, "en", "US");

        assert_eq!(meta.tmdb_genre_ids, vec![35, 18]);
        assert_eq!(meta.genres, vec!["Comedy", "Drama"]);
    }

    #[test]
    fn a_movie_keeps_the_series_it_belongs_to() {
        let raw = r#"{
            "id": 11,
            "title": "Star Wars",
            "belongs_to_collection": {"id": 10, "name": "Star Wars Collection"}
        }"#;
        let d: Details = serde_json::from_str(raw).unwrap();

        let meta = metadata_from(d, Target::Movie, 11, "en", "US");

        assert_eq!(meta.collection_id, Some(10));
        assert_eq!(
            meta.collection_name.as_deref(),
            Some("Star Wars Collection")
        );
    }

    #[test]
    fn a_show_keeps_the_tv_only_genre_ids() {
        let raw = r#"{
            "id": 1399,
            "name": "Game of Thrones",
            "genres": [{"id": 10765, "name": "Sci-Fi & Fantasy"}, {"id": 18, "name": "Drama"}]
        }"#;
        let d: Details = serde_json::from_str(raw).unwrap();

        let meta = metadata_from(d, Target::Tv, 1399, "en", "US");

        assert_eq!(meta.tmdb_genre_ids, vec![10765, 18]);
    }

    #[test]
    fn a_movie_takes_its_age_rating_from_the_release_dates_of_the_asked_country() {
        let raw = r#"{
            "id": 603,
            "title": "The Matrix",
            "release_dates": {"results": [
                {"iso_3166_1": "FR", "release_dates": [{"certification": "12"}]},
                {"iso_3166_1": "US", "release_dates": [{"certification": "R"}]}
            ]}
        }"#;
        let d: Details = serde_json::from_str(raw).unwrap();

        let meta = metadata_from(d, Target::Movie, 603, "fr", "FR");

        assert_eq!(meta.certification.as_deref(), Some("12"));
    }

    #[test]
    fn a_show_takes_its_age_rating_from_the_content_ratings() {
        let raw = r#"{
            "id": 1396,
            "name": "Breaking Bad",
            "content_ratings": {"results": [{"iso_3166_1": "US", "rating": "TV-MA"}]}
        }"#;
        let d: Details = serde_json::from_str(raw).unwrap();

        let meta = metadata_from(d, Target::Tv, 1396, "en", "US");

        assert_eq!(meta.certification.as_deref(), Some("TV-MA"));
    }

    #[test]
    fn a_title_no_provider_rated_carries_no_age_rating() {
        let d: Details = serde_json::from_str(r#"{"id": 1, "title": "X"}"#).unwrap();

        let meta = metadata_from(d, Target::Movie, 1, "en", "US");

        assert!(meta.certification.is_none());
    }

    #[test]
    fn a_title_with_no_genres_carries_no_genre_ids() {
        let d: Details = serde_json::from_str(r#"{"id": 1, "title": "X"}"#).unwrap();

        let meta = metadata_from(d, Target::Movie, 1, "en", "US");

        assert!(meta.tmdb_genre_ids.is_empty());
        assert!(meta.genres.is_empty());
    }

    #[test]
    fn a_poster_prefers_configured_language_then_votes_and_size() {
        let raw = r#"{
            "id": 1,
            "title": "X",
            "poster_path": "/fallback.jpg",
            "images": {"posters": [
                {"file_path": "/en.jpg", "iso_639_1": "en", "vote_average": 9.5, "width": 2000, "height": 3000},
                {"file_path": "/zh-small.jpg", "iso_639_1": "zh", "vote_average": 3.0, "width": 500, "height": 750},
                {"file_path": "/zh-large.jpg", "iso_639_1": "zh", "vote_average": 3.0, "width": 1000, "height": 1500}
            ]}
        }"#;
        let details: Details = serde_json::from_str(raw).unwrap();
        let meta = metadata_from(details, Target::Movie, 1, "zh", "CN");
        assert_eq!(
            meta.poster_url.as_deref(),
            Some("https://image.tmdb.org/t/p/w500/zh-large.jpg")
        );
    }

    #[test]
    fn the_gallery_is_best_voted_first_and_never_repeats_the_primary_backdrop() {
        let raw = r#"{
            "id": 603,
            "title": "The Matrix",
            "backdrop_path": "/hero.jpg",
            "images": {"backdrops": [
                {"file_path": "/hero.jpg", "vote_average": 9.0},
                {"file_path": "/second.jpg", "vote_average": 5.0},
                {"file_path": "/third.jpg", "vote_average": 7.5}
            ]}
        }"#;
        let d: Details = serde_json::from_str(raw).unwrap();

        let meta = metadata_from(d, Target::Movie, 603, "en", "US");

        assert_eq!(
            meta.backdrop_url.as_deref(),
            Some("https://image.tmdb.org/t/p/w1280/hero.jpg")
        );
        assert_eq!(
            meta.backdrops,
            vec![
                "https://image.tmdb.org/t/p/w1280/third.jpg".to_string(),
                "https://image.tmdb.org/t/p/w1280/second.jpg".to_string(),
            ]
        );
    }

    #[test]
    fn a_title_without_gallery_images_carries_no_backdrops() {
        let d =
            serde_json::from_str(r#"{"id": 1, "title": "X", "backdrop_path": "/a.jpg"}"#).unwrap();

        let meta = metadata_from(d, Target::Movie, 1, "en", "US");

        assert!(meta.backdrops.is_empty());
    }
}
