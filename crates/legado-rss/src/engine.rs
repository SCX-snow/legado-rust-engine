use std::sync::Arc;

use legado_book::{PageEval, PageFetcher, content_for, fetch_page, split_value, strip_reverse};
use legado_core::{BookChapter, JsEvaluator, Result, RssArticle, RssSource, Source};
use legado_net::absolute_url;
use legado_rule::{Content, RuleEngine};

use crate::parser::parse_default;

pub struct RssEngine {
    rules: Arc<RuleEngine>,
    js: Arc<dyn JsEvaluator>,
    fetcher: Arc<dyn PageFetcher>,
}

impl RssEngine {
    pub fn new(
        rules: Arc<RuleEngine>,
        js: Arc<dyn JsEvaluator>,
        fetcher: Arc<dyn PageFetcher>,
    ) -> Self {
        Self { rules, js, fetcher }
    }

    pub async fn articles(
        &self,
        source: &RssSource,
        sort_name: &str,
        sort_url: &str,
        page: i32,
    ) -> Result<(Vec<RssArticle>, Option<String>)> {
        let response = fetch_page(
            &*self.js,
            source,
            &*self.fetcher,
            sort_url,
            source.key(),
            None,
            Some(page),
        )
        .await?;
        let Some(rule_articles) = source
            .rule_articles
            .as_deref()
            .filter(|rule| !rule.trim().is_empty())
        else {
            let articles = parse_default(sort_name, &response.text, source.key());
            return Ok((articles, None));
        };
        let eval = PageEval::new(
            &self.rules,
            &*self.js,
            source,
            &*self.fetcher,
            sort_url,
            sort_url,
            Some(page),
            None,
        );
        analyze_articles(&eval, source, sort_name, rule_articles, &response.text).await
    }

    pub async fn content(
        &self,
        source: &RssSource,
        article: &RssArticle,
        rule_content: &str,
    ) -> Result<String> {
        let response = fetch_page(
            &*self.js,
            source,
            &*self.fetcher,
            &article.link,
            &article.origin,
            None,
            None,
        )
        .await?;
        let base = absolute_url(&article.origin, &article.link);
        let eval = PageEval::new(
            &self.rules,
            &*self.js,
            source,
            &*self.fetcher,
            base.clone(),
            base,
            None,
            None,
        );
        eval.string(
            Some(rule_content),
            &content_for(&response.text),
            false,
            true,
            None,
            Some(&mut BookChapter {
                variables: article.variables.clone(),
                ..Default::default()
            }),
        )
        .await
    }
}

async fn analyze_articles(
    eval: &PageEval<'_, RssSource>,
    source: &RssSource,
    sort_name: &str,
    rule_articles: &str,
    body: &str,
) -> Result<(Vec<RssArticle>, Option<String>)> {
    let content = content_for(body);
    let (rule_text, reverse) = strip_reverse(rule_articles);
    let items = if rule_text.trim().is_empty() {
        Vec::new()
    } else {
        split_value(eval.value(rule_text, &content, None, None).await?)
    };

    let next_url = match source
        .rule_next_page
        .as_deref()
        .filter(|rule| !rule.trim().is_empty())
    {
        Some(rule) if rule.eq_ignore_ascii_case("page") => Some(eval.base_url.clone()),
        Some(rule) => {
            let raw = eval
                .string(Some(rule), &content, false, true, None, None)
                .await?;
            let raw = raw.trim();
            if raw.is_empty() {
                None
            } else {
                Some(absolute_url(&eval.base_url, raw))
            }
        }
        None => None,
    };

    let mut articles = Vec::new();
    for item in &items {
        if let Some(article) = parse_item(eval, source, item, sort_name).await? {
            articles.push(article);
        }
    }
    if reverse {
        articles.reverse();
    }
    Ok((articles, next_url))
}

async fn parse_item(
    eval: &PageEval<'_, RssSource>,
    source: &RssSource,
    item: &Content,
    sort_name: &str,
) -> Result<Option<RssArticle>> {
    let mut article = RssArticle {
        origin: source.key().to_string(),
        sort: sort_name.to_string(),
        ..Default::default()
    };
    let mut variables = BookChapter {
        variables: source.variables.clone(),
        ..Default::default()
    };
    article.title = eval
        .string(
            source.rule_title.as_deref(),
            item,
            false,
            true,
            None,
            Some(&mut variables),
        )
        .await
        .unwrap_or_default();
    if article.title.trim().is_empty() {
        return Ok(None);
    }
    article.pub_date = non_empty(
        eval.string(
            source.rule_pub_date.as_deref(),
            item,
            false,
            true,
            None,
            Some(&mut variables),
        )
        .await
        .unwrap_or_default(),
    );
    let has_description = source
        .rule_description
        .as_deref()
        .is_some_and(|rule| !rule.trim().is_empty());
    if has_description {
        article.description = non_empty(
            eval.string(
                source.rule_description.as_deref(),
                item,
                false,
                true,
                None,
                Some(&mut variables),
            )
            .await
            .unwrap_or_default(),
        );
    }
    article.image = non_empty(
        eval.string(
            source.rule_image.as_deref(),
            item,
            true,
            true,
            None,
            Some(&mut variables),
        )
        .await
        .unwrap_or_default(),
    );
    let link = eval
        .string(
            source.rule_link.as_deref(),
            item,
            false,
            true,
            None,
            Some(&mut variables),
        )
        .await
        .unwrap_or_default();
    article.link = absolute_url(source.key(), link.trim());
    article.variables = variables.variables;
    Ok(Some(article))
}

fn non_empty(text: String) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}
