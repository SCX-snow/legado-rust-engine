# legado-rust-engine

用 Rust 实现的 [legado](https://github.com/gedoor/legado) 核心引擎。

legado 是 Android 上的阅读器，它的「书源」用一套规则描述如何搜索、解析与抓取网页内容。
本项目把 legado 的核心部分（规则解析与求值、请求层、JS 宿主、书源链路）重写为纯 Rust 库，
不依赖 JVM，可以嵌入自己的爬虫、聚合服务或阅读器。

规则语义、JS 宿主、网络行为、替换与 RSS 都以真实 legado 引擎的输出为回归基准，
对拍语料保存在 `fixtures/` 下。

当前版本 **0.1.0**，API 仍可能调整。

## 特性

- **规则引擎** —— 兼容 legado 书源规则语法：CSS / XPath / JsonPath / 正则、`{{}}` 模板与变量、`@get` / `@put`、`##` 替换、`@js:` 内联脚本
- **请求层** —— URL 规则与选项、HTTP（超时 / 重试 / 代理 / gzip / br）、Cookie、编码检测、限速
- **JS 运行时** —— Rhino 风格的 `java.*` 与 `source` / `book` / `chapter` / `cookie` / `cache` 宿主，支持 `jsLib` 作用域复用、`Packages` / `importClass` / `with()`、Jsoup 垫片
- **书源链路** —— 搜索、发现、详情、目录、正文（分页、`formatKeepImg`、`replaceRegex`）、精确搜索
- **配套能力** —— 替换净化、RSS 解析、TTF 字体反爬、对称与非对称加密
- **对齐 legado** —— 与真实引擎的输出逐条回归，行为漂移会被对拍语料捕获

## 安装

环境要求：

- Rust 1.85 或更高（edition 2024）
- 系统库 `libxml2`（XPath）与 `libclang`（生成 FFI 绑定）

```bash
# Debian / Ubuntu
sudo apt install libxml2-dev libclang-dev

# 找不到 libclang 时手动指定
export LIBCLANG_PATH=/usr/lib/llvm/22/lib64
```

在 `Cargo.toml` 中以 git 依赖或本地路径引入：

```toml
[dependencies]
legado-rust-engine = { git = "https://gitea.slcomplex.party/SunLiCheng/legado-rust-engine.git" }
# 或使用本地路径
# legado-rust-engine = { path = "../legado-rust-engine" }

tokio = { version = "1", features = ["macros", "rt"] }
```

需要 Redis 版缓存时打开 `redis` feature：

```toml
legado-rust-engine = { git = "https://gitea.slcomplex.party/SunLiCheng/legado-rust-engine.git", features = ["redis"] }
```

## 快速开始

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

规则字符串与 legado 书源里的写法一致。完整的抓取链路由 `book::BookEngine` 组织，
它由 `Arc<RuleEngine>`、`Arc<dyn JsEvaluator>` 与 `Arc<dyn PageFetcher>` 组装，
网络层、JS 运行时与缓存都可以替换成自己的实现。

常用模块：`core`（数据模型与 traits）、`rule`（规则引擎）、`net`（请求层）、
`js`（JS 运行时）、`book`（书源链路）、`replace`（替换净化）、`rss`（RSS）。

## 示例

仓库自带一组可运行的示例：

```bash
cargo run --example rule_eval          # 规则引擎：CSS / XPath / JsonPath / 正则 / 模板 / 变量
cargo run --example js_eval            # JS 运行时：java.* 宿主、加密签名、回调规则引擎
cargo run --example book_offline       # 书源链路全流程（离线桩，不联网）
cargo run --example roundtrip_sources  # 书源 JSON 与 legado 导出格式互转
cargo run --example replace_eval       # 替换净化
cargo run --example rss_offline        # RSS 解析
cargo run --example query_ttf          # TTF 字体反爬
```

需要联网或外部服务的示例：

```bash
cargo run --example net_fetch          # 真实站点：URL 规则解析与抓取
cargo run --example book_fetch -- 60   # 真实书源：搜索 → 详情 → 目录 → 正文
cargo run --example rss_fetch          # 真实公开 feed
```

更完整的离线自检（含格式与 clippy 门禁）可以用 `scripts/run-tests.sh` 一次跑完。

## 与 legado 的关系

本项目是 [legado](https://github.com/gedoor/legado)（AGPL-3.0）核心引擎的重写，不是 Android
应用的移植：界面、数据库与阅读器不在范围内。目标是让同一份书源规则在 Rust 侧得到与 legado
一致的结果，因此规则语义、JS 宿主与网络行为都按真实 legado 的输出做回归。少量已知差异
（如部分 XPath 树形写法、无值属性、Rhino 对象字符串化等）在语料与发布说明中逐条记录。

## 许可

**AGPL-3.0**，见 [LICENSE](LICENSE)。本项目是 legado 核心引擎的重写，沿用与原项目相同的许可。
依赖中的 `zhconv` 为 GPL-2.0-or-later。