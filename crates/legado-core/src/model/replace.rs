use serde::{Deserialize, Serialize};

use crate::serde_ext as de;

fn default_timeout_millisecond() -> i64 {
    3000
}

fn default_order() -> i32 {
    i32::MIN
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceRule {
    #[serde(default, with = "de::i64")]
    pub id: i64,
    #[serde(default, with = "de::text")]
    pub name: String,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub group: Option<String>,
    #[serde(default, with = "de::text")]
    pub pattern: String,
    #[serde(default, with = "de::text")]
    pub replacement: String,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub scope: Option<String>,
    #[serde(default, with = "de::boolean")]
    pub scope_title: bool,
    #[serde(default = "de::default_true", with = "de::boolean")]
    pub scope_content: bool,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub exclude_scope: Option<String>,
    #[serde(default = "de::default_true", with = "de::boolean")]
    pub is_enabled: bool,
    #[serde(default = "de::default_true", with = "de::boolean")]
    pub is_regex: bool,
    #[serde(default = "default_timeout_millisecond", with = "de::i64")]
    pub timeout_millisecond: i64,
    #[serde(default = "default_order", with = "de::i32")]
    pub order: i32,
}
