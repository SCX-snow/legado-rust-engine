use serde::{Deserialize, Serialize};

use crate::serde_ext as de;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookListRule {
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub book_list: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub author: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub intro: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub kind: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_chapter: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub update_time: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub book_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub cover_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub word_count: Option<String>,
}

pub type ExploreRule = BookListRule;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRule {
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub check_key_word: Option<String>,
    #[serde(flatten, default)]
    pub list: BookListRule,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookInfoRule {
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub init: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub author: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub intro: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub kind: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_chapter: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub update_time: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub cover_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub toc_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub word_count: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub can_re_name: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub download_urls: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TocRule {
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub pre_update_js: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub chapter_list: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub chapter_name: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub chapter_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub format_js: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub is_volume: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub is_vip: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub is_pay: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub update_time: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_toc_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentRule {
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub content: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub title: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_content_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub web_js: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_regex: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub replace_regex: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub image_style: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub image_decode: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub pay_action: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRule {
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub review_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub avatar_rule: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub content_rule: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub post_time_rule: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub review_quote_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub vote_up_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub vote_down_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub post_review_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub post_quote_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub delete_url: Option<String>,
}
