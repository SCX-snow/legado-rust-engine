# fixtures

真实书源 / 订阅源样本、A/B 对拍语料与对照向量：`examples/roundtrip_sources.rs` 验「legado 导出的 JSON ⇄ 模型」
双向互转，`examples/legado_ab.rs` 拿这里的语料与**真实 legado 引擎**的输出逐条比对，`examples/js_eval.rs`
用 `rsa_test.json` 做 RSA 对照。

| 文件 | 条数 | 来源 |
| --- | --- | --- |
| `book_sources.json` | 110 | 下列公开合集去重合并（按 `bookSourceUrl`，保留首次出现），2026-09-10 抓取 |
| `rss_sources.json` | 4 | 参考源码 `legado/app/src/main/assets/defaultData/rssSources.json` |
| `rsa_test.json` | 1 组 | `openssl` 现生成的一次性测试密钥与向量（2026-09-14），给 `examples/js_eval.rs` 的 RSA 断言做对照 |
| `replace_rules.json` | 20 | [阅读书源合集页](https://yuedu.miaogongzi.net/jh.html) 的「净化规则」合集 `http://yuedu.miaogongzi.net/shuyuan/1125jh.json`，2026-10-08 抓取 |
| `legado_ab/` | 1156 | 与真实 legado 的规则求值 A/B 语料（1149 = 1040 条合成规则 + 109 条真实抓取页面；+7 条手工判别 case） |
| `legado_ab_real/` | 112 | **真实书源 + 真实页面**的小切片（112 个源各 1 条，`scripts/ab/make_real_corpus.py` 从全量重测里切；刻意偏向「我们与 legado 不一致」的 case，页面 ≤40KB，剔掉时间/随机与 `Jsoup.connect`） |
| `replace_ab/` | 48 | 净化规则 A/B 语料（真实规则 × 标题/正文/筛选），legado 侧用 `scripts/ab/regen_replace.sh` 的 harness 入口跑出 |

`legado_ab/` 三份文件是一套：`cases.jsonl`（请求行）、`legado.jsonl`（**真 legado 引擎**的逐条结果，
2026-10-12 起是 **ICU = 真机语义**）、`rust.jsonl`（建立基线时我们的结果）+ `cases.md5`（**输入摘要**：
门禁每次比对，改过 `cases.jsonl` 就必须重录 oracle 并重新基线）。那 7 条手工判别 case 源自
`scripts/ab/curated_cases.jsonl`（补 D10/D11/D15/D16/D21/D22），由 `scripts/ab/merge_curated.py` 并入。

`legado_ab_real/` 同样是 `cases.jsonl` + `legado.jsonl` + `rust.jsonl` + `cases.md5` 一套，oracle 也是 ICU 真机录制；
它专门管**真实页面形态**（编码 / CDATA / 统计像素 / JSON API / 长文本）。

`replace_rules.json` 是 **20 条真实的净化规则**（19 条启用、4 条替换值用 `@js:`），字段就是 legado
导出的 `ReplaceRule` 形态；`replace_ab/` 里的 `legado.jsonl` 记录的同样是**真实 legado 引擎**的输出。
它们由 `examples/legado_ab.rs`（`--corpus fixtures/replace_ab`）在离线验收里比对。

`rsa_test.json` 里是一把 **2048 位测试 RSA 密钥**（不是任何真实凭证），以及用它生成的：
PKCS#1 v1.5 与 OAEP-SHA256 密文、SHA256withRSA 签名。改 RSA 相关测试时用
`openssl genpkey` / `pkeyutl` / `dgst` 重新生成即可（`js_eval` 只读不写这个文件）。

`book_sources.json` 的构成：

| 来源 | 条数 |
| --- | --- |
| [XIU2/Yuedu](https://github.com/XIU2/Yuedu) `shuyuan` | 22 |
| [rektpartyaftermath/Legado-booksource-collection](https://github.com/rektpartyaftermath/Legado-booksource-collection) `AllBooksource.json` | 18 |
| [ZWolken/Light-Novel-Yuedu-Source](https://github.com/ZWolken/Light-Novel-Yuedu-Source) `old_ver/bookSource_22-03-15.json` | 27 |
| 同上 `old_ver/China_based_bookSource.json` | 2 |
| 参考源码 `defaultData/bookSources.json` | 1 |
| [e101406/booksources](https://github.com/e101406/booksources) `3.0/total.json` 里挑出的旧格式 / 类型漂移样本 | 40 |

覆盖到的边界情况（对应 `crates/legado-core/src/serde_ext.rs`）：

- 规则对象是「装着 JSON 的字符串」：4 条（旧版导出形态）
- `ruleExplore` / `ruleReview` 等是数组：11 条（legado 会当 null）
- `bookSourceType` 是字符串（`"0"` / `"TEXT"` / 空串）：22 条
- 2.x 老格式的扁平 `rule*` 字段（`enable` / `ruleSearchList` / ...）：23 条，模型忽略
- `customButton` / `eventListener` 等未建模的新字段：忽略
- 带登录 72 条、带发现 75 条、带段评规则 7 条

数据版权归各书源作者，这里只作回归语料。
