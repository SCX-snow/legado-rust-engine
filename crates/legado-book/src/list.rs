use legado_core::{Book, BookListRule, BookSource, Result, SearchBook, Source};
use legado_rule::Content;

use crate::eval::{Eval, content_for, split_value, strip_reverse};
use crate::fmt::{format_book_author, format_book_name, regex_full_match, word_count_format};
use legado_net::absolute_url;

pub(crate) async fn analyze_book_list(
    eval: &Eval<'_>,
    is_search: bool,
    body: &str,
) -> Result<Vec<SearchBook>> {
    if is_search
        && let Some(pattern) = eval
            .source
            .book_url_pattern
            .as_deref()
            .filter(|p| !p.is_empty())
        && regex_full_match(pattern, &eval.redirect_url)
    {
        return Ok(parse_info_item(eval, body, &eval.redirect_url)
            .await?
            .into_iter()
            .collect());
    }

    let list_rule = select_list_rule(eval.source, is_search);
    let raw_rule = list_rule
        .and_then(|rule| rule.book_list.as_deref())
        .unwrap_or_default();
    let (rule_text, reverse) = strip_reverse(raw_rule);

    let content = content_for(body);
    let items = if rule_text.trim().is_empty() {
        Vec::new()
    } else {
        split_value(eval.value(rule_text, &content, None, None).await?)
    };

    if items.is_empty()
        && eval
            .source
            .book_url_pattern
            .as_deref()
            .is_none_or(str::is_empty)
    {
        return Ok(parse_info_item(eval, body, &eval.redirect_url)
            .await?
            .into_iter()
            .collect());
    }

    let mut books = Vec::new();
    for item in items {
        if let Some(book) = parse_search_item(eval, &item, list_rule).await? {
            books.push(book);
        }
    }
    let mut unique: Vec<SearchBook> = Vec::with_capacity(books.len());
    for book in books {
        if !unique.contains(&book) {
            unique.push(book);
        }
    }
    if reverse {
        unique.reverse();
    }
    Ok(unique)
}

fn select_list_rule(source: &BookSource, is_search: bool) -> Option<&BookListRule> {
    if is_search {
        return source.rule_search.as_ref().map(|rule| &rule.list);
    }
    match source.rule_explore.as_ref() {
        Some(rule)
            if rule
                .book_list
                .as_deref()
                .is_some_and(|text| !text.is_empty()) =>
        {
            Some(rule)
        }
        _ => source.rule_search.as_ref().map(|rule| &rule.list),
    }
}

async fn parse_search_item(
    eval: &Eval<'_>,
    item: &Content,
    rule: Option<&BookListRule>,
) -> Result<Option<SearchBook>> {
    let mut book = SearchBook {
        origin: eval.source.key().to_string(),
        origin_name: eval.source.tag().to_string(),
        book_type: eval.source.book_source_type,
        ..Default::default()
    };

    let name = eval
        .string(
            rule.and_then(|r| r.name.as_deref()),
            item,
            false,
            true,
            None,
            None,
        )
        .await
        .unwrap_or_default();
    let name = format_book_name(&name);
    if name.is_empty() {
        return Ok(None);
    }
    book.name = name;

    book.author = format_book_author(
        &eval
            .string(
                rule.and_then(|r| r.author.as_deref()),
                item,
                false,
                true,
                None,
                None,
            )
            .await
            .unwrap_or_default(),
    );

    if let Ok(kinds) = eval
        .list(
            rule.and_then(|r| r.kind.as_deref()),
            item,
            false,
            None,
            None,
        )
        .await
        && !kinds.is_empty()
    {
        book.kind = Some(kinds.join(","));
    }
    if let Ok(word_count) = eval
        .string(
            rule.and_then(|r| r.word_count.as_deref()),
            item,
            false,
            true,
            None,
            None,
        )
        .await
    {
        let word_count = word_count_format(&word_count);
        if !word_count.is_empty() {
            book.word_count = Some(word_count);
        }
    }
    if let Ok(last_chapter) = eval
        .string(
            rule.and_then(|r| r.last_chapter.as_deref()),
            item,
            false,
            true,
            None,
            None,
        )
        .await
        && !last_chapter.trim().is_empty()
    {
        book.latest_chapter_title = Some(last_chapter);
    }
    if let Ok(intro) = eval
        .string(
            rule.and_then(|r| r.intro.as_deref()),
            item,
            false,
            true,
            None,
            None,
        )
        .await
        && !intro.trim().is_empty()
    {
        book.intro = Some(legado_js::html_format::format_text(&intro));
    }
    if let Ok(cover) = eval
        .string(
            rule.and_then(|r| r.cover_url.as_deref()),
            item,
            false,
            true,
            None,
            None,
        )
        .await
        && !cover.trim().is_empty()
    {
        book.cover_url = Some(absolute_url(&eval.redirect_url, cover.trim()));
    }

    book.book_url = eval
        .string(
            rule.and_then(|r| r.book_url.as_deref()),
            item,
            true,
            true,
            None,
            None,
        )
        .await?;
    Ok(Some(book))
}

async fn parse_info_item(eval: &Eval<'_>, body: &str, url: &str) -> Result<Option<SearchBook>> {
    let mut book = Book {
        book_url: url.to_string(),
        origin: eval.source.key().to_string(),
        origin_name: eval.source.tag().to_string(),
        book_type: eval.source.book_source_type,
        ..Default::default()
    };
    let info_eval = eval.with_urls(url, url);
    crate::info::analyze_book_info(&info_eval, &mut book, false, body).await?;
    if book.name.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(book_to_search(book)))
}

pub(crate) fn book_to_search(book: Book) -> SearchBook {
    SearchBook {
        book_url: book.book_url,
        origin: book.origin,
        origin_name: book.origin_name,
        name: book.name,
        author: book.author,
        kind: book.kind,
        cover_url: book.cover_url,
        intro: book.intro,
        book_type: book.book_type,
        latest_chapter_title: book.latest_chapter_title,
        toc_url: book.toc_url,
        word_count: book.word_count,
        variables: book.variables,
    }
}

pub fn search_book_to_book(book: &SearchBook) -> Book {
    Book {
        book_url: book.book_url.clone(),
        toc_url: book.toc_url.clone(),
        origin: book.origin.clone(),
        origin_name: book.origin_name.clone(),
        name: book.name.clone(),
        author: book.author.clone(),
        kind: book.kind.clone(),
        cover_url: book.cover_url.clone(),
        intro: book.intro.clone(),
        book_type: book.book_type,
        latest_chapter_title: book.latest_chapter_title.clone(),
        word_count: book.word_count.clone(),
        variables: book.variables.clone(),
        ..Default::default()
    }
}
