use std::sync::Arc;

use legado_core::{
    Book, BookChapter, BookSource, Error, JsEvaluator, JsRequest, ReplaceRule, Result, SearchBook,
    Source,
};
use legado_rule::RuleEngine;

use crate::eval::{Eval, fetch_page};
use crate::fetch::PageFetcher;
use crate::{content, info, list, toc};

pub struct BookEngine {
    rules: Arc<RuleEngine>,
    js: Arc<dyn JsEvaluator>,
    fetcher: Arc<dyn PageFetcher>,
}

impl BookEngine {
    pub fn new(
        rules: Arc<RuleEngine>,
        js: Arc<dyn JsEvaluator>,
        fetcher: Arc<dyn PageFetcher>,
    ) -> Self {
        Self { rules, js, fetcher }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval<'a>(
        &'a self,
        source: &'a BookSource,
        base_url: impl Into<String>,
        redirect_url: impl Into<String>,
        page: Option<i32>,
        key: Option<String>,
    ) -> Eval<'a> {
        Eval::new(
            &self.rules,
            &*self.js,
            source,
            &*self.fetcher,
            base_url,
            redirect_url,
            page,
            key,
        )
    }

    pub async fn fetch_page(
        &self,
        source: &BookSource,
        rule: &str,
        base_url: &str,
        key: Option<&str>,
        page: Option<i32>,
    ) -> Result<legado_core::HttpResponse> {
        fetch_page(&*self.js, source, &*self.fetcher, rule, base_url, key, page).await
    }

    pub async fn search(
        &self,
        source: &BookSource,
        key: &str,
        page: i32,
    ) -> Result<Vec<SearchBook>> {
        let rule = source
            .search_url
            .as_deref()
            .filter(|rule| !rule.trim().is_empty())
            .ok_or_else(|| Error::Other(format!("搜索 url 为空: {}", source.tag())))?;
        let response = self
            .fetch_page(source, rule, source.key(), Some(key), Some(page))
            .await?;
        let eval = self.eval(
            source,
            response.url.clone(),
            response.url.clone(),
            Some(page),
            Some(key.to_string()),
        );
        list::analyze_book_list(&eval, true, &response.text).await
    }

    pub async fn explore(
        &self,
        source: &BookSource,
        url: &str,
        page: i32,
    ) -> Result<Vec<SearchBook>> {
        let response = self
            .fetch_page(source, url, source.key(), None, Some(page))
            .await?;
        let eval = self.eval(
            source,
            response.url.clone(),
            response.url.clone(),
            Some(page),
            None,
        );
        list::analyze_book_list(&eval, false, &response.text).await
    }

    pub async fn precise_search(
        &self,
        sources: &[BookSource],
        name: &str,
        author: &str,
    ) -> Result<(Book, usize)> {
        for (index, source) in sources.iter().enumerate() {
            let matched = match self.search(source, name, 1).await {
                Ok(results) => results
                    .into_iter()
                    .find(|book| book.name == name && book.author == author),
                Err(_) => None,
            };
            let Some(matched) = matched else {
                continue;
            };
            let mut book = crate::search_book_to_book(&matched);
            if book.toc_url.trim().is_empty() && self.book_info(source, &mut book).await.is_err() {
                continue;
            }
            return Ok((book, index));
        }
        Err(Error::Other(format!("未搜索到<{name}>{author}")))
    }

    pub async fn book_info(&self, source: &BookSource, book: &mut Book) -> Result<()> {
        let response = self
            .fetch_page(source, &book.book_url, source.key(), None, None)
            .await?;
        let eval = self.eval(
            source,
            book.book_url.clone(),
            response.url.clone(),
            None,
            None,
        );
        info::analyze_book_info(&eval, book, true, &response.text).await
    }

    pub async fn chapters(&self, source: &BookSource, book: &mut Book) -> Result<Vec<BookChapter>> {
        if let Some(script) = source
            .rule_toc
            .as_ref()
            .and_then(|rule| rule.pre_update_js.as_deref())
            .filter(|script| !script.trim().is_empty())
        {
            self.js
                .eval(JsRequest {
                    script,
                    source: Some(source),
                    base_url: &book.book_url,
                    page: None,
                    key: None,
                    result: None,
                    src: None,
                    response: None,
                    book: Some(book),
                    chapter: None,
                    variables: None,
                })
                .await?;
        }
        let response = self
            .fetch_page(source, &book.toc_url, &book.book_url, None, None)
            .await?;
        let eval = self.eval(
            source,
            book.toc_url.clone(),
            response.url.clone(),
            None,
            None,
        );
        toc::analyze_chapters(&eval, book, &response.text).await
    }

    pub async fn content(
        &self,
        source: &BookSource,
        book: &mut Book,
        chapter: &mut BookChapter,
        next_chapter_url: Option<&str>,
    ) -> Result<String> {
        let has_content = source
            .rule_content
            .as_ref()
            .and_then(|rule| rule.content.as_deref())
            .is_some_and(|rule| !rule.trim().is_empty());
        if !has_content {
            return Ok(chapter.url.clone());
        }
        if chapter.is_volume && chapter.url.starts_with(&chapter.title) {
            return Ok(chapter.tag.clone().unwrap_or_default());
        }
        let response = self
            .fetch_page(source, &chapter.url, &chapter.base_url, None, None)
            .await?;
        let eval = self.eval(
            source,
            response.url.clone(),
            response.url.clone(),
            None,
            None,
        );
        content::analyze_content(&eval, book, chapter, next_chapter_url, &response.text).await
    }

    pub async fn content_with_replace(
        &self,
        source: &BookSource,
        book: &mut Book,
        chapter: &mut BookChapter,
        next_chapter_url: Option<&str>,
        rules: &[ReplaceRule],
    ) -> Result<String> {
        let text = self
            .content(source, book, chapter, next_chapter_url)
            .await?;
        if rules.is_empty() {
            return Ok(text);
        }
        let outcome = legado_replace::apply_to_content(
            rules,
            &book.name,
            &book.origin,
            &text,
            Some(&*self.js),
        )
        .await;
        Ok(outcome.text)
    }

    pub async fn apply_title_replace(
        &self,
        book: &Book,
        chapter: &mut BookChapter,
        rules: &[ReplaceRule],
    ) -> legado_replace::ReplaceOutcome {
        let outcome = legado_replace::apply_to_title(
            rules,
            &book.name,
            &book.origin,
            &chapter.title,
            Some(&*self.js),
        )
        .await;
        chapter.title = outcome.text.clone();
        outcome
    }
}
