use legado_core::{Book, BookChapter, Error, Result};
use legado_js::html_format::format_keep_img;
use legado_net::absolute_url;
use legado_rule::{Content, unescape_html};

use crate::eval::{Eval, content_for};

const MAX_CONTENT_PAGES: usize = 50;

pub(crate) async fn analyze_content(
    eval: &Eval<'_>,
    book: &mut Book,
    chapter: &mut BookChapter,
    next_chapter_url: Option<&str>,
    body: &str,
) -> Result<String> {
    let rule = eval.source.rule_content.as_ref();
    let content = content_for(body);

    if let Some(title_rule) = rule
        .and_then(|rule| rule.title.as_deref())
        .filter(|rule| !rule.trim().is_empty())
        && let Ok(title) = eval
            .string(
                Some(title_rule),
                &content,
                false,
                true,
                Some(&mut *book),
                Some(&mut *chapter),
            )
            .await
        && !title.trim().is_empty()
    {
        chapter.title = title;
    }

    let (first, next_urls) = eval_page(eval, book, chapter, &content, true).await?;
    let mut pages = vec![first];
    let mut seen = vec![eval.redirect_url.clone()];
    let next_chapter = next_chapter_url.map(|url| absolute_url(&eval.redirect_url, url));
    let mut count = 0;
    while let Some(next) = next_urls.first().cloned() {
        if next.is_empty() || seen.contains(&next) || count >= MAX_CONTENT_PAGES {
            break;
        }
        if next_chapter.as_deref() == Some(next.as_str()) {
            break;
        }
        seen.push(next.clone());
        count += 1;
        let response = eval
            .fetch_page(&next, &eval.redirect_url, None, None)
            .await?;
        let page_eval = eval.with_urls(&next, &response.url);
        let page_content = content_for(&response.text);
        let (text, _) = eval_page(&page_eval, book, chapter, &page_content, false).await?;
        pages.push(text);
    }

    let mut text = pages.join("\n");
    if let Some(replace) = rule
        .and_then(|rule| rule.replace_regex.as_deref())
        .filter(|rule| !rule.trim().is_empty())
    {
        let trimmed = text.lines().map(str::trim).collect::<Vec<_>>().join("\n");
        let replaced = eval
            .string(
                Some(replace),
                &Content::text(trimmed),
                false,
                true,
                Some(&mut *book),
                Some(&mut *chapter),
            )
            .await?;
        text = replaced
            .split('\n')
            .map(|line| format!("　　{line}"))
            .collect::<Vec<_>>()
            .join("\n");
    }

    if !chapter.is_volume && text.trim().is_empty() {
        return Err(Error::Other("内容为空".into()));
    }
    Ok(text)
}

async fn eval_page(
    eval: &Eval<'_>,
    book: &mut Book,
    chapter: &mut BookChapter,
    content: &Content,
    get_next: bool,
) -> Result<(String, Vec<String>)> {
    let rule = eval.source.rule_content.as_ref();
    let raw = eval
        .string(
            rule.and_then(|rule| rule.content.as_deref()),
            content,
            false,
            false,
            Some(&mut *book),
            Some(&mut *chapter),
        )
        .await?;
    let formatted = format_keep_img(&raw, Some(&eval.redirect_url));
    let text = if formatted.contains('&') {
        unescape_html(&formatted)
    } else {
        formatted
    };

    let next_urls = if get_next {
        eval.list(
            rule.and_then(|rule| rule.next_content_url.as_deref()),
            content,
            true,
            Some(&mut *book),
            Some(&mut *chapter),
        )
        .await?
    } else {
        Vec::new()
    };
    Ok((text, next_urls))
}
