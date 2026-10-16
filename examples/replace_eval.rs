use std::process::ExitCode;

use legado_rust_engine::core::{BoxFuture, JsEvaluator, JsRequest, ReplaceRule, Result};
use legado_rust_engine::replace::{ReplaceTarget, apply_to_content, apply_to_title, select};

#[derive(Default)]
struct Checks {
    passed: usize,
    failures: Vec<String>,
}

impl Checks {
    fn eq(&mut self, label: &str, actual: impl std::fmt::Display, expected: &str) {
        let actual = actual.to_string();
        if actual == expected {
            self.passed += 1;
            println!("  ok   {label}");
        } else {
            self.failures
                .push(format!("{label}: 得到 {actual:?}，期望 {expected:?}"));
            println!("  FAIL {label}: 得到 {actual:?}，期望 {expected:?}");
        }
    }

    fn truthy(&mut self, label: &str, condition: bool, detail: impl std::fmt::Display) {
        if condition {
            self.passed += 1;
            println!("  ok   {label}");
        } else {
            self.failures.push(format!("{label}: {detail}"));
            println!("  FAIL {label}: {detail}");
        }
    }

    fn guard(&mut self, expected: usize) {
        let ran = self.passed + self.failures.len();
        if ran == expected {
            println!("  ok   断言执行数 {expected}");
        } else {
            let message = format!("断言执行数 {ran}，预期 {expected}（有分支被跳过？）");
            self.failures.push(message.clone());
            println!("  FAIL {message}");
        }
    }
}

struct StubJs;

impl JsEvaluator for StubJs {
    fn eval<'a>(&'a self, request: JsRequest<'a>) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            std::thread::sleep(std::time::Duration::from_millis(2));
            Ok(format!(
                "<{}|{}>",
                request.script.trim(),
                request.result.unwrap_or("")
            ))
        })
    }
}

fn rule(value: serde_json::Value) -> ReplaceRule {
    serde_json::from_value(value).expect("替换规则 JSON")
}

fn names(rules: &[&ReplaceRule]) -> String {
    rules
        .iter()
        .map(|rule| rule.name.clone())
        .collect::<Vec<_>>()
        .join(",")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut checks = Checks::default();
    filter_checks(&mut checks);
    replace_checks(&mut checks).await;
    checks.guard(16);

    println!();
    println!("{} 项通过，{} 项失败", checks.passed, checks.failures.len());
    if checks.failures.is_empty() {
        println!("M5 替换净化离线自检通过");
        ExitCode::SUCCESS
    } else {
        for failure in &checks.failures {
            eprintln!("失败: {failure}");
        }
        ExitCode::FAILURE
    }
}

fn filter_checks(checks: &mut Checks) {
    println!("== 规则筛选（scope / 作用范围 / order）");
    let rules = vec![
        rule(serde_json::json!({"name":"c1","pattern":"广告","replacement":"","order":2})),
        rule(
            serde_json::json!({"name":"c2","pattern":"foo","replacement":"bar","order":1,"scope":"某书"}),
        ),
        rule(
            serde_json::json!({"name":"c3","pattern":"x","replacement":"y","order":3,"excludeScope":"某书"}),
        ),
        rule(
            serde_json::json!({"name":"t1","pattern":"([0-9]+)","replacement":"[$1]","scopeTitle":true,"scopeContent":false,"order":0}),
        ),
        rule(serde_json::json!({"name":"off","pattern":"z","replacement":"","isEnabled":false})),
    ];
    checks.eq(
        "正文（某书）：排序 + excludeScope + isEnabled",
        names(&select(&rules, ReplaceTarget::Content, "某书", "https://o")),
        "c2,c1",
    );
    checks.eq(
        "正文（别的书）",
        names(&select(
            &rules,
            ReplaceTarget::Content,
            "别的书",
            "https://o",
        )),
        "c1,c3",
    );
    checks.eq(
        "标题只取 scopeTitle",
        names(&select(&rules, ReplaceTarget::Title, "某书", "https://o")),
        "t1",
    );
    let origin_rule = vec![rule(serde_json::json!({
        "name":"o1","pattern":"a","replacement":"b","scope":"https://o"
    }))];
    checks.eq(
        "scope 命中源地址",
        names(&select(
            &origin_rule,
            ReplaceTarget::Content,
            "无关",
            "https://o",
        )),
        "o1",
    );
}

async fn replace_checks(checks: &mut Checks) {
    println!();
    println!("== 替换行为");
    let literal = vec![rule(
        serde_json::json!({"name":"字面","pattern":"广告","replacement":"","isRegex":false}),
    )];
    let out = apply_to_content(&literal, "书", "o", "看广告啊广告", None).await;
    checks.eq("字面替换", &out.text, "看啊");

    let group = vec![rule(
        serde_json::json!({"name":"正则","pattern":"第([0-9]+)章","replacement":"[$1]"}),
    )];
    let out = apply_to_content(&group, "书", "o", "第12章", None).await;
    checks.eq("正则捕获组", &out.text, "[12]");

    let ordered = vec![
        rule(serde_json::json!({"name":"后","pattern":"b","replacement":"c","order":2})),
        rule(serde_json::json!({"name":"先","pattern":"a","replacement":"b","order":1})),
    ];
    let out = apply_to_content(&ordered, "书", "o", "a", None).await;
    checks.eq("按 order 先后应用", &out.text, "c");
    checks.eq("改动过的规则记录", out.applied.join(","), "先,后");

    println!();
    println!("== @js: 替换值");
    let js_rule = vec![rule(serde_json::json!({
        "name":"脚本","pattern":"b","replacement":"@js:return 'X'"
    }))];
    let out = apply_to_content(&js_rule, "书", "o", "abc", Some(&StubJs)).await;
    checks.eq("每命中一次跑一次 @js", &out.text, "a<return 'X'|b>c");
    let no_js = apply_to_content(&js_rule, "书", "o", "abc", None).await;
    checks.truthy(
        "没有 JsEvaluator 时报错",
        no_js.failed.len() == 1 && no_js.text == "abc",
        "应记一条失败并保留原文",
    );

    println!();
    println!("== 标题空值保留");
    let wipe = vec![rule(serde_json::json!({
        "name":"清空","pattern":".*","replacement":"","scopeTitle":true
    }))];
    let out = apply_to_title(&wipe, "书", "o", "标题", None).await;
    checks.eq("替换结果为空时保留原标题", &out.text, "标题");

    println!();
    println!("== 出错兜底");
    let bad = vec![rule(
        serde_json::json!({"name":"坏正则","pattern":"(","replacement":"x"}),
    )];
    let long = "a".repeat(40);
    let out = apply_to_content(&bad, "书", "o", &long, None).await;
    checks.eq("坏正则后原文不变", &out.text, &long);
    checks.eq("坏正则记一条失败", out.failed.len(), "1");

    let slow = vec![rule(serde_json::json!({
        "name":"脚本超时","pattern":"a","replacement":"@js:return 'x'","timeoutMillisecond":1
    }))];
    let out = apply_to_content(&slow, "书", "o", &long, Some(&StubJs)).await;
    checks.eq("超时后原文不变", &out.text, &long);
    checks.truthy(
        "@js 超时记失败",
        out.failed
            .iter()
            .any(|failure| failure.name == "脚本超时" && failure.error.contains("超时")),
        format!("{:?}", out.failed),
    );

    let bomb_rule = vec![rule(serde_json::json!({
        "name":"灾难正则","pattern":"(a+)+$","replacement":"x","timeoutMillisecond":1
    }))];
    let bomb = format!("{}b", "a".repeat(40));
    let out = apply_to_content(&bomb_rule, "书", "o", &bomb, None).await;
    checks.truthy(
        "timeoutMillisecond 墙钟超时后原文不变",
        out.failed.len() == 1 && out.text == bomb,
        format!("{:?} -> {:?}", out.failed, out.text),
    );
}
