use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use legado_rust_engine::core::{
    BookSource, BoxFuture, Bytes, CacheStore, HeaderMap, HttpFetcher, HttpRequest, HttpResponse,
    MemoryCacheStore, Result, StatusCode,
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
        if self.passed + self.failures.len() == expected {
            println!("  ✓ 断言执行数 {expected}");
        } else {
            self.failures.push(format!(
                "断言执行数 {}，预期 {expected}",
                self.passed + self.failures.len()
            ));
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
    let outline_a = vec![(0i16, 0i16), (10, 0), (10, 10)];
    let outline_b = vec![(0i16, 0i16), (0, 20)];
    let outline_c = vec![(5i16, 5i16), (15, 15)];
    let error_font = build_font(
        &[(0x41, 1), (0x42, 2), (0x43, 3), (0x44, 4)],
        &[
            vec![],
            outline_a.clone(),
            outline_b.clone(),
            outline_c.clone(),
            vec![],
        ],
    );
    let correct_font = build_font(
        &[(0x7532, 1), (0x4E59, 2)],
        &[vec![], outline_a.clone(), outline_b.clone()],
    );
    let error_b64 = base64(&error_font);
    let correct_b64 = base64(&correct_font);

    let rules = Arc::new(RuleEngine::new());
    let http = Arc::new(FontHttp {
        font: Mutex::new(error_font.clone()),
        count: Mutex::new(0),
    });
    let cache: Arc<dyn CacheStore> = Arc::new(MemoryCacheStore::new(16));
    let engine = Arc::new(
        JsEngine::builder()
            .rules(rules.clone())
            .http(http.clone())
            .cache(cache)
            .timeout(std::time::Duration::from_secs(10))
            .build()?,
    );
    rules.set_js(engine.clone());

    let source: BookSource = serde_json::from_str(r#"{"bookSourceUrl": "https://example.com"}"#)?;
    let mut checks = Checks {
        passed: 0,
        failures: Vec::new(),
    };

    println!("\n== queryBase64TTF + replaceFont：按轮廓反查还原 ==");
    let script = format!(
        r#"var e = java.queryBase64TTF("{error_b64}");
var c = java.queryBase64TTF("{correct_b64}");
java.replaceFont("ABCD" + String.fromCharCode(0x4E00), e, c)"#
    );
    checks.eq(
        "replaceFont 反查（'C' 的轮廓不在参考字体里、0x4E00 不在错字体里都原样留）",
        &eval(&rules, &engine, &source, &script).await?,
        "甲乙CD\u{4e00}",
    );
    let script = format!(
        r#"var e = java.queryBase64TTF("{error_b64}", false);
var c = java.queryBase64TTF("{correct_b64}", false);
java.replaceFont("ABCD", e, c, true)"#
    );
    checks.eq(
        "filter=true：错字体里没有轮廓的字符删掉（'D'）",
        &eval(&rules, &engine, &source, &script).await?,
        "甲乙C",
    );
    let script = format!(
        r#"var e = java.queryBase64TTF("{error_b64}");
var c = java.queryBase64TTF("{correct_b64}");
java.replaceFont("A B", e, c, true)"#
    );
    checks.eq(
        "空白字符不参与替换、也不删",
        &eval(&rules, &engine, &source, &script).await?,
        "甲 乙",
    );
    let script = format!(
        r#"var e = java.queryBase64TTF("{error_b64}");
var c = java.queryBase64TTF("{correct_b64}");
java.replaceFont("AB", e, null) + "|" + java.replaceFont("AB", null, c)"#
    );
    checks.eq(
        "任一个字体是 null 时原样返回",
        &eval(&rules, &engine, &source, &script).await?,
        "AB|AB",
    );

    println!("\n== 字体对象上的查询方法 ==");
    let script = format!(
        r#"var e = java.queryBase64TTF("{error_b64}");
var c = java.queryBase64TTF("{correct_b64}");
var same = e.getGlyfByUnicode(0x41) == c.getGlyfByUnicode(0x7532);
[e.getGlyfIdByUnicode(0x41), e.getGlyfByUnicode(0x44) === null,
 c.getUnicodeByGlyf(c.getGlyfByUnicode(0x4E59)), e.getUnicodeByGlyf("没有这个轮廓"),
 e.isBlankUnicode(0x20), e.isBlankUnicode(0x41), same].join("|")"#
    );
    checks.eq(
        "getGlyfIdByUnicode / getGlyfByUnicode / getUnicodeByGlyf / isBlankUnicode",
        &eval(&rules, &engine, &source, &script).await?,
        "1|true|20057|0|true|false|true",
    );

    println!("\n== queryTTF(url)：走 HttpFetcher + SHA-256 缓存 ==");
    let url = "https://example.com/read.ttf";
    let script = format!(
        r#"var f = java.queryTTF("{url}");
f.getGlyfIdByUnicode(0x41)"#
    );
    checks.eq(
        "第一次按 URL 下载并解析",
        &eval(&rules, &engine, &source, &script).await?,
        "1",
    );
    checks.eq("下载次数 1", &http.count().to_string(), "1");
    let script = format!(
        r#"var f = java.queryTTF("{url}");
f.getGlyfIdByUnicode(0x41)"#
    );
    checks.eq(
        "第二次命中缓存（同样的 SHA-256 key）",
        &eval(&rules, &engine, &source, &script).await?,
        "1",
    );
    checks.eq("下载次数仍是 1", &http.count().to_string(), "1");
    let script = format!(
        r#"var f = java.queryTTF("{url}", false);
f.getGlyfIdByUnicode(0x41)"#
    );
    checks.eq(
        "useCache=false 会重新下载",
        &eval(&rules, &engine, &source, &script).await?,
        "1",
    );
    checks.eq("下载次数变 2", &http.count().to_string(), "2");

    println!("\n== 坏数据：报错而不是崩 ==");
    let error = eval(
        &rules,
        &engine,
        &source,
        r#"java.queryTTF("这显然不是字体")"#,
    )
    .await
    .expect_err("坏 base64 应该报错");
    checks.truthy(
        "不是 base64 时报清楚",
        error.to_string().contains("base64"),
        &error.to_string(),
    );
    let error = eval(&rules, &engine, &source, r#"java.queryTTF("AAECAw==")"#)
        .await
        .expect_err("不是字体应该报错");
    checks.truthy(
        "字节数不够时报「缺表」",
        error.to_string().contains("head"),
        &error.to_string(),
    );

    checks.guard(13);
    println!(
        "\n{} 项通过，{} 项失败",
        checks.passed,
        checks.failures.len()
    );
    for failure in &checks.failures {
        println!("  失败: {failure}");
    }
    Ok(checks.failures.is_empty())
}

async fn eval(
    rules: &RuleEngine,
    engine: &Arc<JsEngine>,
    source: &BookSource,
    script: &str,
) -> Result<String> {
    let mut ctx = EvalContext::new(Content::text("")).with_source(source);
    ctx = ctx.with_js(engine.as_ref());
    rules.eval_string(&format!("@js:{script}"), &mut ctx).await
}

#[derive(Default)]
struct FontHttp {
    font: Mutex<Vec<u8>>,
    count: Mutex<usize>,
}

impl FontHttp {
    fn count(&self) -> usize {
        *self.count.lock().expect("计数锁被毒化")
    }
}

impl HttpFetcher for FontHttp {
    fn fetch(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse>> {
        Box::pin(async move {
            *self.count.lock().expect("计数锁被毒化") += 1;
            let body = self.font.lock().expect("字体锁被毒化").clone();
            Ok(HttpResponse {
                url: request.url.clone(),
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: Bytes::from(body.clone()),
                text: String::from_utf8_lossy(&body).into_owned(),
            })
        })
    }
}

fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn build_font(cmap: &[(u32, u16)], glyphs: &[Vec<(i16, i16)>]) -> Vec<u8> {
    let mut glyf = Vec::new();
    let mut offsets = Vec::new();
    for points in glyphs {
        offsets.push(glyf.len() as u32);
        if points.is_empty() {
            glyf.extend_from_slice(&0i16.to_be_bytes());
            glyf.extend_from_slice(&[0u8; 8]);
            continue;
        }
        let (mut x_min, mut y_min, mut x_max, mut y_max) = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
        let (mut x, mut y) = (0i16, 0i16);
        let mut deltas = Vec::new();
        for (dx, dy) in points {
            x = x.wrapping_add(*dx);
            y = y.wrapping_add(*dy);
            x_min = x_min.min(x);
            y_min = y_min.min(y);
            x_max = x_max.max(x);
            y_max = y_max.max(y);
            deltas.push((*dx, *dy));
        }
        glyf.extend_from_slice(&1i16.to_be_bytes());
        glyf.extend_from_slice(&x_min.to_be_bytes());
        glyf.extend_from_slice(&y_min.to_be_bytes());
        glyf.extend_from_slice(&x_max.to_be_bytes());
        glyf.extend_from_slice(&y_max.to_be_bytes());
        glyf.extend_from_slice(&((points.len() - 1) as u16).to_be_bytes());
        glyf.extend_from_slice(&0u16.to_be_bytes());
        glyf.extend(std::iter::repeat_n(0x01u8, points.len()));
        for (dx, _) in &deltas {
            glyf.extend_from_slice(&dx.to_be_bytes());
        }
        for (_, dy) in &deltas {
            glyf.extend_from_slice(&dy.to_be_bytes());
        }
        while glyf.len() % 4 != 0 {
            glyf.push(0);
        }
    }
    offsets.push(glyf.len() as u32);

    let mut loca = Vec::new();
    for offset in &offsets {
        assert!(
            offset % 2 == 0 && offset / 2 <= u16::MAX as u32,
            "测试字体用不了 short loca（偏移 {offset}）"
        );
        loca.extend_from_slice(&((offset / 2) as u16).to_be_bytes());
    }

    let mut segments: Vec<(u32, u16)> = cmap.to_vec();
    segments.sort();
    let mut ends = Vec::new();
    let mut starts = Vec::new();
    let mut deltas = Vec::new();
    for (code, glyph_id) in &segments {
        starts.push(*code as u16);
        ends.push(*code as u16);
        deltas.push((*glyph_id as i32 - *code as i32) as i16);
    }
    starts.push(0xFFFF);
    ends.push(0xFFFF);
    deltas.push(1);
    let seg_count = starts.len();
    let mut subtable = Vec::new();
    let length = 16 + seg_count * 8;
    subtable.extend_from_slice(&4u16.to_be_bytes());
    subtable.extend_from_slice(&(length as u16).to_be_bytes());
    subtable.extend_from_slice(&0u16.to_be_bytes());
    subtable.extend_from_slice(&((seg_count * 2) as u16).to_be_bytes());
    subtable.extend_from_slice(&0u16.to_be_bytes());
    subtable.extend_from_slice(&0u16.to_be_bytes());
    subtable.extend_from_slice(&0u16.to_be_bytes());
    for end in &ends {
        subtable.extend_from_slice(&end.to_be_bytes());
    }
    subtable.extend_from_slice(&0u16.to_be_bytes());
    for start in &starts {
        subtable.extend_from_slice(&start.to_be_bytes());
    }
    for delta in &deltas {
        subtable.extend_from_slice(&delta.to_be_bytes());
    }
    for _ in 0..seg_count {
        subtable.extend_from_slice(&0u16.to_be_bytes());
    }
    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&1u16.to_be_bytes());
    cmap.extend_from_slice(&3u16.to_be_bytes());
    cmap.extend_from_slice(&1u16.to_be_bytes());
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&subtable);

    let mut head = Vec::new();
    head.extend_from_slice(&1u16.to_be_bytes());
    head.extend_from_slice(&0u16.to_be_bytes());
    head.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    head.extend_from_slice(&0u32.to_be_bytes());
    head.extend_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head.extend_from_slice(&0u16.to_be_bytes());
    head.extend_from_slice(&1000u16.to_be_bytes());
    head.extend_from_slice(&[0u8; 16]);
    head.extend_from_slice(&[0u8; 8]);
    head.extend_from_slice(&0u16.to_be_bytes());
    head.extend_from_slice(&8u16.to_be_bytes());
    head.extend_from_slice(&2i16.to_be_bytes());
    head.extend_from_slice(&0i16.to_be_bytes());
    head.extend_from_slice(&0i16.to_be_bytes());

    let mut maxp = Vec::new();
    maxp.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp.extend_from_slice(&(glyphs.len() as u16).to_be_bytes());
    maxp.extend_from_slice(&0u16.to_be_bytes());
    maxp.extend_from_slice(&1u16.to_be_bytes());
    for _ in 0..11 {
        maxp.extend_from_slice(&0u16.to_be_bytes());
    }

    let tables: [(&[u8; 4], &Vec<u8>); 5] = [
        (b"cmap", &cmap),
        (b"glyf", &glyf),
        (b"head", &head),
        (b"loca", &loca),
        (b"maxp", &maxp),
    ];
    let mut out = Vec::new();
    let num_tables = tables.len() as u16;
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&num_tables.to_be_bytes());
    let entry_selector = (num_tables as f32).log2().floor() as u16;
    let search_range: u16 = 16 * (1 << entry_selector);
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&(num_tables * 16 - search_range).to_be_bytes());

    let mut offset = 12 + 16 * tables.len();
    for (tag, table) in &tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        offset += (table.len() + 3) & !3;
    }
    for (_, table) in &tables {
        out.extend_from_slice(table);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}
