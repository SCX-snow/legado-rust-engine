use std::future::Future;
use std::process::ExitCode;
use std::task::{Context, Poll, Waker};

use legado_rust_engine::core::{
    Book, BookChapter, BoxFuture, JsEvaluator, JsRequest, JsValue, Result, Variables,
};
use legado_rust_engine::rule::{Content, EvalContext, RuleEngine, TraceBuffer};

const HTML: &str = r#"<html><body>
  <script>var ad = 1;</script>
  <div id="wrap">
    <ul class="books">
      <li class="book"><a href="/b/1"><h3 class="title">斗破苍穹</h3></a><span class="author">天蚕土豆</span><p class="intro">这里是简介<em>强调</em></p></li>
      <li class="book"><a href="/b/2"><h3 class="title">武动乾坤</h3></a><span class="author">天蚕土豆</span><p class="intro">第二本</p></li>
      <li class="book vip"><a href="/b/3"><h3 class="title">大主宰</h3></a><span class="author">天蚕土豆</span><p class="intro">第三本</p></li>
    </ul>
    <div class="pages"><a href="/p/1">1</a><a href="/p/2">2</a></div>
  </div>
</body></html>"#;

const JSON: &str = r#"{"code":0,"data":{"book-draw":"测试","name":"斗破苍穹","author":"天蚕土豆","books":[{"title":"斗破苍穹","price":10},{"title":"武动乾坤","price":20}]}}"#;

const WS_HTML: &str = r#"<html><body><p class="ws">　全角　</p></body></html>"#;

const BRACKET_HTML: &str = r#"<html><body><p class="b">a[b[c</p></body></html>"#;

const JSOUP_HTML: &str = r#"<html><body>
  <div id="w" class="wrap">
    <p class="8col">数字类名</p>
    <span class="-2">负号类名</span>
    <a href="/x/1">一</a>
  </div>
  <div class="outer"><span class="jYOxx">子元素命中</span></div>
  <div class="outer2"><i><span class="jYOxx">孙元素不算</span></i></div>
  <div class="row"><a href="/y/1">一</a></div>
</body></html>"#;

struct StubJs;

impl JsEvaluator for StubJs {
    fn eval<'a>(&'a self, request: JsRequest<'a>) -> BoxFuture<'a, Result<String>> {
        let script = request.script.to_string();
        Box::pin(async move { Ok(format!("[js:{script}]")) })
    }
}

struct ResultEchoJs;

impl JsEvaluator for ResultEchoJs {
    fn eval<'a>(&'a self, request: JsRequest<'a>) -> BoxFuture<'a, Result<String>> {
        let text = request.result.unwrap_or_default().to_string();
        Box::pin(async move { Ok(text) })
    }

    fn eval_value<'a>(
        &'a self,
        request: JsRequest<'a>,
        result_items: Option<&'a [String]>,
    ) -> BoxFuture<'a, Result<JsValue>> {
        let items = result_items.map(<[String]>::to_vec);
        let text = request.result.unwrap_or_default().to_string();
        Box::pin(async move {
            Ok(match items {
                Some(items) => JsValue::List(items),
                None => JsValue::Text(text),
            })
        })
    }
}

#[derive(Default)]
struct Checker {
    total: usize,
    failures: usize,
}

impl Checker {
    fn check(&mut self, name: &str, expected: &str, got: &str) {
        self.total += 1;
        if expected == got {
            println!("  ok   {name}");
        } else {
            self.failures += 1;
            println!("  FAIL {name}\n       期望 {expected:?}\n       实际 {got:?}");
        }
    }

    fn guard(&mut self, expected: usize) {
        if self.total == expected {
            println!("  ok   断言执行数 {expected}");
        } else {
            self.failures += 1;
            println!(
                "  FAIL 断言执行数 {}，预期 {expected}（有分支被跳过？）",
                self.total
            );
        }
    }
}

fn main() -> ExitCode {
    let engine = RuleEngine::new();
    let js = StubJs;
    let mut check = Checker::default();

    println!("== CSS：后缀、索引、伪类、组合符");
    for (rule, expected) in [
        ("class.book@tag.h3@text", "斗破苍穹\n武动乾坤\n大主宰"),
        ("class.book.0@tag.h3@text", "斗破苍穹"),
        ("class.book.-1@tag.h3@text", "大主宰"),
        ("class.book.0@li.book.0@tag.h3@text", "斗破苍穹"),
        ("class.book!1@tag.h3@text", "斗破苍穹\n大主宰"),
        ("class.book[1]@tag.h3@text", "武动乾坤"),
        ("class.book[-1]@tag.h3@text", "大主宰"),
        ("class.book[0:1]@tag.h3@text", "斗破苍穹\n武动乾坤"),
        ("class.book[-1:0]@tag.h3@text", "大主宰\n武动乾坤\n斗破苍穹"),
        ("class.book[!1]@tag.h3@text", "斗破苍穹\n大主宰"),
        ("class.book:eq(1)@tag.h3@text", "武动乾坤"),
        ("class.book:lt(2)@tag.h3@text", "斗破苍穹\n武动乾坤"),
        ("class.book:gt(0)@tag.h3@text", "武动乾坤\n大主宰"),
        ("class.book:contains(第二本)@tag.h3@text", "武动乾坤"),
        ("class.book:has(em)@tag.h3@text", "斗破苍穹"),
        ("class.book:matches(大主宰)@tag.h3@text", "大主宰"),
        ("@@class.book@tag.a.0@href", "/b/1\n/b/2\n/b/3"),
        (
            "@css:ul.books > li.book > a > h3@text",
            "斗破苍穹\n武动乾坤\n大主宰",
        ),
        ("@css:a[href^=/b/]@href", "/b/1\n/b/2\n/b/3"),
        ("class.intro@ownText", "这里是简介\n第二本\n第三本"),
        ("class.intro@text", "这里是简介强调\n第二本\n第三本"),
        ("class.author@text", "天蚕土豆\n天蚕土豆\n天蚕土豆"),
        ("children@tag.h3@text", "斗破苍穹\n武动乾坤\n大主宰"),
        (
            "class.book@tag.a.0@href||class.pages@tag.a@href",
            "/b/1\n/b/2\n/b/3",
        ),
        (
            "class.book@tag.h3@text&&class.pages@tag.a@text",
            "斗破苍穹\n武动乾坤\n大主宰\n1\n2",
        ),
        (
            "class.book@tag.h3@text%%class.book@tag.a.0@href",
            "斗破苍穹\n/b/1\n武动乾坤\n/b/2\n大主宰\n/b/3",
        ),
        ("class.book.0@tag.h3@text##苍穹##乾坤", "斗破乾坤"),
        ("class.book@tag.h3@text##苍穹##乾坤##1", "乾坤"),
        (
            "class.author@text##{{@css:class.author.0@text}}##X",
            "X\nX\nX",
        ),
        ("class.book@tag.h3@text##^##[##", "["),
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== Jsoup 宽松 CSS（数字开头类名、`:has(>x)`、`:not(:matches(..))`、`[]` 属性值）");
    for (rule, expected) in [
        (".8col@text", "数字类名"),
        ("span.-2.-1@text", "负号类名"),
        ("div.outer:has(>.jYOxx)@text", "子元素命中"),
        ("div.outer2:has(>.jYOxx)@text", ""),
        ("div:not(:has(i)) span.jYOxx@text", "子元素命中"),
        (r"a:not(:matches(\S),[href~=^/x/])@href", ""),
        (r"a:not(:matches(二),[href~=^/x/])@href", "/y/1"),
        (r"a[href~=[_-]\d+$]@href", ""),
    ] {
        let mut ctx = EvalContext::new(Content::html(JSOUP_HTML)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== legado 的 Regex 模式（`SourceRule` 的 `Mode.Regex`：代入后就是结果）");
    for (rule, expected) in [
        ("class.book.0@tag.h3@text$1", "class.book.0@tag.h3@text$1"),
        ("class.book.0@tag.h3@text##苍穹##$1", "斗破$1"),
        ("class.book.0@tag.h3@text##苍穹##乾坤", "斗破乾坤"),
        (
            "class.book.0@tag.h3@text@get:{missing}",
            "class.book.0@tag.h3@text",
        ),
        (
            r#"class.book.0@tag.h3@text{{"!"}}"#,
            r#"class.book.0@tag.h3@text[js:"!"]"#,
        ),
        (
            "https://m.kanman.com/api/getchapterlist?product_id=1&productname=kmh&platformname=wap&comic_id=@get:{bid}@href",
            "https://m.kanman.com/api/getchapterlist?product_id=1&productname=kmh&platformname=wap&comic_id=@href",
        ),
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }
    {
        let mut ctx = EvalContext::new(Content::html(JSOUP_HTML));
        match block_on(engine.eval(r"a:not([href~=search\$0q=x])", &mut ctx)) {
            Ok(value) => check.check(
                "元素求值不吃 Regex 模式",
                "2",
                &value.into_list().len().to_string(),
            ),
            Err(error) => check.check("元素求值不吃 Regex 模式", "2", &format!("错误：{error}")),
        }
    }

    println!("== 文本空白口径（REPORT D6：只认 ASCII 空白）");
    {
        let mut ctx = EvalContext::new(Content::html(WS_HTML));
        match block_on(engine.eval_string("class.ws@text", &mut ctx)) {
            Ok(got) => check.check("全角空格不算空白", "　全角　", &got),
            Err(error) => check.check("全角空格不算空白", "　全角　", &format!("错误：{error}")),
        }
    }

    println!("== 列表规则（Elements 语义：所有 @ 段都是选择器）");
    for (rule, expected) in [
        ("class.books@li", "3"),
        ("ul.books@li", "3"),
        ("class.books@class.book", "3"),
        ("class.pages@a", "2"),
        ("ul.books,ul[class = books extra]", "1"),
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML));
        match block_on(engine.eval(rule, &mut ctx)) {
            Ok(value) => check.check(rule, expected, &value.into_list().len().to_string()),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== XPath");
    for (rule, expected) in [
        ("//h3[@class='title']/text()", "斗破苍穹\n武动乾坤\n大主宰"),
        ("//li[@class='book']/a/@href", "/b/1\n/b/2"),
        ("@xpath:string(//h3[1])", "斗破苍穹"),
        ("@xpath:count(//li)", "3"),
        ("@xpath:(//span[@class='author'])[1]/text()", "天蚕土豆"),
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML));
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    {
        let html = r#"<ul><li><span>甲</span> <a href="/x">第1章 第一章&nbsp;阎王帖</a></li></ul>"#;
        for (rule, expected) in [
            (
                "//li[span]/a@text",
                "<a href=\"/x\">第1章 第一章\u{a0}阎王帖</a>",
            ),
            (
                "//li[span]/a@href",
                "<a href=\"/x\">第1章 第一章\u{a0}阎王帖</a>",
            ),
            ("//a/@href", "/x"),
        ] {
            let mut ctx = EvalContext::new(Content::html(html));
            match block_on(engine.eval_string(rule, &mut ctx)) {
                Ok(got) => check.check(rule, expected, &got),
                Err(error) => check.check(rule, expected, &format!("错误：{error}")),
            }
        }
    }

    {
        let html = "<div id=\"d\">a&nbsp;b<span>c&nbsp;</span>&nbsp;<b>d</b>\u{a0}e</div>";
        let mut ctx = EvalContext::new(Content::html(html));
        let expected = "a b\ne";
        match block_on(engine.eval_string("#d@textNodes", &mut ctx)) {
            Ok(got) => check.check("#d@textNodes", expected, &got),
            Err(error) => check.check("#d@textNodes", expected, &format!("错误：{error}")),
        }
    }

    {
        let list = "<dl><dt>作者</dt><dd>甲</dd><dt>别的</dt><dd>乙</dd></dl>";
        for (rule, expected) in [
            ("dt+dd@text", "甲\n乙"),
            ("dt:contains(作者)+dd@text", "甲"),
            ("dt:contains(作者) + dd@text", "甲"),
            ("dt:contains(作者)+dd:contains(乙)@text", ""),
        ] {
            let mut ctx = EvalContext::new(Content::html(list));
            match block_on(engine.eval_string(rule, &mut ctx)) {
                Ok(got) => check.check(rule, expected, &got),
                Err(error) => check.check(rule, expected, &format!("错误：{error}")),
            }
        }
        let classes = "<div class=\"a b\">X</div><div class=\"a\">Y</div><div class=\"b\">Z</div>";
        for (rule, expected) in [
            ("class.a b@text", "X"),
            ("class.a@text", "X\nY"),
            ("class.a b c@text", ""),
            ("class.b a@text", ""),
        ] {
            let mut ctx = EvalContext::new(Content::html(classes));
            match block_on(engine.eval_string(rule, &mut ctx)) {
                Ok(got) => check.check(rule, expected, &got),
                Err(error) => check.check(rule, expected, &format!("错误：{error}")),
            }
        }
    }

    {
        let json = "{\"a\":[[\"\u{2026}\",\"x\"],[1,2]]}";
        let mut ctx = EvalContext::new(Content::text(json));
        match block_on(engine.eval_string("$.a", &mut ctx)) {
            Ok(got) => check.check("$.a 数组 JSON 转义", "[\"\\u2026\",\"x\"]\n[1,2]", &got),
            Err(error) => check.check("$.a 数组 JSON 转义", "", &format!("错误：{error}")),
        }
        let mut ctx = EvalContext::new(Content::text(json));
        match block_on(engine.eval_list("$.a", &mut ctx)) {
            Ok(got) => check.check(
                "$.a 数组 JSON 转义（列表链）",
                "[\"[\\\"\\\\u2026\\\",\\\"x\\\"]\", \"[1,2]\"]",
                &format!("{got:?}"),
            ),
            Err(error) => check.check(
                "$.a 数组 JSON 转义（列表链）",
                "",
                &format!("错误：{error}"),
            ),
        }
    }

    {
        let html = "<article id=\"content\"><p> a </p><p>  b  </p><span>c\td</span></article>";
        for (rule, expected) in [
            (
                "article@html",
                "<article id=\"content\">\n <p>a</p>\n <p>b</p><span>c d</span>\n</article>",
            ),
            (
                "article@all",
                "<article id=\"content\">\n <p>a</p>\n <p>b</p><span>c d</span>\n</article>",
            ),
        ] {
            let mut ctx = EvalContext::new(Content::html(html));
            match block_on(engine.eval_string(rule, &mut ctx)) {
                Ok(got) => check.check(rule, expected, &got),
                Err(error) => check.check(rule, expected, &format!("错误：{error}")),
            }
        }
        let mut ctx = EvalContext::new(Content::html("<pre> a\tb </pre>"));
        match block_on(engine.eval_string("pre@html", &mut ctx)) {
            Ok(got) => check.check("pre@html（保留空白）", "<pre> a\tb </pre>", &got),
            Err(error) => check.check("pre@html（保留空白）", "", &format!("错误：{error}")),
        }
        let mut ctx = EvalContext::new(Content::html("<div><p>x</p><script>s()</script></div>"));
        match block_on(engine.eval_string("div@html", &mut ctx)) {
            Ok(got) => check.check("div@html（摘掉 script）", "<div>\n <p>x</p>\n</div>", &got),
            Err(error) => check.check("div@html（摘掉 script）", "", &format!("错误：{error}")),
        }
        let mut ctx = EvalContext::new(Content::html("<div><p>x</p><script>s()</script></div>"));
        match block_on(engine.eval_string("div@all", &mut ctx)) {
            Ok(got) => check.check(
                "div@all（留着 script）",
                "<div>\n <p>x</p>\n <script>s()</script>\n</div>",
                &got,
            ),
            Err(error) => check.check("div@all（留着 script）", "", &format!("错误：{error}")),
        }
    }

    {
        let mut ctx = EvalContext::new(Content::text("{\"a\":1}"));
        match block_on(engine.eval_string("$.missing##(^)##$1X", &mut ctx)) {
            Ok(got) => check.check("$.missing + ## 替换", "X", &got),
            Err(error) => check.check("$.missing + ## 替换", "", &format!("错误：{error}")),
        }
        let mut ctx = EvalContext::new(Content::text("{\"a\":1}"));
        match block_on(engine.eval_list("$.missing##(^)##$1X", &mut ctx)) {
            Ok(got) => check.check("$.missing + ## 替换（列表链）", "[]", &format!("{got:?}")),
            Err(error) => check.check(
                "$.missing + ## 替换（列表链）",
                "",
                &format!("错误：{error}"),
            ),
        }
        let mut ctx = EvalContext::new(Content::text("abc123def"));
        match block_on(engine.eval_string(":(\\d+)", &mut ctx)) {
            Ok(got) => check.check("冒号前缀不是正则模式", "", &got),
            Err(error) => check.check("冒号前缀不是正则模式", "", &format!("错误：{error}")),
        }
        let mut ctx = EvalContext::new(Content::text("{\"a\":1}"));
        match block_on(engine.eval_string("description##^##X", &mut ctx)) {
            Ok(got) => check.check("JSON 内容 + 裸规则 + ## 替换", "X", &got),
            Err(error) => check.check(
                "JSON 内容 + 裸规则 + ## 替换",
                "",
                &format!("错误：{error}"),
            ),
        }
        let mut ctx = EvalContext::new(Content::html(
            "<html><body><div class=\"x\">正文</div></body></html>",
        ));
        match block_on(engine.eval_string("description@text##^##X", &mut ctx)) {
            Ok(got) => check.check("HTML 内容 + CSS 没选中 + ## 不替换", "", &got),
            Err(error) => check.check(
                "HTML 内容 + CSS 没选中 + ## 不替换",
                "",
                &format!("错误：{error}"),
            ),
        }
    }

    println!("== JsonPath");
    for (rule, expected) in [
        ("$.data.name", "斗破苍穹"),
        ("$.data.books[*].title", "斗破苍穹\n武动乾坤"),
        ("$.data.books[1].price", "20"),
        ("$..author", "天蚕土豆"),
        ("@json:$.data.books[*].title", "斗破苍穹\n武动乾坤"),
        ("data.name", "斗破苍穹"),
        ("$.data.book-draw", "测试"),
        ("$..book-draw", "测试"),
        ("$.data.missing", ""),
        (
            "$.data.books",
            "{title=斗破苍穹, price=10}\n{title=武动乾坤, price=20}",
        ),
        (
            "$..data.books",
            r#"[{"title":"斗破苍穹","price":10},{"title":"武动乾坤","price":20}]"#,
        ),
        ("{$.data.name}", "斗破苍穹"),
        ("{$.data.name}{$.data.missing}", "斗破苍穹{$.data.missing}"),
    ] {
        let mut ctx = EvalContext::new(Content::text(JSON));
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== 正则");
    for (rule, expected) in [
        (r#":<h3 class="title">([^<]+)</h3>"#, ""),
        (r#":<h3 class="title">([^<]+)</h3>&&斗破"#, ""),
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML));
        match block_on(engine.eval_list(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got.join("\n")),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== Jsoup 下标伪类（兄弟下标）");
    {
        let table = r#"<table><tr><td>t1</td><td>x</td><td>y</td></tr><tr><td>t2</td><td>z</td></tr></table>"#;
        for (rule, expected) in [
            ("td:eq(1)@text", "x\nz"),
            ("td:eq(0)@text", "t1\nt2"),
            ("td:lt(2)@text", "t1\nx\nt2\nz"),
            ("td:gt(0)@text", "x\ny\nz"),
        ] {
            let mut ctx = EvalContext::new(Content::html(table));
            match block_on(engine.eval_string(rule, &mut ctx)) {
                Ok(got) => check.check(rule, expected, &got),
                Err(error) => check.check(rule, expected, &format!("错误：{error}")),
            }
        }
    }

    println!("== `##` 正则编译失败退化成字面替换");
    {
        let mut ctx = EvalContext::new(Content::html(BRACKET_HTML));
        match block_on(engine.eval_string("class.b@text##[##X", &mut ctx)) {
            Ok(got) => check.check("坏正则退化成字面替换", "aXbXc", &got),
            Err(error) => check.check("坏正则退化成字面替换", "aXbXc", &format!("错误：{error}")),
        }
    }

    println!("== 正则引用 $n 与管道");
    {
        let mut ctx = EvalContext::new(Content::List(vec![
            "整体".to_string(),
            "组一".to_string(),
            "组二".to_string(),
        ]));
        match block_on(engine.eval_string("$2", &mut ctx)) {
            Ok(got) => check.check("$2", "组二", &got),
            Err(error) => check.check("$2", "组二", &format!("错误：{error}")),
        }
    }
    {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_string("$2", &mut ctx)) {
            Ok(got) => check.check("$2 非列表原样保留", "$2", &got),
            Err(error) => check.check("$2 非列表原样保留", "$2", &format!("错误：{error}")),
        }
    }
    {
        let mut ctx =
            EvalContext::new(Content::List(vec!["整体".into(), "组一".into()])).with_js(&js);
        match block_on(engine.eval_string("$0", &mut ctx)) {
            Ok(got) => check.check("$0 按字面量保留", "$0", &got),
            Err(error) => check.check("$0 按字面量保留", "$0", &format!("错误：{error}")),
        }
    }

    println!("== {{}} 与 @js:（占位实现）");
    for (rule, expected) in [
        ("{{ 1+1 }}", "[js: 1+1 ]"),
        ("class.book.0@tag.h3@text@js:return 1", "[js:return 1]"),
        ("<js>return 2</js>", "[js:return 2]"),
        (
            r#"{{ 'x'.match(/"/) ? 'y' : 'n' }}"#,
            r#"[js: 'x'.match(/"/) ? 'y' : 'n' ]"#,
        ),
        (
            r#"{{ 'x'.match(/"/) ? 'y' : 'n' }}||class.book@tag.h3@text"#,
            r#"[js: 'x'.match(/"/) ? 'y' : 'n' ]||class.book@tag.h3@text"#,
        ),
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }
    for (content, rule, expected) in [
        (Content::html(HTML), "{{ $.name }}", ""),
        (Content::html(HTML), "前缀{{$.id}}后缀", "前缀后缀"),
        (
            Content::Json(serde_json::json!({"name": "斗破苍穹"})),
            "{{$.name}}",
            "斗破苍穹",
        ),
        (
            Content::html(HTML),
            "{{@css:class.book.0@tag.h3@text}}",
            "斗破苍穹",
        ),
        (
            Content::html(HTML),
            "{{//h3[@class='title']/text()}}",
            "斗破苍穹\n武动乾坤\n大主宰",
        ),
        (
            Content::Json(serde_json::json!({"name": "斗破苍穹"})),
            "{{$['name']}}",
            "斗破苍穹",
        ),
    ] {
        let mut ctx = EvalContext::new(content).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== 步骤短路（REPORT D12）");
    {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_string("class.nope@js:xxxx", &mut ctx)) {
            Ok(got) => check.check("没选中则后续步骤短路", "", &got),
            Err(error) => check.check("没选中则后续步骤短路", "", &format!("错误：{error}")),
        }
    }
    for rule in [
        "class.nope@all\n@js:'ran'",
        "class.nope@all##a##b\n@js:'ran'",
        "class.nope##a##b\n@js:'ran'",
        "class.nope@text\n@js:'ran'",
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(&format!("短路：{rule}"), "", &got),
            Err(error) => check.check(&format!("短路：{rule}"), "", &format!("错误：{error}")),
        }
    }
    {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_string("class.book.0@tag.h3@text\n@js:return 1", &mut ctx)) {
            Ok(got) => check.check("选中时后续步骤照跑", "[js:return 1]", &got),
            Err(error) => check.check(
                "选中时后续步骤照跑",
                "[js:return 1]",
                &format!("错误：{error}"),
            ),
        }
    }

    println!("== @put / @get 与书籍、章节变量");
    {
        let mut source_book = Book {
            name: "斗破苍穹".into(),
            ..Default::default()
        };
        let mut ctx = EvalContext::new(Content::html(HTML))
            .with_js(&js)
            .with_book(&mut source_book)
            .with_base_url("https://example.com");
        let got = block_on(engine.eval_string(
            "class.book.0@tag.a.0@href@put:{bookid:class.book.1@tag.a.0@href}",
            &mut ctx,
        ));
        check.check(
            "@put 求值",
            "/b/1",
            &got.unwrap_or_else(|error| error.to_string()),
        );
        check.check(
            "@put 写进书籍变量",
            "/b/2",
            ctx.book
                .as_deref()
                .and_then(|book| book.get_variable("bookid"))
                .unwrap_or(""),
        );
        let got = block_on(engine.eval_string("@get:{bookid}", &mut ctx));
        check.check(
            "@get 读回变量",
            "/b/2",
            &got.unwrap_or_else(|error| error.to_string()),
        );
        let got = block_on(engine.eval_string("@get:{bookName}", &mut ctx));
        check.check(
            "@get:{bookName} 特殊键",
            "斗破苍穹",
            &got.unwrap_or_else(|error| error.to_string()),
        );
    }
    {
        let mut chapter = BookChapter {
            title: "第一章".into(),
            ..Default::default()
        };
        let mut ctx = EvalContext::new(Content::html(HTML))
            .with_js(&js)
            .with_chapter(&mut chapter);
        let _ = block_on(engine.eval_string("@put:{token:class.book.0@tag.h3@text}", &mut ctx));
        check.check(
            "@put 写进章节变量",
            "斗破苍穹",
            ctx.chapter
                .as_deref()
                .and_then(|chapter| chapter.get_variable("token"))
                .unwrap_or(""),
        );
        let got = block_on(engine.eval_string("@get:{title}", &mut ctx));
        check.check(
            "@get:{title} 特殊键",
            "第一章",
            &got.unwrap_or_else(|error| error.to_string()),
        );
    }

    println!("== trace 事件");
    {
        let trace = TraceBuffer::default();
        let mut ctx = EvalContext::new(Content::html(HTML))
            .with_js(&js)
            .with_trace(&trace);
        let _ = block_on(engine.eval_string("class.book.0@tag.h3@text<js>x</js>", &mut ctx));
        let records = trace.records();
        check.check("步数", "2", &records.len().to_string());
        check.check(
            "第一步输出",
            "斗破苍穹",
            records.first().map(|r| r.output.as_str()).unwrap_or(""),
        );
        check.check(
            "第二步规则",
            "x",
            records.get(1).map(|r| r.rule.as_str()).unwrap_or(""),
        );
    }

    println!("== 坏规则：延迟报错带原文/位置，能退化的退化成空");
    {
        let mut ctx = EvalContext::new(Content::html(HTML));
        let colon = match block_on(engine.eval_string(":[", &mut ctx)) {
            Ok(got) => format!("Ok({got})"),
            Err(error) => format!("Err({error})"),
        };
        check.check("冒号前缀退化成空（真机同）", "Ok()", &colon);
        let mut ctx = EvalContext::new(Content::html(HTML));
        let bad_css = match block_on(engine.eval_string("@css:>>>@text", &mut ctx)) {
            Ok(got) => format!("Ok({got})"),
            Err(error) => error.to_string(),
        };
        check.check(
            "坏 CSS 报错带位置",
            "true",
            &bad_css.contains("第 1 行第 6 列").to_string(),
        );
        check.check(
            "坏 CSS 报错带消息",
            "true",
            &bad_css.contains("CSS 选择器解析失败").to_string(),
        );
        let mut ctx = EvalContext::new(Content::html(HTML));
        match block_on(engine.eval_string("@xpath:not((", &mut ctx)) {
            Ok(got) => check.check("坏 XPath 退化成空", "", &got),
            Err(error) => check.check("坏 XPath 退化成空", "", &format!("错误：{error}")),
        }
        let mut plain = EvalContext::new(Content::html(HTML));
        match block_on(engine.eval_string("class.book@tag.h3@text || ", &mut plain)) {
            Ok(got) => check.check("空规则段保留左侧结果", "斗破苍穹\n武动乾坤\n大主宰", &got),
            Err(error) => check.check(
                "空规则段保留左侧结果",
                "斗破苍穹\n武动乾坤\n大主宰",
                &format!("错误：{error}"),
            ),
        }
        let no_js = match block_on(engine.eval_string("@js:1", &mut plain)) {
            Ok(got) => format!("Ok({got})"),
            Err(error) => error.to_string(),
        };
        check.check(
            "没接 JsEvaluator 时 @js 报错",
            "true",
            &no_js.contains("JsEvaluator").to_string(),
        );
    }

    println!("== getStringList 收尾：空串 / 空表 / 空规则 / 逐条替换");
    for (rule, expected) in [
        (
            "@put:{a:\"class.book@tag.h3@text\"}",
            format!("{:?}", HTML.split('\n').collect::<Vec<_>>()),
        ),
        (
            "@@class.book@tag.h3@html",
            "[\"<h3 class=\\\"title\\\">斗破苍穹</h3>\\n<h3 class=\\\"title\\\">武动乾坤</h3>\\n<h3 class=\\\"title\\\">大主宰</h3>\"]"
                .to_string(),
        ),
        ("$.code&&$.nope", "[]".to_string()),
        ("$.a||$", format!("{:?}", vec![HTML])),
        ("//*[@class=\"title\"]/@href", "[\"\", \"\", \"\"]".to_string()),
        (
            "class.book@tag.h3@text&&class.pages@tag.a@text",
            "[\"斗破苍穹\", \"武动乾坤\", \"大主宰\", \"1\", \"2\"]".to_string(),
        ),
        (
            "class.pages@tag.a@href##/p/##P",
            "[\"P1\", \"P2\"]".to_string(),
        ),
        ("class.nope@text##苍穹##乾坤", "[]".to_string()),
        (
            "class.book@tag.h3@text\nclass.pages@tag.a@text",
            "[]".to_string(),
        ),
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_list(rule, &mut ctx)) {
            Ok(got) => check.check(rule, &expected, &format!("{got:?}")),
            Err(error) => check.check(rule, &expected, &format!("错误：{error}")),
        }
    }
    for (rule, expected) in [
        ("class.book@tag.h3@text##苍穹##乾坤##1", "乾坤"),
        ("class.book@tag.h3@text##^##[##", "["),
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== `##`-only 步骤保持上一步的类型（813）");
    for (rule, expected) in [
        ("$.title\n<js>result</js>\n##</*\\w.*?>", "[]"),
        (
            "class.book@tag.h3@text\n##苍穹##乾坤",
            "[\"斗破乾坤\", \"武动乾坤\", \"大主宰\"]",
        ),
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&ResultEchoJs);
        match block_on(engine.eval_list(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &format!("{got:?}")),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== 空 `@js:` 规则整段跳过（7704）");
    {
        let rule = "class.nope@text@js:##.+##REPL##";
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_list(rule, &mut ctx)) {
            Ok(got) => check.check(rule, "[]", &format!("{got:?}")),
            Err(error) => check.check(rule, "[]", &format!("错误：{error}")),
        }
    }

    println!("== JsonSmart 宽松 JSON：单引号 + `$.[*]`（830）");
    const SQUOTE_JSON: &str = "[{'title':'暂无目录'}]";
    for (rule, expected) in [
        ("$.[*]", "[\"{title=暂无目录}\"]"),
        ("$[*]", "[\"{title=暂无目录}\"]"),
    ] {
        let mut ctx = EvalContext::new(Content::text(SQUOTE_JSON));
        match block_on(engine.eval_list(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &format!("{got:?}")),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }
    for (rule, content, expected) in [
        ("$.[*]", SQUOTE_JSON, "{title=暂无目录}"),
        ("$.[*]", "{title:'x'}", ""),
    ] {
        let mut ctx = EvalContext::new(Content::text(content));
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }
    {
        let rule = "$.missing||$.alsoMissing\n##^##X";
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, "X", &got),
            Err(error) => check.check(rule, "X", &format!("错误：{error}")),
        }
    }

    println!("== Jsoup 逗号选择器列表（5705）");
    const COMMA_HTML: &str = r#"<html><body><div class="pages">
  <a href="?page=1" class="current">1</a><a href="?page=2" class="next">下一页</a><a href="?page=3">3</a>
</div></body></html>"#;
    for (rule, expected) in [
        (
            "@css:a:contains(下一页), a:contains(下一頁)@href",
            "[\"?page=2\"]",
        ),
        ("@css:a:contains(下一页), a.next@href", "[\"?page=2\"]"),
        ("@css:a.next, a.current@href", "[\"?page=1\", \"?page=2\"]"),
        ("@css:a.current, a.next@href", "[\"?page=1\", \"?page=2\"]"),
    ] {
        let mut ctx = EvalContext::new(Content::html(COMMA_HTML)).with_js(&js);
        match block_on(engine.eval_list(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &format!("{got:?}")),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== Jsoup `text()` 的 tail 空格（块级元素后还有兄弟节点）");
    for (content, expected) in [
        ("<div><p>a</p><span>b</span></div>", "a b"),
        ("<div><p>a</p><p>b</p></div>", "a b"),
        ("<div>head<p>a</p></div>", "head a"),
        ("<div><p>a</p>tail</div>", "a tail"),
    ] {
        let rule = "tag.div@text";
        let mut ctx = EvalContext::new(Content::html(content)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(content, expected, &got),
            Err(error) => check.check(content, expected, &format!("错误：{error}")),
        }
    }

    println!("== `getString` 收尾还原 HTML 实体（`StringEscapeUtils.unescapeHtml4`）");
    for (rule, expected) in [
        ("&nbsp;A{{1}}", "\u{a0}A[js:1]"),
        ("&lrm;A{{1}}", "\u{200e}A[js:1]"),
        ("&#65;{{1}}", "A[js:1]"),
        ("&apos;A{{1}}", "&apos;A[js:1]"),
        ("&unknown;{{1}}", "&unknown;[js:1]"),
    ] {
        let mut ctx = EvalContext::new(Content::html("<p>x</p>")).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== XPath 取不到给空串（不是 null），后续步骤照跑");
    for (rule, expected) in [
        ("//td[4]/text()@js:'X'", "[js:'X']"),
        ("//a||//b@js:'X'", "[js:'X']"),
        ("//nope/text()", ""),
    ] {
        let mut ctx = EvalContext::new(Content::html("<div><p>hi</p></div>")).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== 代入占位符后规则文本为空 + 带 `##`：不求值，当前内容原样再替换");
    {
        let rule = "@get:{missing}##文";
        let mut ctx = EvalContext::new(Content::html("<p>abc文</p>")).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, "<p>abc</p>", &got),
            Err(error) => check.check(rule, "<p>abc</p>", &format!("错误：{error}")),
        }
    }

    println!("== `<noscript>` 里按标记解析（Jsoup 口径）");
    const NOSCRIPT_HTML: &str =
        "<body><noscript><div><span id=x>hit</span></div></noscript></body>";
    for rule in ["span@text", "noscript@text"] {
        let mut ctx = EvalContext::new(Content::html(NOSCRIPT_HTML)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, "hit", &got),
            Err(error) => check.check(rule, "hit", &format!("错误：{error}")),
        }
    }

    println!("== `&&` 一段都没产出时整段是 null（后面的 `@js:` 不跑）");
    for (rule, expected) in [
        ("class.nope@text&&class.also@text@js:'X'", ""),
        ("zzz@content&&zzz@content@js:'Y'+result", ""),
        ("class.nope@text&&class.book@text", "hi"),
    ] {
        let content = if rule.starts_with("class.book") || rule.starts_with("class.nope") {
            "<div class=book>hi</div>"
        } else {
            "<div><p>a</p></div>"
        };
        let mut ctx = EvalContext::new(Content::html(content)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== `@put:{{...}}` 抠掉后不留空白（legado 是整段删掉）");
    {
        let rule = "https://a/b?bookId={{$.nope}}@put:{bid:$.nope},X";
        let mut ctx = EvalContext::new(Content::html("<div class=book>hi</div>")).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, "https://a/b?bookId=,X", &got),
            Err(error) => check.check(rule, "https://a/b?bookId=,X", &format!("错误：{error}")),
        }
    }

    println!("== `<js>` 之间的纯空白分段不产生步骤");
    for (rule, expected) in [
        (
            "class.book@tag.h3@text\n<js>result</js>\n<js>result</js>",
            "[js:result]",
        ),
        ("class.book@tag.h3@text<js>result</js>\n", "[js:result]"),
    ] {
        let mut ctx = EvalContext::new(Content::html(HTML)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }

    println!("== XPath 文本节点走 Jsoup 空白口径（折叠 + 削首尾）");
    const XTEXT_HTML: &str = "<div><p>a<b>x</b>z  w</p><p>e<b>y</b>  </p></div>";
    for (rule, expected) in [("//p[1]/text()", "a\nz w"), ("//p[2]/text()", "e\n")] {
        let mut ctx = EvalContext::new(Content::html(XTEXT_HTML));
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(rule, expected, &got),
            Err(error) => check.check(rule, expected, &format!("错误：{error}")),
        }
    }
    {
        let rule = "//p[2]/text()";
        let mut ctx = EvalContext::new(Content::html(XTEXT_HTML));
        match block_on(engine.eval_list(rule, &mut ctx)) {
            Ok(got) => check.check(rule, "[\"e\", \"\"]", &format!("{got:?}")),
            Err(error) => check.check(rule, "[\"e\", \"\"]", &format!("错误：{error}")),
        }
    }

    println!("== CDATA 按 Jsoup 口径当字符数据（RSS 全是 CDATA）");
    for (content, rule, expected) in [
        (
            "<div><![CDATA[a<b>c</b>]]></div>",
            "tag.div@text",
            "a<b>c</b>",
        ),
        (
            "<div>前置<![CDATA[中]]>后置</div>",
            "tag.div@text",
            "前置中后置",
        ),
        ("<div><![CDATA[未闭合</div>", "tag.div@text", "未闭合</div>"),
        ("<div><![CDATA[a &amp; b]]></div>", "tag.div@text", "a & b"),
        (
            "<textarea><![CDATA[a<b>c</b>]]></textarea>",
            "tag.textarea@text",
            "<![CDATA[a<b>c</b>]]>",
        ),
    ] {
        let mut ctx = EvalContext::new(Content::html(content)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(content, expected, &got),
            Err(error) => check.check(content, expected, &format!("错误：{error}")),
        }
    }
    for (content, expected) in [
        ("<div><p>a</p><span>b</span></div>", "a b"),
        ("<div><p>a</p><description>b</description></div>", "ab"),
    ] {
        let rule = "tag.div@text";
        let mut ctx = EvalContext::new(Content::html(content)).with_js(&js);
        match block_on(engine.eval_string(rule, &mut ctx)) {
            Ok(got) => check.check(content, expected, &got),
            Err(error) => check.check(content, expected, &format!("错误：{error}")),
        }
    }

    check.guard(189);
    println!("\n{} 项，失败 {} 项", check.total, check.failures);
    if check.failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
