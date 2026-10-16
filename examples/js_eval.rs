use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use legado_rust_engine::core::{
    Book, BookChapter, BookSource, BoxFuture, Bytes, CookieStore, Error, HeaderMap, HttpFetcher,
    HttpRequest, HttpResponse, JsEvaluator, JsRequest, JsValue, JsVariables, Result, StatusCode,
};
use legado_rust_engine::js::JsEngine;
use legado_rust_engine::rule::{Content, EvalContext, RuleEngine};

struct Checks {
    passed: usize,
    failures: Vec<String>,
}

impl Checks {
    fn eq(&mut self, label: &str, actual: &str, expected: &str) {
        if actual == expected {
            self.passed += 1;
            println!("  ✓ {label}");
        } else {
            self.failures
                .push(format!("{label}: 得到 {actual:?}，期望 {expected:?}"));
            println!("  ✗ {label}: 得到 {actual:?}，期望 {expected:?}");
        }
    }

    fn truthy(&mut self, label: &str, ok: bool, detail: &str) {
        if ok {
            self.passed += 1;
            println!("  ✓ {label}");
        } else {
            self.failures.push(format!("{label}: {detail}"));
            println!("  ✗ {label}: {detail}");
        }
    }

    fn guard(&mut self, expected: usize) {
        let ran = self.passed + self.failures.len();
        if ran == expected {
            println!("  ✓ 断言执行数 {expected}");
        } else {
            let message = format!("断言执行数 {ran}，预期 {expected}（有分支被跳过？）");
            self.failures.push(message.clone());
            println!("  ✗ {message}");
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<bool> {
    let rules = Arc::new(RuleEngine::new());
    let engine = Arc::new(
        JsEngine::builder()
            .rules(rules.clone())
            .cookies(Arc::new(StubCookies::default()))
            .timeout(Duration::from_secs(3))
            .memory_limit(32 * 1024 * 1024)
            .android_id("0123456789abcdef")
            .build()?,
    );
    rules.set_js(engine.clone());
    let mut checks = Checks {
        passed: 0,
        failures: Vec::new(),
    };

    let source: BookSource = serde_json::from_str(
        r#"{
            "bookSourceUrl": "https://example.com",
            "bookSourceName": "示例源",
            "jsLib": "function twice(s){ return s + s }"
        }"#,
    )?;

    println!("\n== 全局绑定与 java.* ==");
    checks.eq(
        "result / baseUrl / page / key",
        &eval(
            &engine,
            &source,
            "result + '|' + baseUrl + '|' + page + '|' + key",
        )
        .await?,
        "正文|https://example.com|3|斗破",
    );
    checks.eq(
        "java.base64Encode + base64Decode",
        &eval(
            &engine,
            &source,
            "java.base64Encode('中文abc') + '|' + java.base64Decode(java.base64Encode('中文abc'))",
        )
        .await?,
        "5Lit5paHYWJj|中文abc",
    );
    checks.eq(
        "java.md5Encode",
        &eval(&engine, &source, "java.md5Encode('abc')").await?,
        "900150983cd24fb0d6963f7d28e17f72",
    );
    checks.eq(
        "java.hexEncodeToString / hexDecodeToString",
        &eval(
            &engine,
            &source,
            "java.hexEncodeToString('hi') + '|' + java.hexDecodeToString('6869')",
        )
        .await?,
        "6869|hi",
    );
    checks.eq(
        "java.encodeURI",
        &eval(&engine, &source, "java.encodeURI('斗 破')").await?,
        "%E6%96%97+%E7%A0%B4",
    );
    checks.eq(
        "java.strToBytes / bytesToStr 往返",
        &eval(
            &engine,
            &source,
            "java.bytesToStr(java.strToBytes('中文汉字'))",
        )
        .await?,
        "中文汉字",
    );
    checks.eq(
        "java.strToBytes 按字节给数组（JS 字符串 .length 才是 UTF-16 的 2）",
        &eval(
            &engine,
            &source,
            "java.strToBytes('中文').length + '|' + '中文'.length",
        )
        .await?,
        "6|2",
    );
    checks.eq(
        "java.randomUUID 是 v4 UUID",
        &eval(
            &engine,
            &source,
            "/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(java.randomUUID()) + ''",
        )
        .await?,
        "true",
    );
    checks.eq(
        "java.androidId",
        &eval(&engine, &source, "java.androidId()").await?,
        "0123456789abcdef",
    );
    checks.eq(
        "java.timeFormat 是 yyyy/MM/dd HH:mm（与 JS Date 对照）",
        &eval(
            &engine,
            &source,
            "(function () {\
               var t = 1600000000000;\
               var d = new Date(t);\
               var p = function (n) { return (n < 10 ? '0' : '') + n };\
               return '' + (java.timeFormat(t) === d.getFullYear() + '/' + p(d.getMonth() + 1) + '/' + p(d.getDate()) + ' ' + p(d.getHours()) + ':' + p(d.getMinutes()));\
             })()",
        )
        .await?,
        "true",
    );
    checks.eq(
        "顶层 return（书源里真有这种写法）",
        &eval(
            &engine,
            &source,
            "var a = 2; if (result) { return a * 3 } else { 'no' }",
        )
        .await?,
        "6",
    );
    checks.eq(
        "java.digestBase64Str sha256",
        &eval(&engine, &source, "java.digestBase64Str('abc','sha256')").await?,
        "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=",
    );
    checks.eq(
        "java.digestBase64Str 缺省 MD5",
        &eval(&engine, &source, "java.digestBase64Str('abc')").await?,
        "kAFQmDzST7DWlj99KOF/cg==",
    );
    checks.eq(
        "java.base64Decode 带 flags 参数",
        &eval(&engine, &source, "java.base64Decode('5Lit5paH', 0)").await?,
        "中文",
    );
    checks.eq(
        "java.base64Decode 容忍换行",
        &eval(&engine, &source, "java.base64Decode('5Lit\\n5paH')").await?,
        "中文",
    );
    checks.eq(
        "java.htmlFormat 只留 img",
        &eval(
            &engine,
            &source,
            "java.htmlFormat('<div>a</div><p>b</p><img src=\"/x.jpg\" width=\"3\">')",
        )
        .await?,
        "　　a\n　　b\n　　<img src=\"/x.jpg\">",
    );

    println!("\n== source / book / jsLib ==");
    checks.eq(
        "source.bookSourceUrl",
        &eval(&engine, &source, "source.bookSourceUrl").await?,
        "https://example.com",
    );
    checks.eq(
        "source.getKey()",
        &eval(&engine, &source, "source.getKey()").await?,
        "https://example.com",
    );
    checks.eq(
        "source.getLoginHeaderMap().get('x')",
        &eval(&engine, &source, "source.getLoginHeaderMap().get('x')").await?,
        "",
    );
    checks.eq(
        "book.name",
        &eval(&engine, &source, "book.name").await?,
        "斗破苍穹",
    );
    checks.eq(
        "jsLib 里的函数可见",
        &eval(&engine, &source, "twice('ab')").await?,
        "abab",
    );

    println!("\n== 宿主 API 补齐：hasClass / cookie.getKey / base64Decode 字符集 ==");
    checks.eq(
        "Elements.hasClass 任意元素命中（整词、空集 false）",
        &eval(
            &engine,
            &source,
            r#"var d=org.jsoup.Jsoup.parse('<div class="x no-hide">a</div><div class="y">b</div><div class="y hide">c</div>');
String(d.select('div').hasClass('hide'))+'|'+String(d.select('div.y').hasClass('hide'))+'|'+String(d.select('div.x').hasClass('hide'))+'|'+String(d.select('div.none').hasClass('hide'))"#,
        )
        .await?,
        "true|true|false|false",
    );
    checks.eq(
        "Element.hasClass 整词匹配",
        &eval(
            &engine,
            &source,
            r#"var d=org.jsoup.Jsoup.parse('<div class="x no-hide">a</div><div class="y">b</div>');
String(d.selectFirst('div.x').hasClass('hide'))+'|'+String(d.selectFirst('div.x').hasClass('no-hide'))+'|'+String(d.selectFirst('div.y').hasClass('y'))"#,
        )
        .await?,
        "false|true|true",
    );
    checks.eq(
        "hasClass 忽略大小写（真 jsoup 的 regionMatches(true)）",
        &eval(
            &engine,
            &source,
            r#"var d=org.jsoup.Jsoup.parse('<div class="Hide aB">x</div>');
String(d.select('div').hasClass('hide'))+'|'+String(d.select('div').hasClass('HIDE'))+'|'+String(d.select('div').hasClass('ab'))+'|'+String(d.selectFirst('div').hasClass('aB'))"#,
        )
        .await?,
        "true|true|true|true",
    );
    checks.eq(
        "cookie.getKey（含空值 / null / 无等号段的边界）",
        &eval(
            &engine,
            &source,
            r#"cookie.setCookie('https://a.com','x=1; y=2; z=; n=null; bare');
cookie.getKey('https://a.com','x')+'|'+cookie.getKey('https://a.com','y')+'|'+cookie.getKey('https://a.com','z')+'|'+cookie.getKey('https://a.com','n')+'|'+cookie.getKey('https://a.com','bare')+'|'+cookie.getKey('https://a.com','missing')"#,
        )
        .await?,
        "1|2||null||",
    );
    checks.eq(
        "java.getCookie(tag) / (tag, key)",
        &eval(
            &engine,
            &source,
            r#"cookie.setCookie('https://b.com','sid=abc; t=1');
java.getCookie('https://b.com')+'|'+java.getCookie('https://b.com','sid')+'|'+java.getCookie('https://b.com','missing')"#,
        )
        .await?,
        "sid=abc; t=1|abc|",
    );
    checks.eq(
        "java.aesBase64DecodeToString（openssl 生成的 AES-CBC 密文）",
        &eval(
            &engine,
            &source,
            "java.aesBase64DecodeToString('Ve8vJujltIu5KqsgI4dzrg==','0123456789abcdef','AES/CBC/PKCS5Padding','0123456789abcdef')",
        )
        .await?,
        "legado-rust",
    );

    checks.eq(
        "java.base64Decode(str, charset) 与 (str) 重载",
        &eval(
            &engine,
            &source,
            "java.base64Decode('1tDOxGFi','GBK')+'|'+java.base64Decode('5Lit5paHYWI=','UTF-8')+'|'+java.base64Decode('LU6HZWEAYgA=','UTF-16LE')+'|'+java.base64Decode('5Lit5paHYWI=')",
        )
        .await?,
        "中文ab|中文ab|中文ab|中文ab",
    );
    checks.eq(
        "java.getStringList 的 String() 走 Java 集合格式",
        &eval(
            &engine,
            &source,
            "String(java.getStringList('p@text','<p>甲</p><p>乙</p>'))+'|'+String(java.getStringList('p@text','<i>无</i>'))+'|'+('' + java.getStringList('p@text','<p>甲</p><p>乙</p>'))+'|'+java.getStringList('p@text','<p>甲</p><p>乙</p>').size()",
        )
        .await?,
        "[甲, 乙]|[]|[甲, 乙]|2",
    );

    println!("\n== java.put 写回书章变量 ==");
    let book = Book {
        name: "斗破苍穹".to_string(),
        ..Book::default()
    };
    let chapter = BookChapter::default();
    let mut variables = JsVariables::new(None, Some(book.variables.clone()), None);
    let value = engine
        .eval(JsRequest {
            script: "java.put('id', '42'); java.get('id')",
            source: Some(&source),
            base_url: "https://example.com",
            page: Some(1),
            key: None,
            result: None,
            src: None,
            response: None,
            book: Some(&book),
            chapter: Some(&chapter),
            variables: Some(&mut variables),
        })
        .await?;
    checks.eq("java.get 读回 java.put", &value, "42");
    checks.eq(
        "变量落在了书籍里",
        variables
            .book()
            .and_then(|map| map.get("id"))
            .map(String::as_str)
            .unwrap_or(""),
        "42",
    );

    let mut with_vars = book.clone();
    with_vars
        .variables
        .insert("k".to_string(), "书内".to_string());
    let chapter = BookChapter {
        title: "第一章".to_string(),
        ..BookChapter::default()
    };
    let mut variables = JsVariables::new(
        Some(chapter.variables.clone()),
        Some(with_vars.variables.clone()),
        None,
    );
    let value = engine
        .eval(JsRequest {
            script: "java.get('k') + '|' + java.put('k', '章内') + '|' + java.get('k')",
            source: Some(&source),
            base_url: "https://example.com",
            page: Some(1),
            key: None,
            result: None,
            src: None,
            response: None,
            book: Some(&with_vars),
            chapter: Some(&chapter),
            variables: Some(&mut variables),
        })
        .await?;
    checks.eq(
        "java.get 读时章节优先、章节没有才落到书籍",
        &value,
        "书内|章内|章内",
    );
    checks.eq(
        "java.put 有章节时写章节、书籍不被覆盖",
        &format!(
            "{}|{}",
            variables
                .chapter()
                .and_then(|map| map.get("k"))
                .map(String::as_str)
                .unwrap_or(""),
            variables
                .book()
                .and_then(|map| map.get("k"))
                .map(String::as_str)
                .unwrap_or(""),
        ),
        "章内|书内",
    );

    let mut var_book = Book {
        name: "斗破苍穹".to_string(),
        ..Book::default()
    };
    var_book
        .variables
        .insert("v".to_string(), "书内".to_string());
    let var_chapter = BookChapter {
        title: "第一章".to_string(),
        ..BookChapter::default()
    };
    let mut var_store = JsVariables::new(
        Some(var_chapter.variables.clone()),
        Some(var_book.variables.clone()),
        None,
    );
    let value = engine
        .eval(JsRequest {
            script: "chapter.putVariable('v', '章内') + '|' + java.get('v')",
            source: Some(&source),
            base_url: "https://example.com",
            page: Some(1),
            key: None,
            result: None,
            src: None,
            response: None,
            book: Some(&var_book),
            chapter: Some(&var_chapter),
            variables: Some(&mut var_store),
        })
        .await?;
    checks.eq(
        "chapter.putVariable 写章节变量、java.get 读章节优先",
        &value,
        "true|章内",
    );
    checks.eq(
        "chapter.putVariable 的改动跟着 variables 写回",
        var_store
            .chapter()
            .and_then(|map| map.get("v"))
            .map(String::as_str)
            .unwrap_or(""),
        "章内",
    );
    let value = engine
        .eval(JsRequest {
            script: "String(chapter.putVariable('big', 'y'.repeat(10000))) + '|' + java.get('big')",
            source: Some(&source),
            base_url: "https://example.com",
            page: Some(1),
            key: None,
            result: None,
            src: None,
            response: None,
            book: Some(&var_book),
            chapter: Some(&var_chapter),
            variables: Some(&mut var_store),
        })
        .await?;
    checks.eq(
        "chapter.putVariable 超过 10000 字不落表、仍返回 true",
        &value,
        "true|",
    );
    let value = engine
        .eval(JsRequest {
            script: "chapter.putVariable('v', null); java.get('v')",
            source: Some(&source),
            base_url: "https://example.com",
            page: Some(1),
            key: None,
            result: None,
            src: None,
            response: None,
            book: Some(&var_book),
            chapter: Some(&var_chapter),
            variables: Some(&mut var_store),
        })
        .await?;
    checks.eq(
        "chapter.putVariable(k, null) 是删除（读回到书籍那份）",
        &value,
        "书内",
    );

    println!("\n== java.ajax 接 HttpFetcher ==");
    let http = Arc::new(StubHttp::default());
    let ajax_engine = JsEngine::builder().http(http.clone()).build()?;
    checks.eq(
        "java.ajax 走 HttpFetcher",
        &eval(&ajax_engine, &source, "java.ajax('https://example.com/a')").await?,
        "stub:GET:https://example.com/a:",
    );
    checks.eq(
        "java.connect 的 ,{...} 选项（method/body）",
        &eval(
            &ajax_engine,
            &source,
            "java.connect('https://example.com/a,{\"method\":\"POST\",\"body\":\"b=1\"}').body()",
        )
        .await?,
        "stub:POST:https://example.com/a:b=1",
    );
    checks.eq(
        "java.get(url, headers) 走 HTTP",
        &eval(
            &ajax_engine,
            &source,
            "java.get('https://example.com/a', {\"X-Test\":\"1\"}).code",
        )
        .await?,
        "200",
    );
    {
        let seen = http.seen.lock().expect("记录锁被毒化");
        let passed = seen
            .last()
            .map(|request| {
                request.url == "https://example.com/a"
                    && request
                        .headers
                        .get("X-Test")
                        .and_then(|value| value.to_str().ok())
                        == Some("1")
            })
            .unwrap_or(false);
        checks.truthy(
            "java.get 的第二参数 header 传到了 HttpFetcher",
            passed,
            &format!("HttpFetcher 最后收到的请求: {:?}", seen.last()),
        );
    }
    println!("\n== 空 / 字面 null 内容的 JsonPath，以及 JS 的「当前内容」");
    {
        let mut empty_ctx = EvalContext::new(Content::text("")).with_js(engine.as_ref());
        let empty_error = rules
            .eval_string("$.a", &mut empty_ctx)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        checks.truthy(
            "空内容 + $.a 报 json string can not be null or empty",
            empty_error.contains("json string can not be null or empty"),
            &empty_error,
        );
        let mut null_ctx = EvalContext::new(Content::text("null")).with_js(engine.as_ref());
        let null_error = rules
            .eval_string("$.a", &mut null_ctx)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        checks.truthy(
            "字面 null 内容 + $.a 报 json can not be null",
            null_error.contains("json can not be null"),
            &null_error,
        );
        let mut blank_ctx = EvalContext::new(Content::text(" ")).with_js(engine.as_ref());
        checks.eq(
            "空白内容不报错，取不到就是空",
            &rules
                .eval_string("$.a", &mut blank_ctx)
                .await
                .unwrap_or_else(|error| format!("err {error}")),
            "",
        );
        let mut caught_ctx = EvalContext::new(Content::text("")).with_js(engine.as_ref());
        checks.eq(
            "JS 里能 catch 住同一个错（legado 也是 WrappedException）",
            &rules
                .eval_string(
                    "@js:try{java.getString('$.a')}catch(e){'caught'}",
                    &mut caught_ctx,
                )
                .await?,
            "caught",
        );
        let mut src_ctx = EvalContext::new(Content::text("{\"a\":1}")).with_js(engine.as_ref());
        checks.eq(
            "列表链：上一步取空后，JS 里的 java.getString 仍看原文",
            &format!(
                "{:?}",
                rules
                    .eval_list("$.missing\n@js:java.getString('$.a')", &mut src_ctx)
                    .await?
            ),
            "[\"1\"]",
        );
    }

    println!("\n== java.ajax 的源限速 & ajaxAll 并发");
    {
        let http = Arc::new(ConcurrentHttp::default());
        let engine = JsEngine::builder().http(http.clone()).build()?;
        let mut rated: BookSource = source.clone();
        rated.base.concurrent_rate = Some("2/1000".into());
        checks.eq(
            "java.ajax 请求成功（拿回正文）",
            &eval(&engine, &rated, "java.ajax('https://example.com/rated')").await?,
            "concurrent:https://example.com/rated",
        );
        checks.eq(
            "java.ajax 把源的 concurrentRate 传给取页层",
            &http.view(|seen| seen.rate.clone().unwrap_or_default()),
            "2/1000",
        );
        checks.eq(
            "限速的 key 用源地址（每个源一份限速记录）",
            &http.view(|seen| seen.rate_key.clone()),
            "https://example.com",
        );
        checks.eq(
            "ajaxAll 返回同长度数组",
            &eval(
                &engine,
                &source,
                "java.ajaxAll(['https://example.com/1','https://example.com/2','https://example.com/3']).length",
            )
            .await?,
            "3",
        );
        checks.eq(
            "ajaxAll 三个请求同时在飞（最大并发）",
            &http.view(|seen| seen.max_in_flight.to_string()),
            "3",
        );
    }

    println!("\n== java.cacheFile / importScript（缓存远端文本资源）==");
    {
        let http = Arc::new(StubHttp::default());
        let cache: Arc<dyn legado_rust_engine::core::CacheStore> =
            Arc::new(legado_rust_engine::core::MemoryCacheStore::new(16));
        let engine = JsEngine::builder()
            .http(http.clone())
            .cache(cache)
            .build()?;
        let url = "https://example.com/lib.js";
        checks.eq(
            "cacheFile 第一次：下载并返回文本",
            &eval(&engine, &source, &format!("java.cacheFile('{url}')")).await?,
            "stub:GET:https://example.com/lib.js:",
        );
        checks.eq(
            "下载次数 1",
            &http.seen.lock().expect("记录锁被毒化").len().to_string(),
            "1",
        );
        checks.eq(
            "cacheFile 第二次：命中缓存不再下载（saveTime 秒 TTL）",
            &eval(&engine, &source, &format!("java.cacheFile('{url}', 60)")).await?,
            "stub:GET:https://example.com/lib.js:",
        );
        checks.eq(
            "下载次数仍是 1",
            &http.seen.lock().expect("记录锁被毒化").len().to_string(),
            "1",
        );
        checks.eq(
            "importScript(http) 复用 cacheFile 的缓存",
            &eval(&engine, &source, &format!("java.importScript('{url}')")).await?,
            "stub:GET:https://example.com/lib.js:",
        );
        checks.eq(
            "下载次数还是 1",
            &http.seen.lock().expect("记录锁被毒化").len().to_string(),
            "1",
        );
        let error = eval(&engine, &source, "java.importScript('/storage/x.js')")
            .await
            .expect_err("本地路径应该报错");
        checks.truthy(
            "importScript 的本地路径报清楚（本地文件不做）",
            error.to_string().contains("本地文件不做"),
            &error.to_string(),
        );
    }

    checks.eq(
        "java.ajaxAll 返回同长度响应对象数组",
        &eval(
            &ajax_engine,
            &source,
            "(function () {\
               var r = java.ajaxAll(['https://example.com/a','https://example.com/b']);\
               return r.length + '|' + r[0].body() + '|' + r[1].body();\
             })()",
        )
        .await?,
        "2|stub:GET:https://example.com/a:|stub:GET:https://example.com/b:",
    );
    checks.eq(
        "java.head 用 HEAD",
        &eval(
            &ajax_engine,
            &source,
            "java.head('https://example.com/a', {}).statusCode() + '|' + java.head('https://example.com/a', {}).body()",
        )
        .await?,
        "200|stub:HEAD:https://example.com/a:",
    );
    checks.eq(
        "java.post(url, body) 用 POST",
        &eval(
            &ajax_engine,
            &source,
            "java.post('https://example.com/a', 'b=1').body()",
        )
        .await?,
        "stub:POST:https://example.com/a:b=1",
    );
    checks.eq(
        "可选参数：connect(url) / head(url) / getVariable(key) 单参调用",
        &eval(
            &ajax_engine,
            &source,
            "(function () {\
               var response = java.connect('https://example.com/a');\
               var head = java.head('https://example.com/a');\
               return response.body() + '|' + head.statusCode() + '|' + source.getVariable('x');\
             })()",
        )
        .await?,
        "stub:GET:https://example.com/a:|200|",
    );
    let no_fetcher = eval(
        &engine,
        &source,
        "(function () { try { return '' + java.ajax('https://example.com/a') } catch (e) { return 'THREW:' + e } })()",
    )
    .await?;
    checks.truthy(
        "没接 HttpFetcher 时 java.ajax 不抛、错误文本点名 HttpFetcher（D19）",
        !no_fetcher.starts_with("THREW:") && no_fetcher.contains("HttpFetcher"),
        &format!("得到 {no_fetcher:?}"),
    );

    let failing = JsEngine::builder().http(Arc::new(FailingHttp)).build()?;
    let failed_ajax = eval(
        &failing,
        &source,
        "(function () { try { return '' + java.ajax('https://example.com/x') } catch (e) { return 'THREW:' + e } })()",
    )
    .await?;
    checks.truthy(
        "java.ajax 请求失败不抛、把错误文本当正文（D19）",
        !failed_ajax.starts_with("THREW:") && failed_ajax.contains("stub: 网络断了"),
        &format!("得到 {failed_ajax:?}"),
    );
    checks.eq(
        "java.ajaxAll 失败的项给 null（D19）",
        &eval(
            &failing,
            &source,
            "(function () { var r = java.ajaxAll(['https://example.com/x']); return r.length + '|' + String(r[0]); })()",
        )
        .await?,
        "1|null",
    );

    println!("\n== 超时与内存上限 ==");
    let timeout = JsEngine::builder()
        .timeout(Duration::from_millis(300))
        .build()?;
    let started = std::time::Instant::now();
    let error = eval(&timeout, &source, "while (true) {}")
        .await
        .unwrap_err();
    checks.truthy(
        "死循环被中断",
        started.elapsed() < Duration::from_secs(2) && error.to_string().contains("interrupt"),
        &format!("{}ms 后返回 {error}", started.elapsed().as_millis()),
    );

    let small = JsEngine::builder()
        .memory_limit(8 * 1024 * 1024)
        .timeout(Duration::from_secs(3))
        .build()?;
    let error = eval(
        &small,
        &source,
        "let a=[]; while(true){ a.push(new Array(10000).fill('x')) }",
    )
    .await
    .unwrap_err();
    checks.truthy(
        "超内存报错不炸进程",
        error.to_string().contains("out of memory"),
        &error.to_string(),
    );

    println!("\n== 作用域与求值模式（非严格 / D23 / 顶层 return）==");
    checks.eq(
        "非严格求值：未声明赋值可用、显式 use strict 仍然生效",
        &eval(
            &engine,
            &source,
            "(function () {\
               var sloppy = (implicit_global = 'ok');\
               var strict = (function () { 'use strict'; try { undeclared_in_strict = 1; return 'no-error' } catch (e) { return 'ReferenceError' } })();\
               return sloppy + '|' + strict;\
             })()",
        )
        .await?,
        "ok|ReferenceError",
    );
    let _ = eval(
        &engine,
        &source,
        "let block_scoped = 1; const also_scoped = 2; var kept_var = 3; kept_implicit = 4; ''",
    )
    .await?;
    checks.eq(
        "顶层 let/const/var 都不跨次残留、隐式全局仍在（D23）",
        &eval(
            &engine,
            &source,
            "[typeof block_scoped, typeof also_scoped, typeof kept_var, typeof kept_implicit].join('|')",
        )
        .await?,
        "undefined|undefined|undefined|number",
    );
    let runtime_error = eval(
        &engine,
        &source,
        "return_runs = (globalThis.return_runs || 0) + 1; null.x",
    )
    .await;
    let runs = eval(&engine, &source, "'' + globalThis.return_runs").await?;
    checks.truthy(
        "顶层 return 的兜底只认那个语法错误：运行时错误不重跑",
        runtime_error.is_err() && runs == "1",
        &format!("第一次求值 {runtime_error:?}，之后 return_runs={runs}"),
    );

    println!("\n== 接进规则引擎 ==");
    let rules = RuleEngine::new();
    let mut ctx = EvalContext::new(Content::html("<div class='book'>斗破苍穹</div>"))
        .with_source(&source)
        .with_js(engine.as_ref())
        .with_base_url("https://example.com");
    checks.truthy(
        "@js: 拿得到当前内容",
        rules
            .eval_string(
                "@js:result.indexOf('斗破苍穹') > 0 ? 'yes' : 'no'",
                &mut ctx,
            )
            .await?
            == "yes",
        "result 里没有正文",
    );
    checks.eq(
        "选择器 + @js: 串联",
        &rules
            .eval_string(
                "class.book@text@js:java.md5Encode(result).slice(0,8)",
                &mut ctx,
            )
            .await?,
        &md5_of("斗破苍穹")[..8],
    );
    checks.eq(
        "规则链路里 src 是当前内容（D9）",
        &rules
            .eval_string("@js:src.indexOf('斗破苍穹') >= 0 ? 'yes' : 'no'", &mut ctx)
            .await?,
        "yes",
    );

    println!("\n== java.getString / getElement / setContent ==");
    let mut ctx = EvalContext::new(Content::html(
        "<div class='book'><h3><a href='/b/1.htm'>斗破苍穹</a></h3><p class='x'>正文一</p><p>正文二</p></div>",
    ))
    .with_source(&source)
    .with_base_url("https://example.com")
    .with_js(engine.as_ref());
    checks.eq(
        "java.getString 拿当前内容跑选择器",
        &rules
            .eval_string("@js:java.getString('h3 a@text')", &mut ctx)
            .await?,
        "斗破苍穹",
    );
    checks.eq(
        "java.getString 的第二参数也可以是内容",
        &rules
            .eval_string("@js:java.getString('b@text', '<b>另一段</b>')", &mut ctx)
            .await?,
        "另一段",
    );
    checks.eq(
        "java.setContent 换内容（链式返回 java）",
        &rules
            .eval_string(
                "@js:java.setContent('<b id=x>新正文</b>') && java.getString('id.x@text')",
                &mut ctx,
            )
            .await?,
        "新正文",
    );
    checks.eq(
        "嵌套 @js: 不死锁（内联重入）",
        &rules
            .eval_string(
                "@js:java.getString('h3 a@text@js:\"[\" + result + \"]\"')",
                &mut ctx,
            )
            .await?,
        "[斗破苍穹]",
    );
    checks.eq(
        "java.getElements 的集合长度",
        &rules
            .eval_string("@js:'' + java.getElements('p').length", &mut ctx)
            .await?,
        "2",
    );
    checks.eq(
        "元素对象的 text()",
        &rules
            .eval_string("@js:java.getElements('p')[0].text()", &mut ctx)
            .await?,
        "正文一",
    );
    checks.eq(
        "元素对象的 attr()",
        &rules
            .eval_string("@js:java.getElement('h3 a')[0].attr('href')", &mut ctx)
            .await?,
        "/b/1.htm",
    );
    checks.eq(
        "元素对象的 outerHtml() 就是这段 HTML",
        &rules
            .eval_string("@js:java.getElement('h3 a')[0].outerHtml()", &mut ctx)
            .await?,
        "<a href=\"/b/1.htm\">斗破苍穹</a>",
    );
    checks.eq(
        "元素对象的 select() 二次选择",
        &rules
            .eval_string(
                "@js:java.getElement('div.book')[0].select('p').length",
                &mut ctx,
            )
            .await?,
        "2",
    );
    checks.eq(
        "java.getString 的布尔参数是 unescape（默认还原实体，false 保留）",
        &rules
            .eval_string(
                "@js:java.setContent('<b id=y>a&amp;b</b>') && (java.getString('id.y@html').indexOf('&amp;') >= 0) + '|' + (java.getString('id.y@html', false).indexOf('&amp;') >= 0)",
                &mut ctx,
            )
            .await?,
        "false|true",
    );

    println!("\n== java.getStringList / toNumChapter / timeFormatUTC ==");
    let mut list_ctx = EvalContext::new(Content::html(
        "<div class='book'><p class='x'>正文一</p><p>正文二</p><a href='/b/1.htm'>链接</a></div>",
    ))
    .with_source(&source)
    .with_base_url("https://example.com")
    .with_js(engine.as_ref());
    checks.eq(
        "java.getStringList 每个选中元素一条（List 的 size / get）",
        &rules
            .eval_string(
                "@js:'' + java.getStringList('p@text').size() + '|' + java.getStringList('p@text').get(1)",
                &mut list_ctx,
            )
            .await?,
        "2|正文二",
    );
    checks.eq(
        "java.getStringList 的 toArray 之后按数组用（语料同款写法）",
        &rules
            .eval_string(
                "@js:java.getStringList('p@text').toArray().join('|')",
                &mut list_ctx,
            )
            .await?,
        "正文一|正文二",
    );
    checks.eq(
        "java.getStringList 的单条自带换行不拆（长度 1）",
        &rules
            .eval_string(
                "@js:java.setContent('<div class=x><p>a</p>\\n<p>b</p></div>') && java.getStringList('div.x@html').length",
                &mut list_ctx,
            )
            .await?,
        "1",
    );
    checks.eq(
        "java.getStringList 的单条里换行还在",
        &rules
            .eval_string(
                "@js:java.setContent('<div class=x><p>a</p>\\n<p>b</p></div>') && java.getStringList('div.x@html').get(0).indexOf(String.fromCharCode(10)) >= 0",
                &mut list_ctx,
            )
            .await?,
        "true",
    );

    checks.eq(
        "java.getStringList 第二参数是 mContent",
        &rules
            .eval_string(
                "@js:java.getStringList('p@text', '<p>换内容</p><p>再来一段</p>').join('|')",
                &mut list_ctx,
            )
            .await?,
        "换内容|再来一段",
    );
    checks.eq(
        "java.getStringList 没选中给空表、规则为空给 null",
        &rules
            .eval_string(
                "@js:'' + java.getStringList('nope@text').size() + '|' + java.getStringList('')",
                &mut list_ctx,
            )
            .await?,
        "0|null",
    );
    let oob = eval(
        &engine,
        &source,
        "try { java.getStringList('p@text','<p>甲</p><p>乙</p>').get(2); '不抛' } \
         catch (e) { '抛了: ' + e }",
    )
    .await?;
    checks.truthy(
        "java.getStringList(...).get(越界) 抛 IndexOutOfBoundsException",
        oob.contains("抛了") && oob.contains("Index 2 out of bounds for length 2"),
        &oob,
    );
    let negative = eval(
        &engine,
        &source,
        "try { java.getStringList('p@text','<p>甲</p><p>乙</p>').get(-1); '不抛' } \
         catch (e) { '抛了: ' + e }",
    )
    .await?;
    checks.truthy(
        "java.getStringList(...).get(负数) 也抛（不是 undefined）",
        negative.contains("抛了") && negative.contains("Index -1 out of bounds for length 2"),
        &negative,
    );
    checks.eq(
        "java.getStringList(...).get('1') 按 ToInt32 收（harness 现跑）",
        &eval(
            &engine,
            &source,
            "java.getStringList('p@text','<p>甲</p><p>乙</p>').get('1')",
        )
        .await?,
        "乙",
    );
    checks.eq(
        "java.getStringList(..., true) 补全相对地址 + 去重",
        &eval(
            &engine,
            &source,
            r#"java.getStringList('a@href','<a href="/rel">x</a><a href="http://abs/x">y</a><a href="/rel">d</a><a href="javascript:void(0)">j</a>', true).toArray().join('|')"#,
        )
        .await?,
        "https://example.com/rel|http://abs/x",
    );
    checks.eq(
        "java.getStringList(..., false) 不做地址补全",
        &eval(
            &engine,
            &source,
            "java.getStringList('p@text','<p>甲</p><p>乙</p>', false).toArray().join('|')",
        )
        .await?,
        "甲|乙",
    );

    println!("\n== 列表结果过桥：result 的结构（9895 / 8362）==");
    checks.eq(
        "Java List 空表也是 truthy（`9895:*:list` 靠它整段跳过 ajax）",
        &js_value_text(
            &eval_with_items(&engine, &source, "!result ? 'falsy' : 'truthy'", Some(&[])).await?,
        ),
        "truthy",
    );
    checks.eq(
        "Java List 的 size / isEmpty / toString",
        &js_value_text(
            &eval_with_items(
                &engine,
                &source,
                "String(result) + '|' + result.size() + '|' + result.isEmpty()",
                Some(&[]),
            )
            .await?,
        ),
        "[]|0|true",
    );
    checks.eq(
        "Java List 的 get / typeof（legado 是 object，不是 string）",
        &js_value_text(
            &eval_with_items(
                &engine,
                &source,
                "String(result) + '|' + result.get(0) + '|' + (typeof result)",
                Some(&["甲".to_string()]),
            )
            .await?,
        ),
        "[甲]|甲|object",
    );
    checks.eq(
        "没有列表结构时 result 仍是字符串",
        &js_value_text(
            &eval_with_items(&engine, &source, "result + '|' + (typeof result)", None).await?,
        ),
        "正文|string",
    );
    checks.eq(
        "脚本整段返回 getStringList：文本按 AbstractCollection.toString()",
        &format!(
            "{}|{}",
            eval(
                &engine,
                &source,
                "java.getStringList('p@text','<p>甲</p><p>乙</p>')"
            )
            .await?,
            eval(&engine, &source, "java.getStringList('p@text','<i>无</i>')").await?
        ),
        "[甲, 乙]|[]",
    );
    checks.eq(
        "脚本整段返回 getStringList：结构是 List 而不是文本",
        &format!(
            "{:?}",
            eval_with_items(
                &engine,
                &source,
                "java.getStringList('p@text','<i>无</i>')",
                None,
            )
            .await?
        ),
        "List([])",
    );
    checks.eq(
        "java.t2s 收 List 参数按 toString() 强转（Rhino 语义，`1047` / `362`）",
        &js_value_text(
            &eval_with_items(
                &engine,
                &source,
                "java.t2s(result)",
                Some(&["甲".to_string(), "乙".to_string()]),
            )
            .await?,
        ),
        "[甲, 乙]",
    );

    let mut bridge_ctx = EvalContext::new(Content::text("<html>not json</html>"))
        .with_base_url("https://example.com")
        .with_js(engine.as_ref());
    let truthy_rule = "$.data.book.description\n<js>if(!result){ 'taken' }else{ result }</js>";
    checks.eq(
        "列表模式：上一步的空结果绑成空 List，`!result` 为假（`9895:*:list`）",
        &format!("{:?}", rules.eval_list(truthy_rule, &mut bridge_ctx).await?),
        "[]",
    );
    checks.eq(
        "字符串模式：上一步的空结果绑成空串，`!result` 为真（`9895:*:string`）",
        &rules.eval_string(truthy_rule, &mut bridge_ctx).await?,
        "taken",
    );

    let joined_rule =
        "id.x@text\n<js>'n=' + result.size() + '|' + result.get(0) + '|' + result.get(1)</js>";
    let mut two_items_ctx = EvalContext::new(Content::text(
        "<html><p id=\"x\">a</p><p id=\"x\">b</p></html>",
    ))
    .with_js(engine.as_ref());
    checks.eq(
        "列表模式：上一步的选择器结果带 size / get（对象，不是字符串）",
        &format!(
            "{:?}",
            rules.eval_list(joined_rule, &mut two_items_ctx).await?
        ),
        "[\"n=2|a|b\"]",
    );
    let string_error = rules.eval_string(joined_rule, &mut two_items_ctx).await;
    checks.truthy(
        "字符串模式：上一步是字符串，脚本调 `.size()` 报错（legado 同）",
        string_error.is_err(),
        &format!("期望报错，得到 {string_error:?}"),
    );
    let mut step0_ctx = EvalContext::new(Content::text("abc")).with_js(engine.as_ref());
    checks.eq(
        "列表模式：第一步的 result 还是原始内容字符串（不是 List）",
        &format!(
            "{:?}",
            rules
                .eval_list(
                    "<js>if(!result){ 'taken' }else{ 'skipped' }</js>",
                    &mut step0_ctx
                )
                .await?
        ),
        "[\"skipped\"]",
    );
    let mut template_ctx =
        EvalContext::new(Content::text("<html><div>X</div></html>")).with_js(engine.as_ref());
    checks.eq(
        "列表模式：模板字面量那一步给字符串，不是 List（`1001:*:list`）",
        &format!(
            "{:?}",
            rules
                .eval_list(
                    "{{@@div@text}}•{{@@div@text}}\n<js>typeof result+'|'+result</js>",
                    &mut template_ctx
                )
                .await?
        ),
        "[\"string|X•X\"]",
    );
    checks.eq(
        "列表模式：脚本整段返回 List 时按列表给",
        &format!(
            "{:?}",
            rules
                .eval_list(
                    "id.x@text\n<js>result</js>",
                    &mut EvalContext::new(Content::text(
                        "<html><p id=\"x\">a</p><p id=\"x\">b</p></html>"
                    ))
                    .with_js(engine.as_ref())
                )
                .await?
        ),
        "[\"a\", \"b\"]",
    );
    let mut empty_list_ctx =
        EvalContext::new(Content::text("<html></html>")).with_js(engine.as_ref());
    checks.eq(
        "getStringList 整段返回：字符串模式给 `[]`（`8362:*:string`）",
        &rules
            .eval_string(
                "@js:java.getStringList('id.nope@html')",
                &mut empty_list_ctx,
            )
            .await?,
        "[]",
    );
    checks.eq(
        "getStringList 整段返回：列表模式给空列表而不是 `[\"[]\"]`（`8362:*:list`）",
        &format!(
            "{:?}",
            rules
                .eval_list(
                    "@js:java.getStringList('id.nope@html')",
                    &mut empty_list_ctx
                )
                .await?
        ),
        "[]",
    );

    checks.eq(
        "java.toNumChapter 中文数字（十二 → 12），前后文本丢掉",
        &eval(&engine, &source, "java.toNumChapter('第十二章 风起')").await?,
        "第12章",
    );
    checks.eq(
        "java.toNumChapter 百位（一百二十三 → 123）",
        &eval(&engine, &source, "java.toNumChapter('第一百二十三章')").await?,
        "第123章",
    );
    checks.eq(
        "java.toNumChapter 全角数字先转半角（００７ → 7）",
        &eval(&engine, &source, "java.toNumChapter('第００７章')").await?,
        "第7章",
    );
    checks.eq(
        "java.toNumChapter 认不出的字给 -1；没有「第…章」原样返回",
        &eval(
            &engine,
            &source,
            "java.toNumChapter('第X章') + '|' + java.toNumChapter('第一卷 陨落的天才')",
        )
        .await?,
        "第-1章|第一卷 陨落的天才",
    );
    checks.eq(
        "java.toNumChapter(null) 给 null",
        &eval(&engine, &source, "'' + java.toNumChapter(null)").await?,
        "null",
    );

    checks.eq(
        "java.timeFormatUTC(0, …, 0) 是 UTC 基准",
        &eval(
            &engine,
            &source,
            "java.timeFormatUTC(0, 'yyyy-MM-dd HH:mm', 0)",
        )
        .await?,
        "1970-01-01 00:00",
    );
    checks.eq(
        "java.timeFormatUTC +8 小时（28800000 毫秒）+ 中文格式串",
        &eval(
            &engine,
            &source,
            "java.timeFormatUTC(0, 'yyyy年MM月dd日 HH:mm', 28800000)",
        )
        .await?,
        "1970年01月01日 08:00",
    );
    checks.eq(
        "java.timeFormatUTC 负偏移（-28800000 毫秒 → 前一天）",
        &eval(
            &engine,
            &source,
            "java.timeFormatUTC(0, 'MM-dd HH:mm', -28800000)",
        )
        .await?,
        "12-31 16:00",
    );
    checks.eq(
        "java.timeFormatUTC 语料同款：sh=8（毫秒，不是小时）",
        &eval(
            &engine,
            &source,
            "java.timeFormatUTC(1700000000000, 'yyyy-MM-dd', 8)",
        )
        .await?,
        "2023-11-14",
    );
    checks.eq(
        "java.timeFormatUTC 斜杠 + 带秒（UTC+8）",
        &eval(
            &engine,
            &source,
            "java.timeFormatUTC(1700000000000, 'yyyy/MM/dd HH:mm:ss', 28800000)",
        )
        .await?,
        "2023/11/15 06:13:20",
    );
    checks.eq(
        "java.timeFormatUTC 新增字母 G E a k K w W F（harness 现跑）",
        &eval(
            &engine,
            &source,
            "java.timeFormatUTC(1700000000000, 'G EEE a k K w W F', 0)",
        )
        .await?,
        "公元 周二 下午 22 10 46 3 2",
    );
    checks.eq(
        "java.timeFormatUTC 数字字段 count 是最少位数（kk / KKK / WW / FF / yyyyy）",
        &eval(
            &engine,
            &source,
            "java.timeFormatUTC(1700000000000, 'kk KKK WW FF y yyyyy', 0)",
        )
        .await?,
        "22 010 03 02 2023 02023",
    );
    checks.eq(
        "java.timeFormatUTC 周字段跨年：2023-12-31 属 2024 年第 1 周（harness 现跑）",
        &eval(
            &engine,
            &source,
            "java.timeFormatUTC(1703980800000, 'Y w', 0)",
        )
        .await?,
        "2024 1",
    );
    checks.eq(
        "java.timeFormatUTC 的 S 是毫秒字段（count 最少位数），不是小数位数",
        &eval(
            &engine,
            &source,
            "java.timeFormatUTC(1700000000123, 'S SS SSS SSSSS', 0)",
        )
        .await?,
        "123 123 123 00123",
    );
    checks.eq(
        "java.timeFormatUTC 的名字跟随运行环境 locale（本机 zh_CN 现跑）",
        &eval(
            &engine,
            &source,
            "java.timeFormatUTC(1700000000000, 'MMM MMMM EEE EEEE zzzz', 0)",
        )
        .await?,
        "11月 十一月 周二 星期二 协调世界时",
    );
    let nan_message = eval(
        &engine,
        &source,
        "try { java.timeFormat(NaN); '不抛' } catch (e) { '抛了: ' + e }",
    )
    .await?;
    checks.truthy(
        "java.timeFormat(NaN) 抛错（不给 1970）",
        nan_message.contains("抛了") && nan_message.contains("NaN"),
        &nan_message,
    );
    let nan_utc_message = eval(
        &engine,
        &source,
        "try { java.timeFormatUTC(NaN, 'yyyy', 0); '不抛' } catch (e) { '抛了: ' + e }",
    )
    .await?;
    checks.truthy(
        "java.timeFormatUTC(NaN) 抛错",
        nan_utc_message.contains("抛了") && nan_utc_message.contains("NaN"),
        &nan_utc_message,
    );

    println!("\n== M3b：Packages 注册表 ==");
    checks.eq(
        "hutool AES-256-CBC 解密（语料同款写法）",
        &eval(
            &engine,
            &source,
            "(function () {\
               var key = java.base64DecodeToByteArray(java.digestBase64Str('zG2nSeEfSHfvTCHy5LCcqtBbQehKNLXn', 'sha256'));\
               var iv = java.base64DecodeToByteArray('AAAAAAAAAAAAAAAAAAAAAA==');\
               var aes = Packages.cn.hutool.crypto.symmetric.AES('CBC', 'PKCS5Padding', key, iv);\
               return aes.decryptStr('vvqbS+QcO5umagRNftaF4CO0NBu5S5KRFco1X23gFdk=');\
             })()",
        )
        .await?,
        "斗破苍穹 第一章",
    );
    checks.eq(
        "hutool AES-128-ECB 解密（只给 key）",
        &eval(
            &engine,
            &source,
            "(function () {\
               var aes = Packages.cn.hutool.crypto.symmetric.AES(java.hexDecodeToByteArray('000102030405060708090a0b0c0d0e0f'));\
               return aes.decryptStr('dWZBa9QGwVSu2JTzA57FnA==');\
             })()",
        )
        .await?,
        "hello 世界",
    );
    checks.eq(
        "hutool AES-CBC 加密（对照 openssl）再解密回到原文",
        &eval(
            &engine,
            &source,
            "(function () {\
               var key = java.hexDecodeToByteArray('000102030405060708090a0b0c0d0e0f');\
               var iv = java.hexDecodeToByteArray('0f0e0d0c0b0a09080706050403020100');\
               var aes = Packages.cn.hutool.crypto.symmetric.AES('CBC', 'PKCS5Padding', key, iv);\
               return aes.encryptStr('中文abc') + '|' + aes.decryptStr(aes.encryptStr('中文abc'));\
             })()",
        )
        .await?,
        "EYB0E1zjO3KArB3AcFw2Hg==|中文abc",
    );
    checks.eq(
        "hutool DESede-CBC 解密",
        &eval(
            &engine,
            &source,
            "(function () {\
               var key = java.hexDecodeToByteArray('0123456789abcdef0123456789abcdef0123456789abcdef');\
               var iv = java.hexDecodeToByteArray('0000000000000000');\
               var des = Packages.cn.hutool.crypto.symmetric.DESede('CBC', 'PKCS5Padding', key, iv);\
               return des.decryptStr('iAtfVsuwF8akVI0UDl1G8w==');\
             })()",
        )
        .await?,
        "abc中文",
    );
    let unregistered = eval(
        &engine,
        &source,
        "try { Packages.cn.foo.Bar } catch (e) { '' + e }",
    )
    .await?;
    checks.truthy(
        "没注册的类名报错清楚（点名 cn.foo）",
        unregistered.contains("没有注册") && unregistered.contains("cn.foo"),
        &format!("得到 {unregistered:?}"),
    );
    checks.eq(
        "RandomUtil.randomNumbers",
        &eval(
            &engine,
            &source,
            "(function () { var n = Packages.cn.hutool.core.util.RandomUtil.randomNumbers(15); return n.length + '|' + /^[0-9]+$/.test(n); })()",
        )
        .await?,
        "15|true",
    );
    checks.eq(
        "RandomUtil.randomString",
        &eval(
            &engine,
            &source,
            "(function () { var s = Packages.cn.hutool.core.util.RandomUtil.randomString(32); return s.length + '|' + /^[a-z0-9]+$/.test(s); })()",
        )
        .await?,
        "32|true",
    );
    checks.eq(
        "URLDecoder.decode 按 charset",
        &eval(
            &engine,
            &source,
            "Packages.java.net.URLDecoder.decode('%E6%96%97%E7%A0%B4+%E8%8B%8D', 'UTF-8')",
        )
        .await?,
        "斗破 苍",
    );

    println!("\n== M3b：org.jsoup.Jsoup 垫片 ==");
    let page = "<div id=\"list\"><p class=\"x\">一</p><p>二</p><b data-v=\"7\">三</b></div>";
    checks.eq(
        "parse + select + size/length/attr",
        &eval(
            &engine,
            &source,
            &format!(
                "(function () {{\
                   var doc = org.jsoup.Jsoup.parse('{page}');\
                   var ps = doc.select('p');\
                   return [ps.size(), ps.length, ps.attr('class'), doc.select('b').attr('data-v')].join('|');\
                 }})()"
            ),
        )
        .await?,
        "2|2|x|7",
    );
    checks.eq(
        "text / eachText / String(elements)",
        &eval(
            &engine,
            &source,
            &format!(
                "(function () {{\
                   var ps = org.jsoup.Jsoup.parse('{page}').select('p');\
                   return [ps.text(), ps.eachText().join('/'), String(ps)].join('|');\
                 }})()"
            ),
        )
        .await?,
        "一 二|一/二|<p class=\"x\">一</p><p>二</p>",
    );
    checks.eq(
        "Elements.html() 取元素内部",
        &eval(
            &engine,
            &source,
            &format!("org.jsoup.Jsoup.parse('{page}').select('div').html()"),
        )
        .await?,
        "<p class=\"x\">一</p>\n<p>二</p><b data-v=\"7\">三</b>",
    );
    checks.eq(
        "remove() 后父集合 size() 不变（同 Jsoup）",
        &eval(
            &engine,
            &source,
            "(function () {\
               var r = org.jsoup.Jsoup.parse('<div id=\"list\"><p>a</p><p class=\"non\">b<br></p><p>c</p></div>').select('#list>*');\
               r.select('p:has(br)').remove();\
               return '' + r.size();\
             })()",
        )
        .await?,
        "3",
    );
    checks.eq(
        "remove() 同步集合视图（有意差异，非 Jsoup 行为）",
        &eval(
            &engine,
            &source,
            "(function () {\
               var r = org.jsoup.Jsoup.parse('<div id=\"list\"><p>a</p><p class=\"non\">b<br></p><p>c</p></div>').select('#list>*');\
               r.select('p:has(br)').remove();\
               return [r.select('p.non').size(), String(r)].join('|');\
             })()",
        )
        .await?,
        "0|<p>a</p><p>c</p>",
    );
    checks.eq(
        "importClass 装成全局 + JavaImporter/with",
        &eval(
            &engine,
            &source,
            "(function () {\
               importClass(org.jsoup.Jsoup);\
               var viaGlobal = Jsoup.parse('<i>全局</i>').select('i').text();\
               var aly = new JavaImporter(Packages.cn.hutool.crypto.symmetric.AES, Packages.org.jsoup.Jsoup);\
               var viaWith;\
               with (aly) { viaWith = typeof AES + '|' + Jsoup.parse('<i>with</i>').select('i').text(); }\
               return viaGlobal + '|' + viaWith;\
             })()",
        )
        .await?,
        "全局|function|with",
    );
    checks.eq(
        "java.t2s / java.s2t",
        &eval(
            &engine,
            &source,
            "java.t2s('龍門客棧') + '|' + java.s2t('龙门客栈')",
        )
        .await?,
        "龙门客栈|龍門客棧",
    );

    println!("\n== M3b：HMAC / 摘要 / 签名（对照 openssl 与标准向量）==");
    checks.eq(
        "java.HMacHex（HmacSHA256）",
        &eval(
            &engine,
            &source,
            "java.HMacHex('data', 'HmacSHA256', 'key')",
        )
        .await?,
        "5031fe3d989c6d1537a013fa6e739da23463fdaec3b70137d828e36ace221bd0",
    );
    checks.eq(
        "java.HMacBase64（HmacSHA1）",
        &eval(
            &engine,
            &source,
            "java.HMacBase64('data', 'HmacSHA1', 'key')",
        )
        .await?,
        "EEFSxb/coHvGM+69RhmfAlXJ9J0=",
    );
    checks.eq(
        "java.HMacHex（HmacMD5）",
        &eval(&engine, &source, "java.HMacHex('data', 'HmacMD5', 'key')").await?,
        "9d5c73ef85594d34ec4438b7c97e51d8",
    );
    checks.eq(
        "java.digestHex（sha512）",
        &eval(&engine, &source, "java.digestHex('abc', 'sha512')").await?,
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
    );
    checks.eq(
        "java.digestHex（sha1）",
        &eval(&engine, &source, "java.digestHex('abc', 'sha1')").await?,
        "a9993e364706816aba3e25717850c26c9cd0d89d",
    );
    checks.eq(
        "java.md5Encode16",
        &eval(&engine, &source, "java.md5Encode16('abc')").await?,
        "3cd24fb0d6963f7d",
    );
    checks.eq(
        "DigestUtil.sha256Hex",
        &eval(
            &engine,
            &source,
            "Packages.cn.hutool.crypto.digest.DigestUtil.sha256Hex('abc')",
        )
        .await?,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
    checks.eq(
        "javax.crypto.Mac + SecretKeySpec + DatatypeConverter",
        &eval(
            &engine,
            &source,
            "(function () {\
               importClass(javax.crypto.Mac, javax.crypto.spec.SecretKeySpec, javax.xml.bind.DatatypeConverter);\
               var mac = Mac.getInstance('HmacSHA256');\
               mac.init(new SecretKeySpec(java.strToBytes('key'), 'HmacSHA256'));\
               return DatatypeConverter.printBase64Binary(mac.doFinal(java.strToBytes('data')));\
             })()",
        )
        .await?,
        "UDH+PZicbRU3oBP6bnOdojRj/a7DtwE32Cjjas4iG9A=",
    );
    checks.eq(
        "javax.crypto.Cipher 解密（AES/CBC/PKCS5Padding）",
        &eval(
            &engine,
            &source,
            "(function () {\
               importClass(javax.crypto.Cipher, javax.crypto.spec.SecretKeySpec, javax.crypto.spec.IvParameterSpec);\
               var key = java.base64DecodeToByteArray(java.digestBase64Str('zG2nSeEfSHfvTCHy5LCcqtBbQehKNLXn', 'sha256'));\
               var iv = java.base64DecodeToByteArray('AAAAAAAAAAAAAAAAAAAAAA==');\
               var cipher = Cipher.getInstance('AES/CBC/PKCS5Padding');\
               cipher.init(Cipher.DECRYPT_MODE, new SecretKeySpec(key, 'AES'), new IvParameterSpec(iv));\
               return java.bytesToStr(cipher.doFinal(java.base64DecodeToByteArray('vvqbS+QcO5umagRNftaF4CO0NBu5S5KRFco1X23gFdk=')));\
             })()",
        )
        .await?,
        "斗破苍穹 第一章",
    );
    checks.eq(
        "java.createSymmetricCrypto + decryptStr",
        &eval(
            &engine,
            &source,
            "(function () {\
               var key = java.base64DecodeToByteArray(java.digestBase64Str('zG2nSeEfSHfvTCHy5LCcqtBbQehKNLXn', 'sha256'));\
               var iv = java.base64DecodeToByteArray('AAAAAAAAAAAAAAAAAAAAAA==');\
               return java.createSymmetricCrypto('AES/CBC/PKCS5Padding', key, iv).decryptStr('vvqbS+QcO5umagRNftaF4CO0NBu5S5KRFco1X23gFdk=');\
             })()",
        )
        .await?,
        "斗破苍穹 第一章",
    );

    println!("\n== M3b：RSA（密钥与密文由 openssl 生成，见 fixtures/rsa_test.json）==");
    let rsa: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("fixtures/rsa_test.json")?)?;
    let private = rsa_field(&rsa, "privateKeyPkcs8Base64")?;
    let public = rsa_field(&rsa, "publicKeySpkiBase64")?;
    let plaintext = rsa_field(&rsa, "plaintext")?;
    let message = rsa_field(&rsa, "message")?;
    let pkcs1 = rsa_field(&rsa, "pkcs1CipherBase64")?;
    let oaep = rsa_field(&rsa, "oaepSha256CipherBase64")?;
    let signature = rsa_field(&rsa, "signatureBase64")?;

    checks.eq(
        "createAsymmetricCrypto 解 PKCS1 密文（openssl 生成）",
        &eval(
            &engine,
            &source,
            &format!(
                "java.createAsymmetricCrypto('RSA/ECB/PKCS1Padding')\
                   .setPrivateKey('{private}')\
                   .decryptStr('{pkcs1}')"
            ),
        )
        .await?,
        plaintext,
    );
    checks.eq(
        "createAsymmetricCrypto 解 OAEP-SHA256 密文",
        &eval(
            &engine,
            &source,
            &format!(
                "java.createAsymmetricCrypto('RSA/ECB/OAEPWithSHA-256AndMGF1Padding')\
                   .setPrivateKey('{private}')\
                   .decryptStr('{oaep}')"
            ),
        )
        .await?,
        plaintext,
    );
    checks.eq(
        "hutool RSA 类：加密再解密回原文",
        &eval(
            &engine,
            &source,
            &format!(
                "(function () {{\
                   var rsa = new Packages.cn.hutool.crypto.asymmetric.RSA('{private}', '{public}');\
                   return rsa.decryptStr(rsa.encryptBase64('中文-RSA'));\
                 }})()"
            ),
        )
        .await?,
        "中文-RSA",
    );
    checks.eq(
        "createSign 验签（openssl 生成的签名）",
        &eval(
            &engine,
            &source,
            &format!(
                "java.createSign('SHA256withRSA')\
                   .setPublicKey('{public}')\
                   .verify('{message}', '{signature}') + ''"
            ),
        )
        .await?,
        "true",
    );
    checks.eq(
        "createSign 签名再验签",
        &eval(
            &engine,
            &source,
            &format!(
                "(function () {{\
                   var signer = java.createSign('SHA256withRSA').setPrivateKey('{private}');\
                   var ok = signer.verify('{message}', signer.signHex('{message}'));\
                   var bad = signer.verify('{message} 改过了', signer.signHex('{message}'));\
                   return ok + '|' + bad;\
                 }})()"
            ),
        )
        .await?,
        "true|false",
    );

    println!("\n== 源级变量（`java.put` / `java.get` 的最后一档） ==");
    {
        let mut ctx = EvalContext::new(Content::html("<p>x</p>"))
            .with_source(&source)
            .with_js(engine.as_ref());
        let _ = rules
            .eval_string("@js:java.put('fromJs', '来自 JS')", &mut ctx)
            .await?;
        let mut ctx = EvalContext::new(Content::html("<p>x</p>"))
            .with_source(&source)
            .with_js(engine.as_ref());
        checks.eq(
            "java.put 落在源上（换一个求值上下文仍读得到）",
            &rules
                .eval_string("@js:java.get('fromJs')", &mut ctx)
                .await?,
            "来自 JS",
        );
        let mut ctx = EvalContext::new(Content::html("<p>x</p>")).with_source(&source);
        let _ = rules
            .eval_string("@put:{fromRule:来自规则}", &mut ctx)
            .await?;
        checks.eq(
            "规则级 @put 不落源（真机同）",
            &rules.eval_string("@get:{fromRule}", &mut ctx).await?,
            "",
        );
    }

    checks.guard(159);
    println!(
        "\n{} 项通过，{} 项失败",
        checks.passed,
        checks.failures.len()
    );
    if checks.failures.is_empty() {
        println!("M3a + M3b JS 运行时自检通过");
        Ok(true)
    } else {
        for failure in &checks.failures {
            eprintln!("失败: {failure}");
        }
        Ok(false)
    }
}

async fn eval(engine: &JsEngine, source: &BookSource, script: &str) -> Result<String> {
    let book = Book {
        name: "斗破苍穹".to_string(),
        ..Book::default()
    };
    engine
        .eval(JsRequest {
            script,
            source: Some(source),
            base_url: "https://example.com",
            page: Some(3),
            key: Some("斗破"),
            result: Some("正文"),
            src: None,
            response: None,
            book: Some(&book),
            chapter: None,
            variables: None,
        })
        .await
}

async fn eval_with_items(
    engine: &JsEngine,
    source: &BookSource,
    script: &str,
    items: Option<&[String]>,
) -> Result<JsValue> {
    let book = Book {
        name: "斗破苍穹".to_string(),
        ..Book::default()
    };
    engine
        .eval_value(
            JsRequest {
                script,
                source: Some(source),
                base_url: "https://example.com",
                page: Some(3),
                key: Some("斗破"),
                result: Some("正文"),
                src: None,
                response: None,
                book: Some(&book),
                chapter: None,
                variables: None,
            },
            items,
        )
        .await
}

fn js_value_text(value: &JsValue) -> String {
    match value {
        JsValue::Text(text) => text.clone(),
        JsValue::List(items) => legado_rust_engine::core::java_list_to_string(items),
        JsValue::Null => String::new(),
        JsValue::Other(text) => text.clone(),
    }
}

fn md5_of(text: &str) -> String {
    use md5::{Digest as _, Md5};
    let mut hasher = Md5::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn rsa_field<'a>(fixture: &'a serde_json::Value, name: &str) -> Result<&'a str> {
    fixture[name].as_str().ok_or_else(|| {
        Error::Other(format!(
            "fixtures/rsa_test.json 缺字段 {name}（改测试要重新生成）"
        ))
    })
}

#[derive(Default)]
struct StubCookies {
    jar: Mutex<std::collections::HashMap<String, String>>,
}

impl CookieStore for StubCookies {
    fn get_cookie(&self, url: &str) -> String {
        self.jar
            .lock()
            .expect("cookie 锁被毒化")
            .get(url)
            .cloned()
            .unwrap_or_default()
    }

    fn set_cookie(&self, url: &str, cookie: &str) {
        self.jar
            .lock()
            .expect("cookie 锁被毒化")
            .insert(url.to_string(), cookie.to_string());
    }

    fn replace_cookie(&self, url: &str, cookie: &str) {
        self.set_cookie(url, cookie);
    }

    fn remove_cookie(&self, url: &str) {
        self.jar.lock().expect("cookie 锁被毒化").remove(url);
    }
}

#[derive(Default)]
struct StubHttp {
    seen: Mutex<Vec<HttpRequest>>,
}

impl HttpFetcher for StubHttp {
    fn fetch(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse>> {
        Box::pin(async move {
            let body = request
                .body
                .as_ref()
                .map(|body| String::from_utf8_lossy(body).into_owned())
                .unwrap_or_default();
            let text = format!("stub:{}:{}:{}", request.method, request.url, body);
            let url = request.url.clone();
            self.seen.lock().expect("记录锁被毒化").push(request);
            Ok(HttpResponse {
                url,
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: Bytes::from(text.clone()),
                text,
            })
        })
    }
}

#[derive(Default)]
struct ConcurrentHttp {
    state: Mutex<ConcurrentState>,
}

#[derive(Default)]
struct ConcurrentState {
    rate: Option<String>,
    rate_key: String,
    in_flight: usize,
    max_in_flight: usize,
}

impl ConcurrentHttp {
    fn view<T>(&self, read: impl FnOnce(&ConcurrentState) -> T) -> T {
        read(&self.state.lock().expect("记录锁被毒化"))
    }
}

impl HttpFetcher for ConcurrentHttp {
    fn fetch(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse>> {
        Box::pin(async move {
            {
                let mut state = self.state.lock().expect("记录锁被毒化");
                state.in_flight += 1;
                state.max_in_flight = state.max_in_flight.max(state.in_flight);
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            self.state.lock().expect("记录锁被毒化").in_flight -= 1;
            let url = request.url.clone();
            let text = format!("concurrent:{url}");
            Ok(HttpResponse {
                url,
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: Bytes::from(text.clone()),
                text,
            })
        })
    }

    fn fetch_limited(
        &self,
        request: HttpRequest,
        rate: Option<String>,
        rate_key: String,
    ) -> BoxFuture<'_, Result<HttpResponse>> {
        {
            let mut state = self.state.lock().expect("记录锁被毒化");
            state.rate = rate;
            state.rate_key = rate_key;
        }
        self.fetch(request)
    }
}

struct FailingHttp;

impl HttpFetcher for FailingHttp {
    fn fetch(&self, _request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse>> {
        Box::pin(async move { Err(Error::Other("stub: 网络断了".into())) })
    }
}
