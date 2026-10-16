use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use legado_rust_engine::book::{BookEngine, PageFetcher, search_book_to_book};
use legado_rust_engine::core::{BookSource, Error, Source};
use legado_rust_engine::js::JsEngine;
use legado_rust_engine::net::HttpClient;
use legado_rust_engine::rule::RuleEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Search,
    Detail,
    Toc,
    Content,
}

struct Failure {
    stage: Stage,
    error: Error,
}

#[derive(Default)]
struct Stats {
    attempted: usize,
    search_ok: usize,
    detail_ok: usize,
    toc_ok: usize,
    content_ok: usize,
    full: usize,
    skipped_webview: usize,
    skipped_no_search: usize,
    chain_seconds: f64,
}

impl Stats {
    fn record(&mut self, outcome: &Result<(), Failure>) {
        match outcome {
            Ok(()) => {
                self.search_ok += 1;
                self.detail_ok += 1;
                self.toc_ok += 1;
                self.content_ok += 1;
                self.full += 1;
            }
            Err(failure) => match failure.stage {
                Stage::Detail => self.search_ok += 1,
                Stage::Toc => {
                    self.search_ok += 1;
                    self.detail_ok += 1;
                }
                Stage::Content => {
                    self.search_ok += 1;
                    self.detail_ok += 1;
                    self.toc_ok += 1;
                }
                Stage::Search => {}
            },
        }
    }

    fn percent(&self, value: usize) -> f64 {
        if self.attempted == 0 {
            0.0
        } else {
            value as f64 * 100.0 / self.attempted as f64
        }
    }

    fn report(&self) {
        println!("\n== 成功率分布（尝试 {} 个源） ==", self.attempted);
        for (name, value) in [
            ("搜索有结果", self.search_ok),
            ("详情", self.detail_ok),
            ("目录", self.toc_ok),
            ("正文", self.content_ok),
            ("完整链路", self.full),
        ] {
            println!("  {name:<10} {value:>3} ({:>5.1}%)", self.percent(value));
        }
        println!(
            "  跳过：WebView {} 个、没有 searchUrl {} 个",
            self.skipped_webview, self.skipped_no_search
        );
        let average = if self.full == 0 {
            0.0
        } else {
            self.chain_seconds / self.full as f64
        };
        println!(
            "统计: 尝试 {} / 搜索 {} / 详情 {} / 目录 {} / 正文 {} / 完整 {} ({:.1}%) / 平均整链 {:.1}s",
            self.attempted,
            self.search_ok,
            self.detail_ok,
            self.toc_ok,
            self.content_ok,
            self.full,
            self.percent(self.full),
            average
        );
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut limit = 60usize;
    let mut keyword = "斗破苍穹".to_string();
    let mut until_first = false;
    let mut sources_path = "fixtures/book_sources.json".to_string();
    let mut emit_cases: Option<String> = None;
    let mut positional = 0;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--first" => until_first = true,
            "--sources" => sources_path = args.next().unwrap_or(sources_path),
            "--emit-cases" => emit_cases = args.next(),
            other => {
                if positional == 0 {
                    limit = other.parse().unwrap_or(limit);
                } else if positional == 1 {
                    keyword = other.to_string();
                }
                positional += 1;
            }
        }
    }
    match run(
        limit,
        &keyword,
        until_first,
        &sources_path,
        emit_cases.as_deref(),
    )
    .await
    {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(
    limit: usize,
    keyword: &str,
    until_first: bool,
    sources_path: &str,
    emit_cases: Option<&str>,
) -> legado_rust_engine::core::Result<bool> {
    let text = std::fs::read_to_string(sources_path)?;
    let sources: Vec<BookSource> = serde_json::from_str(&text)?;
    let mut case_writer = match emit_cases {
        Some(path) => {
            if let Some(parent) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            Some(std::io::BufWriter::new(std::fs::File::create(path)?))
        }
        None => None,
    };
    let mut case_count = 0usize;

    let client = Arc::new(
        HttpClient::builder()
            .total_timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(10))
            .build()?,
    );
    let rules = Arc::new(RuleEngine::new());
    let js = Arc::new(
        JsEngine::builder()
            .http(client.clone())
            .rules(rules.clone())
            .build()?,
    );
    rules.set_js(js.clone());
    let fetcher: Arc<dyn PageFetcher> = client.clone();
    let engine = BookEngine::new(rules, js, fetcher);

    let mut stats = Stats::default();
    for (index, source) in sources.iter().enumerate() {
        if !source.enabled {
            continue;
        }
        if stats.attempted >= limit {
            break;
        }
        if source
            .search_url
            .as_deref()
            .is_none_or(|rule| rule.trim().is_empty())
        {
            stats.skipped_no_search += 1;
            continue;
        }
        let raw = serde_json::to_string(source).unwrap_or_default();
        if ["startBrowserAwait", "getVerificationCode", "webView"]
            .iter()
            .any(|api| raw.contains(api))
        {
            stats.skipped_webview += 1;
            continue;
        }
        stats.attempted += 1;
        println!(
            "\n[{}/{}] {} <{}>",
            stats.attempted,
            limit,
            source.book_source_name,
            source.key()
        );
        let started = Instant::now();
        let outcome = walk(
            &engine,
            source,
            index,
            keyword,
            started,
            &mut case_writer,
            &mut case_count,
        )
        .await;
        if outcome.is_ok() {
            stats.chain_seconds += started.elapsed().as_secs_f64();
        }
        stats.record(&outcome);
        if let Err(failure) = &outcome {
            println!("   ✗ {:?}: {}", failure.stage, failure.error);
        }
        if until_first && stats.full > 0 {
            break;
        }
    }

    stats.report();
    if let Some(path) = emit_cases {
        println!("\n对拍请求行: {case_count} 条 → {path}");
        return Ok(case_count > 0);
    }
    if stats.full > 0 {
        println!(
            "\n✅ 完整链路成功: {} 个书源（尝试 {} 个）",
            stats.full, stats.attempted
        );
        Ok(true)
    } else {
        eprintln!("\n❌ {} 个书源都没走完整条链路", stats.attempted);
        Ok(false)
    }
}

fn info_field<'a>(
    source: &'a BookSource,
    pick: impl Fn(&'a legado_rust_engine::core::BookInfoRule) -> &'a Option<String>,
) -> Option<&'a String> {
    source
        .rule_book_info
        .as_ref()
        .and_then(|rule| pick(rule).as_ref())
}

fn toc_field<'a>(
    source: &'a BookSource,
    pick: impl Fn(&'a legado_rust_engine::core::TocRule) -> &'a Option<String>,
) -> Option<&'a String> {
    source
        .rule_toc
        .as_ref()
        .and_then(|rule| pick(rule).as_ref())
}

fn content_field<'a>(
    source: &'a BookSource,
    pick: impl Fn(&'a legado_rust_engine::core::ContentRule) -> &'a Option<String>,
) -> Option<&'a String> {
    source
        .rule_content
        .as_ref()
        .and_then(|rule| pick(rule).as_ref())
}

#[allow(clippy::too_many_arguments)]
async fn emit_stage(
    engine: &BookEngine,
    source: &BookSource,
    index: usize,
    stage: &str,
    url: &str,
    fields: &[(&str, Option<&String>)],
    case_writer: &mut Option<std::io::BufWriter<std::fs::File>>,
    case_count: &mut usize,
) {
    use std::io::Write as _;
    let Some(writer) = case_writer.as_mut() else {
        return;
    };
    if url.trim().is_empty() {
        return;
    }
    let response = match engine
        .fetch_page(source, url, source.key(), None, None)
        .await
    {
        Ok(response) => response,
        Err(error) => {
            println!("   · {stage} 页取不到（{url}）：{error}");
            return;
        }
    };
    let content = response.text;
    if content.is_empty() {
        return;
    }
    let source_json = serde_json::to_value(source).unwrap_or_default();
    let mut emitted = 0;
    for (field, rule) in fields {
        let Some(rule) = rule.map(String::as_str) else {
            continue;
        };
        if rule.trim().is_empty() {
            continue;
        }
        let modes: &[&str] = if matches!(*field, "chapterList" | "content") {
            &["string", "list"]
        } else {
            &["string"]
        };
        for mode in modes {
            let case = serde_json::json!({
                "id": format!("{index}:{stage}:{field}:{mode}"),
                "mode": mode,
                "rule": rule,
                "content": content,
                "baseUrl": url,
                "source": source_json,
            });
            if writeln!(writer, "{case}").is_err() {
                return;
            }
            emitted += 1;
        }
    }
    *case_count += emitted;
}

#[allow(clippy::too_many_arguments)]
async fn walk(
    engine: &BookEngine,
    source: &BookSource,
    index: usize,
    keyword: &str,
    started: Instant,
    case_writer: &mut Option<std::io::BufWriter<std::fs::File>>,
    case_count: &mut usize,
) -> Result<(), Failure> {
    let results = engine
        .search(source, keyword, 1)
        .await
        .map_err(|error| Failure {
            stage: Stage::Search,
            error,
        })?;
    if results.is_empty() {
        return Err(Failure {
            stage: Stage::Search,
            error: Error::Other("搜索无结果".into()),
        });
    }
    println!(
        "   ✓ 搜索 {} 条（{:.1}s）",
        results.len(),
        started.elapsed().as_secs_f32()
    );
    let search_book = results.first().expect("刚刚判过非空");
    println!(
        "     首条: 《{}》 {} {}",
        search_book.name, search_book.author, search_book.book_url
    );

    let mut book = search_book_to_book(search_book);
    engine
        .book_info(source, &mut book)
        .await
        .map_err(|error| Failure {
            stage: Stage::Detail,
            error,
        })?;
    println!("   ✓ 详情: 《{}》 目录 {}", book.name, book.toc_url);
    if case_writer.is_some() {
        emit_stage(
            engine,
            source,
            index,
            "detail",
            &book.book_url,
            &[
                ("name", info_field(source, |rule| &rule.name)),
                ("author", info_field(source, |rule| &rule.author)),
                ("intro", info_field(source, |rule| &rule.intro)),
                ("kind", info_field(source, |rule| &rule.kind)),
                ("lastChapter", info_field(source, |rule| &rule.last_chapter)),
                ("updateTime", info_field(source, |rule| &rule.update_time)),
                ("coverUrl", info_field(source, |rule| &rule.cover_url)),
                ("tocUrl", info_field(source, |rule| &rule.toc_url)),
                ("wordCount", info_field(source, |rule| &rule.word_count)),
            ],
            case_writer,
            case_count,
        )
        .await;
    }

    let mut chapters = engine
        .chapters(source, &mut book)
        .await
        .map_err(|error| Failure {
            stage: Stage::Toc,
            error,
        })?;
    if chapters.is_empty() {
        return Err(Failure {
            stage: Stage::Toc,
            error: Error::Other("目录为空".into()),
        });
    }
    println!(
        "   ✓ 目录 {} 章，首章《{}》",
        chapters.len(),
        chapters[0].title
    );

    if case_writer.is_some() {
        emit_stage(
            engine,
            source,
            index,
            "toc",
            &book.toc_url,
            &[
                ("chapterList", toc_field(source, |rule| &rule.chapter_list)),
                ("chapterName", toc_field(source, |rule| &rule.chapter_name)),
                ("chapterUrl", toc_field(source, |rule| &rule.chapter_url)),
                ("isVip", toc_field(source, |rule| &rule.is_vip)),
                ("isVolume", toc_field(source, |rule| &rule.is_volume)),
                ("updateTime", toc_field(source, |rule| &rule.update_time)),
                ("nextTocUrl", toc_field(source, |rule| &rule.next_toc_url)),
            ],
            case_writer,
            case_count,
        )
        .await;
    }

    let next = chapters.get(1).map(|chapter| chapter.url.clone());
    let mut chapter = chapters.remove(0);
    let content = engine
        .content(source, &mut book, &mut chapter, next.as_deref())
        .await
        .map_err(|error| Failure {
            stage: Stage::Content,
            error,
        })?;
    if content.trim().is_empty() {
        return Err(Failure {
            stage: Stage::Content,
            error: Error::Other("正文为空".into()),
        });
    }
    let preview: String = content.chars().take(60).collect();
    println!(
        "   ✓ 正文 《{}》 {} 字: {}…",
        chapter.title,
        content.chars().count(),
        preview.replace('\n', " ")
    );
    if case_writer.is_some() {
        emit_stage(
            engine,
            source,
            index,
            "content",
            &chapter.url,
            &[
                ("content", content_field(source, |rule| &rule.content)),
                ("title", content_field(source, |rule| &rule.title)),
                (
                    "nextContentUrl",
                    content_field(source, |rule| &rule.next_content_url),
                ),
                (
                    "replaceRegex",
                    content_field(source, |rule| &rule.replace_regex),
                ),
            ],
            case_writer,
            case_count,
        )
        .await;
    }
    println!("   ≡ 整链耗时 {:.1}s", started.elapsed().as_secs_f32());
    Ok(())
}
