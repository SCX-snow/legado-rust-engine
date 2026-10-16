fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if let Ok(out) = std::process::Command::new("pkg-config")
        .args(["--libs", "icu-i18n"])
        .output()
        && out.status.success()
    {
        let libs = String::from_utf8_lossy(&out.stdout);
        for token in libs.split_whitespace() {
            if let Some(name) = token.strip_prefix("-l") {
                println!("cargo:rustc-link-lib={name}");
            } else if let Some(path) = token.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={path}");
            }
        }
        return;
    }
    println!("cargo:rustc-link-lib=icui18n");
    println!("cargo:rustc-link-lib=icuuc");
}
