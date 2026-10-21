use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DECLS: &[&str] = &[
    "fn uregex_open(pattern: *const u16, length: i32, flags: u32, pe: *mut c_void, status: *mut UErrorCode) -> *mut URegularExpression;",
    "fn uregex_close(regexp: *mut URegularExpression);",
    "fn uregex_clone(regexp: *const URegularExpression, status: *mut UErrorCode) -> *mut URegularExpression;",
    "fn uregex_setText(regexp: *mut URegularExpression, text: *const u16, len: i32, status: *mut UErrorCode);",
    "fn uregex_findNext(regexp: *mut URegularExpression, status: *mut UErrorCode) -> i8;",
    "fn uregex_start(regexp: *mut URegularExpression, group: i32, status: *mut UErrorCode) -> i32;",
    "fn uregex_end(regexp: *mut URegularExpression, group: i32, status: *mut UErrorCode) -> i32;",
    "fn uregex_group(regexp: *mut URegularExpression, group: i32, dest: *mut u16, capacity: i32, status: *mut UErrorCode) -> i32;",
    "fn uregex_groupCount(regexp: *mut URegularExpression, status: *mut UErrorCode) -> i32;",
    "fn uregex_setTimeLimit(regexp: *mut URegularExpression, millis: i32, status: *mut UErrorCode);",
    "fn u_errorName(code: UErrorCode) -> *const c_char;",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let (libs, dirs) = pkg_config();
    for lib in &libs {
        println!("cargo:rustc-link-lib={lib}");
    }
    for dir in &dirs {
        println!("cargo:rustc-link-search=native={dir}");
    }
    write_bindings(&symbol_suffix(&dirs));
}

fn pkg_config() -> (Vec<String>, Vec<String>) {
    let mut libs = Vec::new();
    let mut dirs = Vec::new();
    if let Ok(out) = Command::new("pkg-config")
        .args(["--libs", "icu-i18n"])
        .output()
        && out.status.success()
    {
        for token in String::from_utf8_lossy(&out.stdout).split_whitespace() {
            if let Some(name) = token.strip_prefix("-l") {
                libs.push(name.to_string());
            } else if let Some(path) = token.strip_prefix("-L") {
                dirs.push(path.to_string());
            }
        }
    }
    if let Ok(out) = Command::new("pkg-config")
        .args(["--variable=libdir", "icu-i18n"])
        .output()
        && out.status.success()
    {
        let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !dir.is_empty() {
            dirs.push(dir);
        }
    }
    if libs.is_empty() {
        libs = vec!["icui18n".to_string(), "icuuc".to_string()];
    }
    (libs, dirs)
}

fn symbol_suffix(dirs: &[String]) -> String {
    let mut search: Vec<PathBuf> = dirs.iter().map(PathBuf::from).collect();
    search.extend(
        [
            "/usr/lib",
            "/usr/lib64",
            "/usr/local/lib",
            "/lib",
            "/opt/homebrew/lib",
        ]
        .iter()
        .map(PathBuf::from),
    );
    for dir in search {
        for file in icu_libs(&dir) {
            let Ok(bytes) = fs::read(&file) else {
                continue;
            };
            if let Some(suffix) = scan(&bytes) {
                return suffix;
            }
        }
    }
    String::new()
}

fn icu_libs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut shared = Vec::new();
    let mut archive = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("libicui18n.so") || name.starts_with("libicui18n.dylib") {
            shared.push(entry.path());
        } else if name.starts_with("libicui18n.a") {
            archive.push(entry.path());
        }
    }
    shared.extend(archive);
    shared
}

fn scan(bytes: &[u8]) -> Option<String> {
    let needle = b"uregex_open";
    let mut from = 0usize;
    while let Some(at) = find_from(bytes, needle, from) {
        from = at + 1;
        let rest = &bytes[at + needle.len()..];
        match rest.first().copied() {
            Some(0) => return Some(String::new()),
            Some(b'_') => {
                let digits: String = rest[1..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .map(|b| *b as char)
                    .collect();
                if !digits.is_empty() && rest.get(digits.len() + 1) == Some(&0) {
                    return Some(format!("_{digits}"));
                }
            }
            _ => {}
        }
    }
    None
}

fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if haystack.len() < needle.len() + from {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|pos| pos + from)
}

fn write_bindings(suffix: &str) {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR missing"));
    let mut src = String::from("unsafe extern \"C\" {\n");
    for decl in DECLS {
        if !suffix.is_empty() {
            let name = decl_name(decl);
            let _ = writeln!(src, "    #[link_name = \"{name}{suffix}\"]");
        }
        let _ = writeln!(src, "    {decl}");
    }
    src.push_str("}\n");
    fs::write(out_dir.join("icu.rs"), src).expect("write icu.rs");
}

fn decl_name(decl: &str) -> &str {
    let start = decl.find("fn ").map(|at| at + 3).expect("fn missing");
    let end = decl[start..]
        .find('(')
        .map(|at| at + start)
        .expect("( missing");
    decl[start..end].trim()
}
