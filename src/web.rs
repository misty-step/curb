use crate::api::{HeaderMap, Request, Response};

#[derive(Clone, Copy)]
struct Asset {
    path: &'static str,
    content_type: &'static str,
    bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

pub fn handle(request: &Request) -> Option<Response> {
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return None;
    }
    let asset = asset_for(&request.path)?;
    let mut headers = HeaderMap::default();
    headers.insert("content-type", asset.content_type);
    Some(Response {
        status: 200,
        headers,
        body: if request.method == "HEAD" {
            Vec::new()
        } else {
            asset.bytes.to_vec()
        },
    })
}

fn asset_for(path: &str) -> Option<Asset> {
    let normalized = normalize_path(path);
    ASSETS
        .iter()
        .find(|asset| asset.path == normalized)
        .copied()
        .or_else(index_asset)
}

fn normalize_path(path: &str) -> &str {
    if path.is_empty() || path == "/" {
        "/index.html"
    } else {
        path
    }
}

fn index_asset() -> Option<Asset> {
    ASSETS
        .iter()
        .find(|asset| asset.path == "/index.html")
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_index_and_spa_fallback_without_filesystem_access() {
        let index = handle(&Request::new("GET", "/")).unwrap();
        assert_eq!(index.status, 200);
        assert_eq!(
            index.headers.get("content-type"),
            Some("text/html; charset=utf-8")
        );
        assert!(index.text().contains("<div id=\"root\"></div>"));

        let fallback = handle(&Request::new("GET", "/deep/link")).unwrap();
        assert_eq!(fallback.body, index.body);

        let traversal = handle(&Request::new("GET", "/../Cargo.toml")).unwrap();
        assert_eq!(traversal.body, index.body);
    }

    #[test]
    fn serves_vite_assets_with_content_types_and_head_body_rules() {
        let script = ASSETS
            .iter()
            .find(|asset| asset.path.ends_with(".js"))
            .expect("embedded js asset");
        let style = ASSETS
            .iter()
            .find(|asset| asset.path.ends_with(".css"))
            .expect("embedded css asset");

        let js = handle(&Request::new("GET", script.path)).unwrap();
        assert_eq!(
            js.headers.get("content-type"),
            Some("text/javascript; charset=utf-8")
        );
        assert_eq!(js.body, script.bytes);

        let css = handle(&Request::new("GET", style.path)).unwrap();
        assert_eq!(
            css.headers.get("content-type"),
            Some("text/css; charset=utf-8")
        );
        assert_eq!(css.body, style.bytes);

        let head = handle(&Request::new("HEAD", "/")).unwrap();
        assert!(head.body.is_empty());
        assert_eq!(
            head.headers.get("content-type"),
            Some("text/html; charset=utf-8")
        );
    }

    #[test]
    fn rejects_non_get_head_methods() {
        assert!(handle(&Request::new("POST", "/")).is_none());
    }
}
