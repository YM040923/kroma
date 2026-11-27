//! Per-module admin state: the enabled flag + config values each module carries,
//! persisted in the `moduleStates` settings blob (`{ id: { enabled, config } }`).
//! The registry of which modules exist lives in `kroma-module-kernel`.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::db::Pool;
use crate::services::settings::Settings;

fn states(settings: &Settings) -> Map<String, Value> {
    settings
        .get("moduleStates")
        .as_object()
        .cloned()
        .unwrap_or_default()
}

// One settings write-lock, so a concurrent enable + config-save cannot clobber
// each other the way a read-then-write pair would.
fn update_entry(
    settings: &Settings,
    pool: &Pool,
    id: &str,
    f: impl FnOnce(&mut Map<String, Value>),
) {
    settings.update_json(pool, "moduleStates", |current| {
        let mut all = current.as_object().cloned().unwrap_or_default();
        let mut entry = all
            .get(id)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        f(&mut entry);
        all.insert(id.to_string(), Value::Object(entry));
        Value::Object(all)
    });
}

/// True when the module was never toggled.
pub fn module_enabled(settings: &Settings, id: &str) -> bool {
    states(settings)
        .get(id)
        .and_then(|s| s.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

pub fn module_config(settings: &Settings, id: &str) -> Map<String, Value> {
    states(settings)
        .get(id)
        .and_then(|s| s.get("config"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

pub fn set_module_enabled(settings: &Settings, pool: &Pool, id: &str, enabled: bool) {
    update_entry(settings, pool, id, |entry| {
        entry.insert("enabled".into(), json!(enabled));
    });
}

/// Merges into the stored config rather than replacing it.
pub fn set_module_config(settings: &Settings, pool: &Pool, id: &str, values: Map<String, Value>) {
    update_entry(settings, pool, id, |entry| {
        let mut cfg = entry
            .get("config")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (k, v) in values {
            cfg.insert(k, v);
        }
        entry.insert("config".into(), Value::Object(cfg));
    });
}

fn selected_folder_map(settings: &Settings, id: &str, key: &str) -> BTreeMap<String, String> {
    states(settings)
        .get(id)
        .and_then(|state| state.get(key))
        .and_then(Value::as_object)
        .map(|folders| {
            folders
                .iter()
                .filter_map(|(guest, host)| {
                    host.as_str().map(|host| (guest.clone(), host.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn selected_guest_path_valid(guest: &str) -> bool {
    let Some(grant) = guest.strip_prefix("/selected/") else {
        return false;
    };
    !grant.is_empty()
        && grant
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

/// Host paths explicitly chosen for, and currently granted to, one module.
pub fn selected_folders(settings: &Settings, id: &str) -> BTreeMap<String, String> {
    selected_folder_map(settings, id, "selectedFolders")
}

/// Host paths chosen in the UI but not yet committed with the module settings.
pub fn pending_selected_folders(settings: &Settings, id: &str) -> BTreeMap<String, String> {
    selected_folder_map(settings, id, "pendingSelectedFolders")
}

pub fn add_pending_selected_folder(
    settings: &Settings,
    pool: &Pool,
    id: &str,
    guest: String,
    host: String,
) -> Result<(), String> {
    if !selected_guest_path_valid(&guest) {
        return Err("selected folder path is invalid".into());
    }
    update_entry(settings, pool, id, |entry| {
        let mut pending = entry
            .get("pendingSelectedFolders")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        pending.insert(guest, Value::String(host));
        entry.insert("pendingSelectedFolders".into(), Value::Object(pending));
    });
    Ok(())
}

/// Activate only the pending/active folders still referenced by saved module
/// mappings. A path supplied by the module is accepted only when the host issued
/// that exact guest path earlier.
pub fn commit_selected_folders(
    settings: &Settings,
    pool: &Pool,
    id: &str,
    requested: &[String],
) -> Result<bool, String> {
    let active = selected_folders(settings, id);
    let pending = pending_selected_folders(settings, id);
    let had_pending = !pending.is_empty();
    let mut available = active.clone();
    available.extend(pending);
    let mut next = BTreeMap::new();
    for guest in requested {
        if !selected_guest_path_valid(guest) {
            return Err("selected folder path is invalid".into());
        }
        let host = available
            .get(guest)
            .ok_or_else(|| "selected folder was not chosen on this computer".to_string())?;
        next.insert(guest.clone(), host.clone());
    }
    let changed = next != active || had_pending;
    update_entry(settings, pool, id, |entry| {
        let selected = next
            .into_iter()
            .map(|(guest, host)| (guest, Value::String(host)))
            .collect();
        entry.insert("selectedFolders".into(), Value::Object(selected));
        entry.insert("pendingSelectedFolders".into(), Value::Object(Map::new()));
    });
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::TempPool;

    fn store() -> (TempPool, Settings) {
        let pool = crate::db::testing::temp_pool("modstate");
        let settings = Settings::load(&pool);
        (pool, settings)
    }

    #[test]
    fn a_module_nobody_ever_toggled_is_enabled() {
        let (_pool, settings) = store();
        assert!(module_enabled(&settings, "tv.kroma.indexer"));
        assert!(module_enabled(&settings, "a.module.that.does.not.exist"));
        assert!(module_config(&settings, "tv.kroma.indexer").is_empty());
    }

    #[test]
    fn the_enabled_flag_round_trips() {
        let (pool, settings) = store();
        set_module_enabled(&settings, &pool, "tv.kroma.vpn", false);
        assert!(!module_enabled(&settings, "tv.kroma.vpn"));
        assert!(module_enabled(&settings, "tv.kroma.indexer"));

        set_module_enabled(&settings, &pool, "tv.kroma.vpn", true);
        assert!(module_enabled(&settings, "tv.kroma.vpn"));
    }

    #[test]
    fn config_merges_rather_than_replaces() {
        let (pool, settings) = store();
        set_module_config(
            &settings,
            &pool,
            "tv.kroma.indexer",
            [
                ("host".to_string(), json!("nas.local")),
                ("port".to_string(), json!(9117)),
            ]
            .into_iter()
            .collect(),
        );
        set_module_config(
            &settings,
            &pool,
            "tv.kroma.indexer",
            [("port".to_string(), json!(9200))].into_iter().collect(),
        );

        let cfg = module_config(&settings, "tv.kroma.indexer");
        assert_eq!(
            cfg.get("host"),
            Some(&json!("nas.local")),
            "an untouched key survives"
        );
        assert_eq!(
            cfg.get("port"),
            Some(&json!(9200)),
            "a named key is overwritten"
        );
    }

    #[test]
    fn enabling_and_configuring_do_not_clobber_each_other() {
        let (pool, settings) = store();
        set_module_config(
            &settings,
            &pool,
            "tv.kroma.whisper",
            [("model".to_string(), json!("small"))]
                .into_iter()
                .collect(),
        );
        set_module_enabled(&settings, &pool, "tv.kroma.whisper", false);

        assert!(!module_enabled(&settings, "tv.kroma.whisper"));
        assert_eq!(
            module_config(&settings, "tv.kroma.whisper").get("model"),
            Some(&json!("small"))
        );

        // The other order, on a second module.
        set_module_enabled(&settings, &pool, "tv.kroma.scene", false);
        set_module_config(
            &settings,
            &pool,
            "tv.kroma.scene",
            [("threshold".to_string(), json!(0.4))]
                .into_iter()
                .collect(),
        );
        assert!(!module_enabled(&settings, "tv.kroma.scene"));
        assert_eq!(
            module_config(&settings, "tv.kroma.scene").get("threshold"),
            Some(&json!(0.4))
        );
        assert!(!module_enabled(&settings, "tv.kroma.whisper"));
    }

    #[test]
    fn selected_folders_are_pending_until_the_module_settings_commit() {
        let (pool, settings) = store();
        add_pending_selected_folder(
            &settings,
            &pool,
            "tv.kroma.cloud115",
            "/selected/one".into(),
            r"D:\Media\Movies".into(),
        )
        .unwrap();

        assert!(selected_folders(&settings, "tv.kroma.cloud115").is_empty());
        assert_eq!(
            pending_selected_folders(&settings, "tv.kroma.cloud115").get("/selected/one"),
            Some(&r"D:\Media\Movies".to_string())
        );

        assert!(commit_selected_folders(
            &settings,
            &pool,
            "tv.kroma.cloud115",
            &["/selected/one".into()],
        )
        .unwrap());
        assert_eq!(
            selected_folders(&settings, "tv.kroma.cloud115").get("/selected/one"),
            Some(&r"D:\Media\Movies".to_string())
        );
        assert!(pending_selected_folders(&settings, "tv.kroma.cloud115").is_empty());
    }

    #[test]
    fn selected_folder_commit_rejects_paths_the_host_did_not_issue() {
        let (pool, settings) = store();

        assert!(commit_selected_folders(
            &settings,
            &pool,
            "tv.kroma.cloud115",
            &["/selected/not-issued".into()],
        )
        .is_err());
        assert!(selected_folders(&settings, "tv.kroma.cloud115").is_empty());
    }

    #[test]
    fn a_blob_written_by_hand_is_read_defensively() {
        let (pool, settings) = store();
        settings.update_json(&pool, "moduleStates", |_| json!("not an object"));
        assert!(module_enabled(&settings, "tv.kroma.indexer"));
        assert!(module_config(&settings, "tv.kroma.indexer").is_empty());

        settings.update_json(
            &pool,
            "moduleStates",
            |_| json!({ "tv.kroma.indexer": { "enabled": "yes", "config": [1, 2] } }),
        );
        assert!(module_enabled(&settings, "tv.kroma.indexer"));
        assert!(module_config(&settings, "tv.kroma.indexer").is_empty());

        set_module_enabled(&settings, &pool, "tv.kroma.indexer", false);
        assert!(!module_enabled(&settings, "tv.kroma.indexer"));
    }

    #[test]
    fn state_survives_a_reload_from_the_database() {
        let (pool, settings) = store();
        set_module_enabled(&settings, &pool, "tv.kroma.vector", false);
        set_module_config(
            &settings,
            &pool,
            "tv.kroma.vector",
            [("dim".to_string(), json!(512))].into_iter().collect(),
        );

        let reloaded = Settings::load(&pool);
        assert!(!module_enabled(&reloaded, "tv.kroma.vector"));
        assert_eq!(
            module_config(&reloaded, "tv.kroma.vector").get("dim"),
            Some(&json!(512))
        );
    }
}
