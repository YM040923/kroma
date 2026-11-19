//! Visible-library catalogue facets, recommendations and addressable people.
use super::*;
use crate::api::{util::blocking, visibility};
use crate::model::Metadata;
use crate::model::slugify;
use std::collections::{BTreeMap, BTreeSet};

fn param<'a>(params: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    params.iter().find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.trim()).filter(|value| !value.is_empty())
}

fn localized_role(role: &str) -> String {
    role.replace("(voice)", "（配音）").replace("(Voice)", "（配音）")
}

fn parts(value: &str) -> impl Iterator<Item = &str> {
    value.split([',', '|']).map(str::trim).filter(|s| !s.is_empty())
}

fn page(mut items: Vec<Value>, params: &HashMap<String, String>) -> Value {
    let total = items.len();
    let start = param(params, "StartIndex").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    let limit = param(params, "Limit").and_then(|v| v.parse::<usize>().ok()).unwrap_or(500);
    items = items.into_iter().skip(start).take(limit).collect();
    json!({ "Items": items, "TotalRecordCount": total, "StartIndex": start })
}

/// Read and localize only granted libraries before deriving any catalogue data.
async fn visible_catalog(
    state: &SharedState, user: &User, locale: &'static str,
) -> Result<(Vec<MediaItem>, Vec<Show>), Response> {
    let user = user.clone();
    query(&state.db, move |pool| {
        let mut items = db::list_items(&pool, None)?;
        let mut shows = db::list_shows(&pool, None)?;
        visibility::keep_items(&user, &mut items);
        visibility::keep_shows(&user, &mut shows);
        db::localize::overlay_items(&pool, &mut items, locale)?;
        db::localize::overlay_shows(&pool, &mut shows, locale)?;
        Ok((items, shows))
    }).await
}

/// ParentId is never allowed to fall back to the entire catalogue.
fn scoped_catalog(
    state: &SharedState, mut items: Vec<MediaItem>, mut shows: Vec<Show>,
    params: &HashMap<String, String>,
) -> (Vec<MediaItem>, Vec<Show>) {
    let mut episode_parent = false;
    if let Some(parent) = param(params, "ParentId") {
        let defs = crate::services::settings::library_defs(&state.settings, &state.config);
        if let Some(def) = defs.iter().find(|d| d.id == parent || d.name == parent) {
            items.retain(|i| i.library == def.id || i.library == def.name);
            shows.retain(|s| s.library == def.id || s.library == def.name);
        } else {
            let (show_id, season) = match parent.rsplit_once(":s") {
                Some((id, number)) => match number.parse::<u32>() {
                    Ok(number) => (id, Some(number)),
                    Err(_) => return (Vec::new(), Vec::new()),
                },
                None => (parent, None),
            };
            if let Some(show) = shows.iter().find(|s| s.id == show_id) {
                episode_parent = true;
                // Episodes commonly carry no credits/genres; inherit the visible show.
                for item in &mut items {
                    if item.show_id.as_deref() == Some(show_id) && item.metadata.is_none() {
                        item.metadata = show.metadata.clone();
                    }
                }
                items.retain(|i| i.show_id.as_deref() == Some(show_id)
                    && season.is_none_or(|s| i.season == Some(s)));
                shows.retain(|s| s.id == show_id && season.is_none());
            } else {
                items.clear();
                shows.clear();
            }
        }
    }
    // Also inherit metadata for an explicit Episode query at library/root level.
    for item in &mut items {
        if item.metadata.is_none() {
            if let Some(show) = shows.iter().find(|s| Some(s.id.as_str()) == item.show_id.as_deref()) {
                item.metadata = show.metadata.clone();
            }
        }
    }
    if let Some(include) = param(params, "IncludeItemTypes") {
        let has = |kind: &str| parts(include).any(|s| s.eq_ignore_ascii_case(kind));
        items.retain(|i| if i.show_id.is_some() || matches!(i.kind, Kind::Episode) {
            has("Episode")
        } else {
            match i.kind { Kind::Movie => has("Movie"), Kind::Video => has("Video"), _ => false }
        });
        if !has("Series") { shows.clear(); }
    } else if !episode_parent {
        items.retain(|i| i.show_id.is_none() && matches!(i.kind, Kind::Movie | Kind::Video));
    }
    (items, shows)
}

fn person_id(name: &str, tmdb_id: Option<u64>) -> Option<String> {
    if let Some(id) = tmdb_id.filter(|id| *id > 0) {
        return Some(format!("person:{id}"));
    }
    let slug = slugify(name);
    // A numeric name must not impersonate a provider id.
    (!slug.is_empty() && slug.parse::<u64>().is_err()).then(|| format!("person:{slug}"))
}

#[derive(Clone)]
struct Credit {
    id: String,
    name: String,
    tmdb_id: Option<u64>,
    profile: Option<String>,
    kind: &'static str,
    role: Option<String>,
}

fn credits(meta: &Metadata) -> Vec<Credit> {
    let mut out = Vec::new();
    for c in &meta.cast {
        if let Some(id) = person_id(&c.name, c.tmdb_id) {
            out.push(Credit { id, name: c.name.clone(), tmdb_id: c.tmdb_id,
                profile: c.profile_url.clone(), kind: "Actor", role: c.character.clone() });
        }
    }
    for c in &meta.crew {
        if let Some(id) = person_id(&c.name, c.tmdb_id) {
            let kind = if c.job.eq_ignore_ascii_case("director") { "Director" }
                else if c.job.eq_ignore_ascii_case("writer") || c.job.eq_ignore_ascii_case("screenplay") { "Writer" }
                else { "Person" };
            out.push(Credit { id, name: c.name.clone(), tmdb_id: c.tmdb_id,
                profile: c.profile_url.clone(), kind, role: Some(c.job.clone()) });
        }
    }
    out
}

fn credit_value(c: &Credit, embedded: bool) -> Value {
    let tag = c.profile.as_deref().filter(|s| !s.is_empty()).map(|_| "1");
    json!({
        "Id": c.id, "Name": c.name, "Type": if embedded { c.kind } else { "Person" },
        "Role": c.role.as_deref().map(localized_role), "IsFolder": false, "PrimaryImageTag": tag,
        "ImageTags": if tag.is_some() { json!({"Primary": tag}) } else { json!({}) },
        "ProviderIds": c.tmdb_id.map(|id| json!({"Tmdb": id.to_string()})).unwrap_or_else(|| json!({})),
    })
}

/// Embedded credits use exactly the same identity as /Persons and /Items/person:*.
pub(super) fn people(meta: Option<&Metadata>) -> Value {
    let Some(meta) = meta else { return json!([]); };
    let addressable: HashSet<_> = unique_people(std::iter::once(meta)).into_iter().map(|c| c.id).collect();
    json!(credits(meta).iter().filter(|c| addressable.contains(&c.id))
        .map(|c| credit_value(c, true)).collect::<Vec<_>>())
}

/// AND between filter dimensions, OR within each Jellyfin list parameter.
/// Call after applying the request locale and the user's library visibility.
pub(super) fn matches_metadata(
    meta: Option<&Metadata>, year: Option<u32>, params: &HashMap<String, String>,
) -> bool {
    if let Some(years) = param(params, "Years") {
        if !parts(years).any(|s| s.parse::<u32>().ok().zip(year).is_some_and(|(a, b)| a == b)) {
            return false;
        }
    }
    if let Some(ratings) = param(params, "OfficialRatings") {
        if !meta.and_then(|m| m.certification.as_deref()).is_some_and(|rating|
            parts(ratings).any(|s| s.eq_ignore_ascii_case(rating))) { return false; }
    }
    let ids = param(params, "PersonIds");
    let name = param(params, "Person");
    if ids.is_none() && name.is_none() { return true; }
    let Some(meta) = meta else { return false; };
    let all = credits(meta);
    if let Some(ids) = ids {
        if !parts(ids).any(|id| {
            let wanted = id.strip_prefix("person:").unwrap_or(id);
            all.iter().any(|c| c.id.strip_prefix("person:") == Some(wanted))
        }) { return false; }
    }
    if let Some(name) = name {
        if !all.iter().any(|c| c.name.eq_ignore_ascii_case(name)) { return false; }
    }
    true
}

fn similarity(source: &Metadata, candidate: &Metadata) -> usize {
    let genres: HashSet<_> = source.genres.iter().map(|g| g.to_lowercase()).collect();
    let other: HashSet<_> = candidate.genres.iter().map(|g| g.to_lowercase()).collect();
    let people: HashSet<_> = credits(source).into_iter().map(|c| c.id).collect();
    let other_people: HashSet<_> = credits(candidate).into_iter().map(|c| c.id).collect();
    genres.intersection(&other).count() * 2 + people.intersection(&other_people).count() * 3
}

pub(super) async fn similar(
    State(state): State<SharedState>, headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>, Path(path): Path<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let locale = metadata_locale(&state, &headers, &params);
    let (items, shows) = visible_catalog(&state, &user, locale).await?;
    let id = path.get("id").map(String::as_str).unwrap_or_default();
    let source_item = items.iter().find(|i| i.id == id);
    let show_id = source_item.and_then(|i| i.show_id.as_deref()).unwrap_or(id);
    let source_show = shows.iter().find(|s| s.id == show_id);
    let mut ranked = Vec::new();
    if let Some(source) = source_show {
        let meta = source.metadata.as_ref().ok_or_else(visibility::out_of_scope)?;
        for show in &shows {
            if show.id == source.id || !matches_metadata(show.metadata.as_ref(), show.year, &params) { continue; }
            let score = show.metadata.as_ref().map(|m| similarity(meta, m)).unwrap_or(0);
            if score > 0 { ranked.push((score, show.id.clone(), series_dto(&state, show))); }
        }
    } else if let Some(source) = source_item.filter(|i| i.show_id.is_none() && matches!(i.kind, Kind::Movie | Kind::Video)) {
        let meta = source.metadata.as_ref().ok_or_else(visibility::out_of_scope)?;
        for item in &items {
            if item.id == source.id || item.show_id.is_some()
                || std::mem::discriminant(&item.kind) != std::mem::discriminant(&source.kind)
                || !matches_metadata(item.metadata.as_ref(), item.year, &params) { continue; }
            let score = item.metadata.as_ref().map(|m| similarity(meta, m)).unwrap_or(0);
            if score > 0 { ranked.push((score, item.id.clone(), movie_dto(&state, item))); }
        }
    } else { return Err(visibility::out_of_scope()); }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut payload = page(ranked.into_iter().map(|(_, _, dto)| dto).collect(), &params);
    if let Some(items) = payload["Items"].as_array_mut() {
        apply_user_data(&state, &user, items).await?;
    }
    Ok(Json(payload))
}

async fn facets(
    state: &SharedState, headers: &HeaderMap, params: &HashMap<String, String>, objects: bool,
) -> Result<Value, Response> {
    let (user, _) = user_of(state, headers, params).await?;
    let (items, shows) = visible_catalog(state, &user, metadata_locale(state, headers, params)).await?;
    let (items, shows) = scoped_catalog(state, items, shows, params);
    let mut genres = BTreeSet::new();
    let mut years = BTreeSet::new();
    let mut ratings = BTreeSet::new();
    for (meta, year) in items.iter().map(|i| (i.metadata.as_ref(), i.year))
        .chain(shows.iter().map(|s| (s.metadata.as_ref(), s.year))) {
        if let Some(year) = year { years.insert(year); }
        if let Some(meta) = meta {
            genres.extend(meta.genres.iter().filter(|s| !s.trim().is_empty()).cloned());
            if let Some(rating) = meta.certification.as_ref().filter(|s| !s.trim().is_empty()) {
                ratings.insert(rating.clone());
            }
        }
    }
    let genres = if objects {
        json!(genres.into_iter().map(|name| json!({"Id": name, "Name": name})).collect::<Vec<_>>())
    } else { json!(genres) };
    Ok(json!({"Genres": genres, "Years": years.into_iter().rev().collect::<Vec<_>>(),
        "OfficialRatings": ratings, "Tags": []}))
}

pub(super) async fn filters(
    State(state): State<SharedState>, headers: HeaderMap, Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    Ok(Json(facets(&state, &headers, &params, false).await?))
}

pub(super) async fn filters2(
    State(state): State<SharedState>, headers: HeaderMap, Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    Ok(Json(facets(&state, &headers, &params, true).await?))
}

pub(super) async fn genres(
    State(state): State<SharedState>, headers: HeaderMap, Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let value = facets(&state, &headers, &params, true).await?;
    Ok(Json(page(value["Genres"].as_array().cloned().unwrap_or_default(), &params)))
}

fn unique_people<'a>(metas: impl Iterator<Item = &'a Metadata>) -> Vec<Credit> {
    let mut unique: BTreeMap<String, Credit> = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for c in metas.flat_map(credits) {
        if let Some(old) = unique.get_mut(&c.id) {
            // Slug collisions must not silently alias different credited names.
            if c.tmdb_id.is_none() && c.name != old.name { ambiguous.insert(c.id.clone()); }
            if old.profile.is_none() { old.profile = c.profile; }
        } else { unique.insert(c.id.clone(), c); }
    }
    unique.into_iter().filter(|(id, _)| !ambiguous.contains(id)).map(|(_, c)| c).collect()
}

pub(super) async fn persons(
    State(state): State<SharedState>, headers: HeaderMap, Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let (items, shows) = visible_catalog(&state, &user, metadata_locale(&state, &headers, &params)).await?;
    let (items, shows) = scoped_catalog(&state, items, shows, &params);
    let mut people = unique_people(items.iter().filter_map(|i| i.metadata.as_ref())
        .chain(shows.iter().filter_map(|s| s.metadata.as_ref())));
    if let Some(search) = param(&params, "SearchTerm") {
        let search = search.to_lowercase();
        people.retain(|c| c.name.to_lowercase().contains(&search));
    }
    people.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    if param(&params, "SortOrder").is_some_and(|s| s.eq_ignore_ascii_case("Descending")) { people.reverse(); }
    Ok(Json(page(people.iter().map(|c| credit_value(c, false)).collect(), &params)))
}

/// Resolve using the native index, but choose identity/art only from visible credits.
/// Name lookups that refer to multiple provider ids deliberately return 404.
async fn visible_person(
    state: &SharedState, user: &User, id: &str, locale: &'static str,
) -> Result<Credit, Response> {
    let lookup = id.strip_prefix("person:").unwrap_or(id).trim().to_string();
    if lookup.is_empty() { return Err(visibility::out_of_scope()); }
    let wanted = lookup.clone();
    query(&state.db, move |pool| db::resolve_person(&pool, &wanted)).await?
        .ok_or_else(visibility::out_of_scope)?;
    let (items, shows) = visible_catalog(state, user, locale).await?;
    // Do not use the global resolver's chosen name or title counts: hidden
    // libraries must neither select a namesake nor supply this person's art.
    let all = unique_people(items.iter().filter(|i| i.show_id.is_none() && !matches!(i.kind, Kind::Episode))
        .filter_map(|i| i.metadata.as_ref())
        .chain(shows.iter().filter_map(|s| s.metadata.as_ref())));
    select_person(all, &lookup).ok_or_else(visibility::out_of_scope)
}

fn select_person(all: Vec<Credit>, lookup: &str) -> Option<Credit> {
    if let Ok(id) = lookup.parse::<u64>() {
        return all.into_iter().find(|c| c.tmdb_id == Some(id));
    }
    let slug = slugify(lookup);
    let mut matches: Vec<_> = all.into_iter().filter(|c|
        c.name.eq_ignore_ascii_case(lookup) || slugify(&c.name) == slug).collect();
    // A legacy slug addresses only legacy credits, never guesses a numeric identity.
    if matches.len() != 1 { return None; }
    matches.pop()
}

async fn person_data(
    state: &SharedState, user: &User, id: &str, locale: &'static str,
) -> Result<(Value, Option<String>), Response> {
    let mut credit = visible_person(state, user, id, locale).await?;
    let mut dto = credit_value(&credit, false);
    dto["ServerId"] = json!(server_id(state));
    // A name-only TMDB search cannot distinguish namesakes. Keep that credit's
    // real local information rather than attributing another person's biography.
    if let (Some(api_key), Some(tmdb_id)) = (state.config.tmdb_api_key.clone(), credit.tmdb_id) {
        let name = credit.name.clone();
        let data_dir = state.config.data_dir.clone();
        let detail = blocking(move || {
            let detail = crate::infra::metadata::person::detail(&api_key, locale, &name, Some(tmdb_id))
                .map(|mut person| {
                    if let Some(local) = person.profile_url.as_deref()
                        .and_then(|url| crate::infra::image::cache_remote(&data_dir, url)) {
                        person.profile_url = Some(local);
                    }
                    person
                });
            Ok(detail)
        }).await?;
        if let Some(detail) = detail.filter(|p| p.tmdb_id == tmdb_id) {
            dto["Name"] = json!(detail.name);
            dto["Overview"] = json!(detail.biography);
            dto["PremiereDate"] = json!(detail.birthday.as_deref().map(|d| format!("{d}T00:00:00.0000000Z")));
            dto["EndDate"] = json!(detail.deathday.as_deref().map(|d| format!("{d}T00:00:00.0000000Z")));
            dto["ProductionLocations"] = json!(detail.place_of_birth.into_iter().collect::<Vec<_>>());
            if detail.profile_url.is_some() { credit.profile = detail.profile_url; }
        }
    }
    let tag = credit.profile.as_ref().filter(|s| !s.is_empty()).map(|_| "1");
    dto["PrimaryImageTag"] = json!(tag);
    dto["ImageTags"] = if tag.is_some() { json!({"Primary": tag}) } else { json!({}) };
    Ok((dto, credit.profile))
}


/// The item-detail caller can preserve its request's explicit metadata language.
pub(super) async fn person_value_with_locale(
    state: &SharedState, user: &User, id: &str, locale: &'static str,
) -> Result<Value, Response> {
    Ok(person_data(state, user, id, locale).await?.0)
}

pub(super) async fn person_art(
    state: &SharedState, user: &User, id: &str,
) -> Result<Option<String>, Response> {
    let locale = metadata_locale(state, &HeaderMap::new(), &HashMap::new());
    let credit = visible_person(state, user, id, locale).await?;
    if let Some(art) = credit.profile.filter(|s| !s.is_empty()) { return Ok(Some(art)); }
    Ok(person_data(state, user, id, locale).await?.1)
}

pub(super) async fn person_detail(
    State(state): State<SharedState>, headers: HeaderMap, Query(params): Query<HashMap<String, String>>,
    Path(name): Path<String>,
) -> Result<Json<Value>, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let locale = metadata_locale(&state, &headers, &params);
    Ok(Json(person_value_with_locale(&state, &user, &name, locale).await?))
}

pub(super) async fn person_image(
    State(state): State<SharedState>, headers: HeaderMap, Query(params): Query<HashMap<String, String>>,
    Path((name, kind)): Path<(String, String)>,
) -> Result<Response, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    if !kind.eq_ignore_ascii_case("Primary") { return Err(visibility::out_of_scope()); }
    let locale = metadata_locale(&state, &headers, &params);
    let credit = visible_person(&state, &user, &name, locale).await?;
    let art = match credit.profile.filter(|s| !s.is_empty()) {
        Some(art) => Some(art),
        None => person_data(&state, &user, &name, locale).await?.1,
    };
    let target = art.ok_or_else(visibility::out_of_scope)?;
    redirect(&client_url(&headers, &target))
}

fn studio_names(raw: &Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for key in ["studios", "networks", "productionCompanies", "production_companies"] {
        if let Some(entries) = raw.get(key).and_then(Value::as_array) {
            for entry in entries {
                if let Some(name) = entry.as_str().or_else(|| entry.get("name").or_else(|| entry.get("Name")).and_then(Value::as_str)) {
                    if !name.trim().is_empty() { names.insert(name.trim().to_string()); }
                }
            }
        }
    }
    names
}

pub(super) async fn studios(
    State(state): State<SharedState>, headers: HeaderMap, Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, Response> {
    let (user, _) = user_of(&state, &headers, &params).await?;
    let (items, shows) = visible_catalog(&state, &user, metadata_locale(&state, &headers, &params)).await?;
    let (items, shows) = scoped_catalog(&state, items, shows, &params);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for meta in items.iter().filter_map(|i| i.metadata.as_ref())
        .chain(shows.iter().filter_map(|s| s.metadata.as_ref())) {
        if let Ok(raw) = serde_json::to_value(meta) {
            for name in studio_names(&raw) { *counts.entry(name).or_default() += 1; }
        }
    }
    let search = param(&params, "SearchTerm").unwrap_or_default().to_lowercase();
    let values = counts.into_iter().filter(|(name, _)| name.to_lowercase().contains(&search))
        .map(|(name, count)| json!({"Id": format!("studio:{name}"), "Name": name,
            "Type": "Studio", "ChildCount": count, "IsFolder": true})).collect();
    Ok(Json(page(values, &params)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(cast: Value, genres: &[&str]) -> Metadata {
        serde_json::from_value(json!({"tmdbId": 1, "tmdbUrl": "", "genres": genres,
            "cast": cast, "certification": "PG-13"})).unwrap()
    }

    #[test]
    fn same_name_provider_ids_do_not_alias() {
        let meta = metadata(json!([{"name": "John Williams", "tmdbId": 1},
            {"name": "John Williams", "tmdbId": 2}]), &[]);
        assert_eq!(people(Some(&meta))[0]["Id"], "person:1");
        assert_eq!(people(Some(&meta))[1]["Id"], "person:2");
        assert!(select_person(credits(&meta), "John Williams").is_none());
        assert_eq!(select_person(credits(&meta), "2").unwrap().tmdb_id, Some(2));
    }

    #[test]
    fn legacy_slug_matches_native_resolver_and_keeps_image_tag() {
        let meta = metadata(json!([{"name": "Timothée Chalamet", "profileUrl": "/api/images/a.jpg"}]), &[]);
        let value = people(Some(&meta));
        assert_eq!(value[0]["Id"], "person:timothee-chalamet");
        assert_eq!(value[0]["PrimaryImageTag"], "1");
        assert!(person_id("123", None).is_none());
    }

    #[test]
    fn slug_collisions_are_not_returned_as_one_person() {
        let meta = metadata(json!([{"name": "Anne-Marie"}, {"name": "Anne Marie"}]), &[]);
        assert!(unique_people(std::iter::once(&meta)).is_empty());
    }

    #[test]
    fn metadata_filters_are_conjunctive_and_reject_missing_values() {
        let meta = metadata(json!([{"name": "Actor", "tmdbId": 42}]), &[]);
        let mut params = HashMap::from([
            ("Years".into(), "2023,2024".into()), ("OfficialRatings".into(), "PG|PG-13".into()),
            ("PersonIds".into(), "person:42".into()), ("Person".into(), "actor".into()),
        ]);
        assert!(matches_metadata(Some(&meta), Some(2024), &params));
        assert!(!matches_metadata(Some(&meta), None, &params));
        assert!(!matches_metadata(None, Some(2024), &params));
        params.insert("PersonIds".into(), "person:43".into());
        assert!(!matches_metadata(Some(&meta), Some(2024), &params));
        assert!(matches_metadata(None, None, &HashMap::new()));
    }

    #[test]
    fn recommendation_score_counts_real_distinct_overlap() {
        let a = metadata(json!([{"name": "A", "tmdbId": 1}]), &["Drama"]);
        let b = metadata(json!([{"name": "Renamed", "tmdbId": 1}]), &["Drama", "Drama"]);
        let c = metadata(json!([{"name": "A", "tmdbId": 2}]), &["Comedy"]);
        assert_eq!(similarity(&a, &b), 5);
        assert_eq!(similarity(&a, &c), 0);
    }

    #[test]
    fn pagination_keeps_total_and_honors_zero_limit() {
        let params = HashMap::from([("StartIndex".into(), "1".into()), ("Limit".into(), "1".into())]);
        let value = page(vec![json!(1), json!(2), json!(3)], &params);
        assert_eq!(value["TotalRecordCount"], 3);
        assert_eq!(value["Items"], json!([2]));
        assert_eq!(page(vec![json!(1)], &HashMap::from([("Limit".into(), "0".into())]))["Items"], json!([]));
    }

    #[test]
    fn studios_use_only_supplied_metadata() {
        assert!(studio_names(&json!({})).is_empty());
        assert_eq!(studio_names(&json!({"studios": ["Example", {"name": "Example"}],
            "networks": [{"name": "Network"}]})), BTreeSet::from(["Example".into(), "Network".into()]));
    }
}
