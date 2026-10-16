use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::model::rule::{BookInfoRule, ContentRule, ExploreRule, ReviewRule, SearchRule, TocRule};
use crate::serde_ext as de;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBase {
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub js_lib: Option<String>,
    #[serde(
        default,
        with = "de::opt_boolean",
        skip_serializing_if = "Option::is_none"
    )]
    pub enabled_cookie_jar: Option<bool>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub concurrent_rate: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub header: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub login_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub login_ui: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub login_check_js: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub cover_decode_js: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub variable_comment: Option<String>,
}

static SOURCE_VARIABLES: OnceLock<Mutex<HashMap<String, HashMap<String, String>>>> =
    OnceLock::new();

fn source_store() -> &'static Mutex<HashMap<String, HashMap<String, String>>> {
    SOURCE_VARIABLES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn source_variables(key: &str) -> HashMap<String, String> {
    source_store()
        .lock()
        .map(|all| all.get(key).cloned().unwrap_or_default())
        .unwrap_or_default()
}

pub fn put_source_variable(key: &str, name: &str, value: &str) {
    if let Ok(mut all) = source_store().lock() {
        all.entry(key.to_string())
            .or_default()
            .insert(name.to_string(), value.to_string());
    }
}

pub trait Source: Send + Sync + std::fmt::Debug {
    fn common(&self) -> &SourceBase;
    fn key(&self) -> &str;
    fn tag(&self) -> &str;

    fn js_lib(&self) -> Option<&str> {
        self.common().js_lib.as_deref()
    }

    fn header(&self) -> Option<&str> {
        self.common().header.as_deref()
    }

    fn login_url(&self) -> Option<&str> {
        self.common().login_url.as_deref()
    }

    fn login_check_js(&self) -> Option<&str> {
        self.common().login_check_js.as_deref()
    }

    fn cover_decode_js(&self) -> Option<&str> {
        self.common().cover_decode_js.as_deref()
    }

    fn concurrent_rate(&self) -> Option<&str> {
        self.common().concurrent_rate.as_deref()
    }

    fn cookie_jar(&self) -> bool {
        self.common().enabled_cookie_jar.unwrap_or(false)
    }

    fn to_json(&self) -> serde_json::Value;

    fn get(&self, key: &str) -> Option<String> {
        source_variables(self.key()).get(key).cloned()
    }

    fn put(&self, key: &str, value: &str) {
        put_source_variable(self.key(), key, value);
    }

    fn to_owned_source(&self) -> Box<dyn Source>;
}

fn default_respond_time() -> i64 {
    180_000
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookSource {
    #[serde(default, with = "de::text")]
    pub book_source_url: String,
    #[serde(default, with = "de::text")]
    pub book_source_name: String,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub book_source_group: Option<String>,
    #[serde(default, with = "de::i32")]
    pub book_source_type: i32,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub book_url_pattern: Option<String>,
    #[serde(default, with = "de::i32")]
    pub custom_order: i32,
    #[serde(default = "de::default_true", with = "de::boolean")]
    pub enabled: bool,
    #[serde(default = "de::default_true", with = "de::boolean")]
    pub enabled_explore: bool,
    #[serde(flatten, default)]
    pub base: SourceBase,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub book_source_comment: Option<String>,
    #[serde(default, with = "de::i64")]
    pub last_update_time: i64,
    #[serde(default = "default_respond_time", with = "de::i64")]
    pub respond_time: i64,
    #[serde(default, with = "de::i32")]
    pub weight: i32,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub explore_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub explore_screen: Option<String>,
    #[serde(default, with = "de::rule", skip_serializing_if = "Option::is_none")]
    pub rule_explore: Option<ExploreRule>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub search_url: Option<String>,
    #[serde(default, with = "de::rule", skip_serializing_if = "Option::is_none")]
    pub rule_search: Option<SearchRule>,
    #[serde(default, with = "de::rule", skip_serializing_if = "Option::is_none")]
    pub rule_book_info: Option<BookInfoRule>,
    #[serde(default, with = "de::rule", skip_serializing_if = "Option::is_none")]
    pub rule_toc: Option<TocRule>,
    #[serde(default, with = "de::rule", skip_serializing_if = "Option::is_none")]
    pub rule_content: Option<ContentRule>,
    #[serde(default, with = "de::rule", skip_serializing_if = "Option::is_none")]
    pub rule_review: Option<ReviewRule>,
}

impl Source for BookSource {
    fn common(&self) -> &SourceBase {
        &self.base
    }

    fn key(&self) -> &str {
        &self.book_source_url
    }

    fn tag(&self) -> &str {
        &self.book_source_name
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    fn to_owned_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RssSource {
    #[serde(default, with = "de::text")]
    pub source_url: String,
    #[serde(default, with = "de::text")]
    pub source_name: String,
    #[serde(default, with = "de::text")]
    pub source_icon: String,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_group: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_comment: Option<String>,
    #[serde(default = "de::default_true", with = "de::boolean")]
    pub enabled: bool,
    #[serde(flatten, default)]
    pub base: SourceBase,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub sort_url: Option<String>,
    #[serde(default, with = "de::boolean")]
    pub single_url: bool,
    #[serde(default, with = "de::i32")]
    pub article_style: i32,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub rule_articles: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub rule_next_page: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub rule_title: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub rule_pub_date: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub rule_description: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub rule_image: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub rule_link: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub rule_content: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub content_whitelist: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub content_blacklist: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub should_override_url_loading: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub style: Option<String>,
    #[serde(default = "de::default_true", with = "de::boolean")]
    pub enable_js: bool,
    #[serde(default = "de::default_true", with = "de::boolean")]
    pub load_with_base_url: bool,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub inject_js: Option<String>,
    #[serde(default, with = "de::i64")]
    pub last_update_time: i64,
    #[serde(default, with = "de::i32")]
    pub custom_order: i32,
    #[serde(
        default,
        rename = "variable",
        with = "de::variables",
        skip_serializing_if = "de::is_empty"
    )]
    pub variables: HashMap<String, String>,
}

impl Source for RssSource {
    fn common(&self) -> &SourceBase {
        &self.base
    }

    fn key(&self) -> &str {
        &self.source_url
    }

    fn tag(&self) -> &str {
        &self.source_name
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    fn to_owned_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }
}
