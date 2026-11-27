//! Account types: users, capability permissions, the public profile-picker
//! shape and registration invites.
//!
//! The JSON shape here is a public contract web/TV clients depend on it, so
//! field names and casing must not drift.

use std::collections::HashSet;

use serde::{de::Error, Deserialize, Deserializer, Serialize};

pub use kroma_module_wire::{LibraryScope, Permission, User};

/// The account's home component overrides. Two empty lists keep the original
/// home; hiding a component need not remove its place in the order.
#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HomeLayout {
    #[serde(deserialize_with = "component_ids")]
    order: Vec<String>,
    #[serde(deserialize_with = "component_ids")]
    hidden: Vec<String>,
}

/// Sidebar visibility is a display preference, not a library access policy.
#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SidebarLayout {
    #[serde(deserialize_with = "component_ids")]
    hidden: Vec<String>,
}

const MAX_COMPONENTS: usize = 1024;
const MAX_COMPONENT_ID_BYTES: usize = 64;

fn valid_component_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    id.len() <= MAX_COMPONENT_ID_BYTES
        && bytes.next().is_some_and(|b| b.is_ascii_alphanumeric())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

fn component_ids<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<String>, D::Error> {
    let ids = Vec::<String>::deserialize(de)?;
    if ids.len() > MAX_COMPONENTS {
        return Err(D::Error::custom("too many layout components"));
    }
    let mut seen = HashSet::new();
    for id in &ids {
        if !valid_component_id(id) {
            return Err(D::Error::custom("invalid layout component id"));
        }
        if !seen.insert(id) {
            return Err(D::Error::custom("duplicate layout component id"));
        }
    }
    Ok(ids)
}

/// The message key naming the role a capability set adds up to, for the admin
/// UI's role badge. A key rather than words, so the client translates it and
/// can match on it.
pub fn role_label(perms: &[Permission]) -> &'static str {
    if perms.contains(&Permission::UsersManage) && perms.contains(&Permission::SettingsManage) {
        "admin.roleOwner"
    } else if perms.contains(&Permission::Playback) {
        "admin.roleMember"
    } else {
        "admin.roleRestricted"
    }
}

/// The publicly-listable subset of a user, surfaced by `GET /api/users` to
/// populate the "Qui regarde ?" profile picker (no email).
#[derive(Debug, Clone, Serialize)]
pub struct PublicUser {
    pub id: String,
    pub username: String,
    #[serde(rename = "avatarUrl", skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(rename = "hasPin")]
    pub has_pin: bool,
}

/// A registration invitation created by a user with `users.manage`. After the
/// bootstrap owner, an invite is the only way to create an account.
#[derive(Debug, Clone, Serialize)]
pub struct Invite {
    pub token: String,
    pub permissions: Vec<Permission>,
    #[serde(rename = "createdBy", skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: i64,
    pub used: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_layout_bounds_apply_to_each_list_without_requiring_a_component_catalog() {
        use serde_json::json;

        let ids: Vec<_> = (0..1024).map(|n| format!("row:{n}")).collect();
        let longest = "a".repeat(64);
        let valid = json!({ "order": ids, "hidden": ["row:0", longest] });
        let layout: HomeLayout = serde_json::from_value(valid.clone()).unwrap();
        assert_eq!(serde_json::to_value(layout).unwrap(), valid);

        let too_many: Vec<_> = (0..1025).map(|n| format!("row:{n}")).collect();
        for list in [
            json!(["hero", "hero"]),
            json!([""]),
            json!(["-hero"]),
            json!(["genre/28"]),
            json!(["hero\n"]),
            json!(["a".repeat(65)]),
            json!(too_many),
        ] {
            for field in ["order", "hidden"] {
                let mut body = json!({ "order": [], "hidden": [] });
                body[field] = list.clone();
                assert!(serde_json::from_value::<HomeLayout>(body).is_err());
            }
        }
    }

    #[test]
    fn every_permission_parses_back_from_its_stored_key() {
        for perm in Permission::all() {
            let stored = serde_json::to_string(&perm).unwrap();
            let key = stored.trim_matches('"');
            assert_eq!(Permission::parse(key), Some(perm), "{key}");
        }
    }

    #[test]
    fn an_unknown_permission_key_is_ignored_rather_than_failing_the_account() {
        assert_eq!(Permission::parse("modules.manage"), None);
        assert_eq!(Permission::parse(""), None);
    }

    #[test]
    fn the_role_badge_follows_the_capability_set() {
        assert_eq!(role_label(&Permission::all()), "admin.roleOwner");
        assert_eq!(
            role_label(&[Permission::UsersManage, Permission::SettingsManage]),
            "admin.roleOwner"
        );
        assert_eq!(
            role_label(&[Permission::Playback, Permission::RequestsCreate]),
            "admin.roleMember"
        );
        assert_eq!(
            role_label(&[Permission::RequestsCreate]),
            "admin.roleRestricted"
        );
        assert_eq!(role_label(&[]), "admin.roleRestricted");
        assert_eq!(
            role_label(&[Permission::UsersManage]),
            "admin.roleRestricted"
        );
    }
}
