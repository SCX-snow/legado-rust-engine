use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::Variables;
use crate::serde_ext as de;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookChapter {
    #[serde(default, with = "de::text")]
    pub url: String,
    #[serde(default, with = "de::text")]
    pub title: String,
    #[serde(default, with = "de::boolean")]
    pub is_volume: bool,
    #[serde(default, with = "de::text")]
    pub base_url: String,
    #[serde(default, with = "de::text")]
    pub book_url: String,
    #[serde(default, with = "de::i32")]
    pub index: i32,
    #[serde(default, with = "de::boolean")]
    pub is_vip: bool,
    #[serde(default, with = "de::boolean")]
    pub is_pay: bool,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub resource_url: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub tag: Option<String>,
    #[serde(
        default,
        rename = "variable",
        with = "de::variables",
        skip_serializing_if = "de::is_empty"
    )]
    pub variables: HashMap<String, String>,
}

impl Variables for BookChapter {
    fn variables(&self) -> &HashMap<String, String> {
        &self.variables
    }

    fn variables_mut(&mut self) -> &mut HashMap<String, String> {
        &mut self.variables
    }
}
