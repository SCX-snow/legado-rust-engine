use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::Variables;
use crate::serde_ext as de;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RssArticle {
    #[serde(default, with = "de::text")]
    pub origin: String,
    #[serde(default, with = "de::text")]
    pub sort: String,
    #[serde(default, with = "de::text")]
    pub title: String,
    #[serde(default, with = "de::text")]
    pub link: String,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub pub_date: Option<String>,
    #[serde(
        default,
        with = "de::opt_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
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
    pub image: Option<String>,
    #[serde(
        default,
        rename = "variable",
        with = "de::variables",
        skip_serializing_if = "de::is_empty"
    )]
    pub variables: HashMap<String, String>,
}

impl Variables for RssArticle {
    fn variables(&self) -> &HashMap<String, String> {
        &self.variables
    }

    fn variables_mut(&mut self) -> &mut HashMap<String, String> {
        &mut self.variables
    }
}
