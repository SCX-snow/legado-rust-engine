# legado-rust-engine

A Rust implementation of the core engine of [legado](https://github.com/gedoor/legado).

legado is an Android reader whose "book sources" use rules to describe how to search for,
parse, and fetch web content. This project reimplements legado's core components (rule parsing
and evaluation, the request layer, the JavaScript host, and the book-source workflow) as a pure
Rust library. It does not depend on the JVM and can be embedded in your own crawler, aggregation
service, or reader.

Rule semantics, the JavaScript host, network behavior, replacement rules, and RSS are regression-
tested against output from the real legado engine. The comparison corpus is stored in `fixtures/`.

The current version is **0.1.0**, and the API may still change.

## Features

- **Rule engine**: Supports legado book-source rule syntax, including CSS / XPath / JsonPath / regular expressions, `{{}}` templates and variables, `@get` / `@put`, `##` replacements, and inline `@js:` scripts
- **Request layer**: URL rules and options; HTTP timeouts, retries, proxies, gzip, and br; cookies; encoding detection; and rate limiting
- **JavaScript runtime**: Rhino-style `java.*` and `source` / `book` / `chapter` / `cookie` / `cache` hosts; reusable `jsLib` scopes; `Packages` / `importClass` / `with()`; and a Jsoup shim
- **Book-source workflow**: Search, discovery, book details, catalogs, and content (pagination, `formatKeepImg`, and `replaceRegex`), plus exact-match search
- **Supporting features**: Replacement and sanitization, RSS parsing, TTF font anti-scraping, and symmetric and asymmetric encryption
- **legado compatibility**: Outputs are regression-tested against the real engine, and behavioral drift is caught by the comparison corpus

## Installation

Requirements:

- Rust 1.85 or later (edition 2024)
- The system libraries `libxml2` (XPath) and `libclang` (generating FFI bindings)

```bash
# Debian / Ubuntu
sudo apt install libxml2-dev libclang-dev

# Set this manually if libclang cannot be found
export LIBCLANG_PATH=/usr/lib/llvm/22/lib64
```

Add the dependency to `Cargo.toml` using Git or a local path:

```toml
[dependencies]
legado-rust-engine = { git = "https://gitea.slcomplex.party/SunLiCheng/legado-rust-engine.git" }
# Or use a local path
# legado-rust-engine = { path = "../legado-rust-engine" }

tokio = { version = "1", features = ["macros", "rt"] }
```

Enable the `redis` feature to use Redis-backed caching:

```toml
legado-rust-engine = { git = "https://gitea.slcomplex.party/SunLiCheng/legado-rust-engine.git", features = ["redis"] }
```

## Quick Start

```rust
use legado_rust_engine::rule::{Content, EvalContext, RuleEngine};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let html = r#"<div class="book"><h3 class="title">斗破苍穹</h3><a class="url" href="/book/1.html">详情</a></div>"#;

    let engine = RuleEngine::new();
    let mut ctx = EvalContext::new(Content::html(html));

    let title = engine.eval_string("class.book@h3.title@text", &mut ctx).await?;
    let url = engine.eval_string("class.book@a.url@href", &mut ctx).await?;

    println!("{title} -> {url}"); // 斗破苍穹 -> /book/1.html
    Ok(())
}
```

Rule strings use the same syntax as legado book sources. The complete fetching workflow is
orchestrated by `book::BookEngine`, assembled from `Arc<RuleEngine>`, `Arc<dyn JsEvaluator>`,
and `Arc<dyn PageFetcher>`. You can replace the network layer, JavaScript runtime, and cache with
your own implementations.

Common modules: `core` (data models and traits), `rule` (rule engine), `net` (request layer),
`js` (JavaScript runtime), `book` (book-source workflow), `replace` (replacement and
sanitization), and `rss` (RSS).

## Examples

The repository includes runnable examples:

```bash
cargo run --example rule_eval          # Rule engine: CSS / XPath / JsonPath / regex / templates / variables
cargo run --example js_eval            # JavaScript runtime: java.* hosts, crypto signatures, rule-engine callbacks
cargo run --example book_offline       # Complete book-source workflow (offline stubs; no network)
cargo run --example roundtrip_sources  # Convert between book-source JSON and legado export formats
cargo run --example replace_eval       # Replacement and sanitization
cargo run --example rss_offline        # RSS parsing
cargo run --example query_ttf          # TTF font anti-scraping
```

Examples that require network access or external services:

```bash
cargo run --example net_fetch          # Live site: parse URL rules and fetch content
cargo run --example book_fetch -- 60   # Live book source: search -> details -> catalog -> content
cargo run --example rss_fetch          # Public RSS feed
```

For a more complete offline check, including formatting and Clippy gates, run
`scripts/run-tests.sh`.

## Relationship to legado

This project is a rewrite of the core engine of [legado](https://github.com/gedoor/legado)
(AGPL-3.0), not a port of the Android app. The UI, database, and reader are not included. The goal
is to produce the same results as legado from the same book-source rules on the Rust side, so rule
semantics, the JavaScript host, and network behavior are regression-tested against the real legado
engine. A small number of known differences (such as some XPath tree expressions, valueless
attributes, and Rhino object stringification) are documented individually in the corpus and
release notes.

## License

**AGPL-3.0**, see [LICENSE](LICENSE). This project rewrites legado's core engine and uses the same
license as the original project. The `zhconv` dependency is licensed under GPL-2.0-or-later.
