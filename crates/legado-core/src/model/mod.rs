pub mod book;
pub mod chapter;
pub mod replace;
pub mod rss;
pub mod rule;
pub mod source;

pub use book::{Book, SearchBook};
pub use chapter::BookChapter;
pub use replace::ReplaceRule;
pub use rss::RssArticle;
pub use rule::{
    BookInfoRule, BookListRule, ContentRule, ExploreRule, ReviewRule, SearchRule, TocRule,
};
pub use source::{BookSource, RssSource, Source, SourceBase};

use std::collections::HashMap;

pub trait Variables {
    fn variables(&self) -> &HashMap<String, String>;
    fn variables_mut(&mut self) -> &mut HashMap<String, String>;

    fn put_variable(&mut self, key: &str, value: Option<&str>) {
        match value {
            Some(value) => {
                self.variables_mut()
                    .insert(key.to_string(), value.to_string());
            }
            None => {
                self.variables_mut().remove(key);
            }
        }
    }

    fn get_variable(&self, key: &str) -> Option<&str> {
        self.variables().get(key).map(String::as_str)
    }
}
