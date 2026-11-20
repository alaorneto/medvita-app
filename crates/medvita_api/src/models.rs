//! Shapes of the core endpoints the shell needs. Module endpoints add their own models.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `GET /api/v1/me/` (`skeleton/api/views.py`, `me_data`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    pub sysid: Uuid,
    pub email: String,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub last_name: String,
    pub revision: u64,
    pub profile: Profile,
}

impl Me {
    /// The name to greet the person with: the profile display name, the full name or the e-mail.
    pub fn display_name(&self) -> String {
        if !self.profile.display_name.trim().is_empty() {
            return self.profile.display_name.trim().to_owned();
        }
        let full = format!("{} {}", self.first_name.trim(), self.last_name.trim());
        if !full.trim().is_empty() {
            return full.trim().to_owned();
        }
        self.email.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub sysid: Option<Uuid>,
    pub revision: u64,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub timezone: String,
    #[serde(default)]
    pub language: String,
}

/// One membership of `GET /api/v1/organizations/` (`organization_data`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Organization {
    /// Membership id.
    pub sysid: Uuid,
    pub organization_id: Uuid,
    pub is_active: bool,
    pub organization_name: String,
    pub organization_slug: String,
}

/// `GET /api/v1/server/` (proposed in alaorneto/skeleton#605; absent on older servers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub product: String,
    pub core_version: String,
    #[serde(default)]
    pub api_versions: Vec<String>,
    pub oidc_issuer: Option<String>,
}

/// One item of `GET /api/v1/o/<org>/apps/active/` (proposed in alaorneto/skeleton#605).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveApp {
    pub app_id: String,
    pub label: String,
    pub version: String,
    #[serde(default)]
    pub title: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me(display: &str, first: &str, last: &str) -> Me {
        Me {
            sysid: Uuid::nil(),
            email: "ana@example.com".into(),
            first_name: first.into(),
            last_name: last.into(),
            revision: 1,
            profile: Profile {
                sysid: None,
                revision: 1,
                display_name: display.into(),
                timezone: "America/Sao_Paulo".into(),
                language: "pt-br".into(),
            },
        }
    }

    #[test]
    fn display_name_prefers_profile_then_full_name_then_email() {
        assert_eq!(me("Dra. Ana", "Ana", "Silva").display_name(), "Dra. Ana");
        assert_eq!(me("", "Ana", "Silva").display_name(), "Ana Silva");
        assert_eq!(me(" ", "", "").display_name(), "ana@example.com");
    }
}
