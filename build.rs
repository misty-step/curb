use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest_dir.join("internal/web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut assets = Vec::new();
    collect_assets(&dist, &dist, &mut assets);
    assets.sort_by(|left, right| left.0.cmp(&right.0));

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let path = out_dir.join("web_assets.rs");
    let mut file = fs::File::create(path).unwrap();
    writeln!(file, "const ASSETS: &[Asset] = &[").unwrap();
    for (route, path) in assets {
        let mime = content_type(&route);
        writeln!(
            file,
            "    Asset {{ path: {:?}, content_type: {:?}, bytes: include_bytes!({:?}) }},",
            route,
            mime,
            path.display().to_string()
        )
        .unwrap();
    }
    writeln!(file, "];").unwrap();
}

fn collect_assets(root: &Path, dir: &Path, assets: &mut Vec<(String, PathBuf)>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_assets(root, &path, assets);
            continue;
        }
        let relative = path.strip_prefix(root).unwrap();
        let route = format!(
            "/{}",
            relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        );
        assets.push((route, path));
    }
}

fn content_type(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}
