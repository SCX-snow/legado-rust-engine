use legado_core::{Book, BookChapter, Error, JsRequest, JsValue, Result};

use crate::eval::{Eval, content_for, split_value, strip_reverse};
use crate::fmt::is_true;

const MAX_TOC_PAGES: usize = 50;

pub(crate) async fn analyze_chapters(
    eval: &Eval<'_>,
    book: &mut Book,
    body: &str,
) -> Result<Vec<BookChapter>> {
    let toc_rule = eval.source.rule_toc.as_ref();
    let raw_list = toc_rule
        .and_then(|rule| rule.chapter_list.as_deref())
        .unwrap_or_default();
    let (list_rule, reverse) = strip_reverse(raw_list);

    let (mut chapters, mut next_urls) = parse_page(eval, book, body, list_rule, toc_rule).await?;
    let mut seen = vec![eval.redirect_url.clone()];
    let mut pages = 1;
    while let Some(next) = next_urls.first().cloned() {
        if next.is_empty() || seen.contains(&next) || pages >= MAX_TOC_PAGES {
            break;
        }
        seen.push(next.clone());
        pages += 1;
        let response = eval
            .fetch_page(&next, &eval.redirect_url, None, None)
            .await?;
        let page_eval = eval.with_urls(&next, &response.url);
        let (page_chapters, page_next) =
            parse_page(&page_eval, book, &response.text, list_rule, toc_rule).await?;
        chapters.extend(page_chapters);
        next_urls = page_next;
    }

    let mut unique: Vec<BookChapter> = Vec::with_capacity(chapters.len());
    for chapter in chapters {
        if !unique.contains(&chapter) {
            unique.push(chapter);
        }
    }
    if reverse {
        unique.reverse();
    }
    if unique.is_empty() {
        return Err(Error::Other(format!("章节列表为空: {}", eval.redirect_url)));
    }
    for (index, chapter) in unique.iter_mut().enumerate() {
        chapter.index = index as i32;
    }
    if let Some(format_js) = toc_rule
        .and_then(|rule| rule.format_js.as_deref())
        .filter(|script| !script.trim().is_empty())
    {
        for (position, chapter) in unique.iter_mut().enumerate() {
            let quoted_title = serde_json::to_string(&chapter.title)?;
            let quoted_script = serde_json::to_string(format_js)?;
            let script = format!(
                "(function(){{var index = {};var title = {};var gInt = 0;return eval({});}})()",
                position + 1,
                quoted_title,
                quoted_script
            );
            let request = JsRequest {
                script: &script,
                source: Some(eval.source),
                base_url: &eval.base_url,
                page: None,
                key: None,
                result: None,
                src: None,
                response: None,
                book: Some(&*book),
                chapter: Some(chapter),
                variables: None,
            };
            match eval.js.eval_value(request, None).await {
                Ok(JsValue::Null) | Err(_) => {}
                Ok(value) => chapter.title = value.into_text(),
            }
        }
    }
    book.total_chapter_num = unique.len() as i32;
    book.latest_chapter_title = unique.last().map(|chapter| chapter.title.clone());
    Ok(unique)
}

async fn parse_page(
    eval: &Eval<'_>,
    book: &mut Book,
    body: &str,
    list_rule: &str,
    toc_rule: Option<&legado_core::TocRule>,
) -> Result<(Vec<BookChapter>, Vec<String>)> {
    let content = content_for(body);
    let items = if list_rule.trim().is_empty() {
        Vec::new()
    } else {
        split_value(
            eval.value(list_rule, &content, Some(&mut *book), None)
                .await?,
        )
    };

    let next_urls = eval
        .list(
            toc_rule.and_then(|rule| rule.next_toc_url.as_deref()),
            &content,
            true,
            Some(&mut *book),
            None,
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|url| url != &eval.redirect_url)
        .collect();

    let mut chapters = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(chapter) = parse_chapter(eval, book, item, toc_rule, index).await? {
            chapters.push(chapter);
        }
    }
    Ok((chapters, next_urls))
}

async fn parse_chapter(
    eval: &Eval<'_>,
    book: &mut Book,
    item: &legado_rule::Content,
    toc_rule: Option<&legado_core::TocRule>,
    index: usize,
) -> Result<Option<BookChapter>> {
    let mut chapter = BookChapter {
        book_url: book.book_url.clone(),
        base_url: eval.redirect_url.clone(),
        ..Default::default()
    };

    chapter.title = eval
        .string(
            toc_rule.and_then(|rule| rule.chapter_name.as_deref()),
            item,
            false,
            true,
            Some(&mut *book),
            Some(&mut chapter),
        )
        .await
        .unwrap_or_default();
    chapter.url = eval
        .string(
            toc_rule.and_then(|rule| rule.chapter_url.as_deref()),
            item,
            false,
            true,
            Some(&mut *book),
            Some(&mut chapter),
        )
        .await
        .unwrap_or_default();
    let tag = eval
        .string(
            toc_rule.and_then(|rule| rule.update_time.as_deref()),
            item,
            false,
            true,
            Some(&mut *book),
            Some(&mut chapter),
        )
        .await
        .unwrap_or_default();
    if !tag.trim().is_empty() {
        chapter.tag = Some(tag);
    }

    chapter.is_volume = is_true(
        &eval
            .string(
                toc_rule.and_then(|rule| rule.is_volume.as_deref()),
                item,
                false,
                true,
                Some(&mut *book),
                Some(&mut chapter),
            )
            .await
            .unwrap_or_default(),
    );
    if chapter.url.is_empty() {
        if chapter.is_volume {
            chapter.url = format!("{}{}", chapter.title, index);
        } else {
            chapter.url = eval.base_url.clone();
        }
    }
    if chapter.title.is_empty() {
        return Ok(None);
    }

    chapter.is_vip = is_true(
        &eval
            .string(
                toc_rule.and_then(|rule| rule.is_vip.as_deref()),
                item,
                false,
                true,
                Some(&mut *book),
                Some(&mut chapter),
            )
            .await
            .unwrap_or_default(),
    );
    chapter.is_pay = is_true(
        &eval
            .string(
                toc_rule.and_then(|rule| rule.is_pay.as_deref()),
                item,
                false,
                true,
                Some(&mut *book),
                Some(&mut chapter),
            )
            .await
            .unwrap_or_default(),
    );
    Ok(Some(chapter))
}
