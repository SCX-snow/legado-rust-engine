use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::Variables;
use crate::serde_ext as de;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    #[serde(default, with = "de::text")]
    pub book_url: String,
    #[serde(default, with = "de::text")]
    pub toc_url: String,
    #[serde(default, with = "de::text")]
    pub origin: String,
    #[serde(default, with = "de::text")]
    pub origin_name: String,
    #[serde(default, with = "de::text")]
    pub name: String,
    #[serde(default, with = "de::text")]
    pub author: String,
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
    pub cover_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub intro: Option<String>,
    #[serde(default, rename = "type", with = "de::i32")]
    pub book_type: i32,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub latest_chapter_title: Option<String>,
    #[serde(default, with = "de::i32")]
    pub total_chapter_num: i32,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub word_count: Option<String>,
    #[serde(
        default,
        rename = "variable",
        with = "de::variables",
        skip_serializing_if = "de::is_empty"
    )]
    pub variables: HashMap<String, String>,
}

impl Variables for Book {
    fn variables(&self) -> &HashMap<String, String> {
        &self.variables
    }

    fn variables_mut(&mut self) -> &mut HashMap<String, String> {
        &mut self.variables
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBook {
    #[serde(default, with = "de::text")]
    pub book_url: String,
    #[serde(default, with = "de::text")]
    pub origin: String,
    #[serde(default, with = "de::text")]
    pub origin_name: String,
    #[serde(default, with = "de::text")]
    pub name: String,
    #[serde(default, with = "de::text")]
    pub author: String,
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
    pub cover_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub intro: Option<String>,
    #[serde(default, rename = "type", with = "de::i32")]
    pub book_type: i32,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub latest_chapter_title: Option<String>,
    #[serde(default, with = "de::text")]
    pub toc_url: String,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub word_count: Option<String>,
    #[serde(
        default,
        rename = "variable",
        with = "de::variables",
        skip_serializing_if = "de::is_empty"
    )]
    pub variables: HashMap<String, String>,
}

impl Variables for SearchBook {
    fn variables(&self) -> &HashMap<String, String> {
        &self.variables
    }

    fn variables_mut(&mut self) -> &mut HashMap<String, String> {
        &mut self.variables
    }
}
