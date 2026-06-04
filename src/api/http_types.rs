use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub scheme: String,
    pub host: String,
}

impl Request {
    pub fn new(method: impl Into<String>, target: impl Into<String>) -> Self {
        let target = target.into();
        let (path, query) = super::routes::split_target(&target);
        Self {
            method: method.into().to_ascii_uppercase(),
            path,
            query,
            headers: HeaderMap::default(),
            body: Vec::new(),
            scheme: "http".to_string(),
            host: "127.0.0.1:8765".to_string(),
        }
    }

    #[cfg(test)]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name, value);
        self
    }

    #[cfg(test)]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    #[cfg(test)]
    pub fn origin(mut self, origin: impl Into<String>) -> Self {
        self.headers.insert("origin", origin);
        self
    }

    #[cfg(test)]
    pub fn cookie(mut self, cookie: impl Into<String>) -> Self {
        self.headers.insert("cookie", cookie);
        self
    }

    #[cfg(test)]
    pub fn endpoint(mut self, scheme: impl Into<String>, host: impl Into<String>) -> Self {
        self.scheme = scheme.into();
        self.host = host.into();
        self
    }

    pub(super) fn header_value(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    pub(super) fn cookie_value(&self, name: &str) -> Option<String> {
        self.header_value("cookie").and_then(|raw| {
            raw.split(';').find_map(|part| {
                let (key, value) = part.trim().split_once('=')?;
                (key == name).then(|| value.to_string())
            })
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Response {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    pub(super) fn empty(status: u16) -> Self {
        Self {
            status,
            headers: HeaderMap::default(),
            body: Vec::new(),
        }
    }

    pub(super) fn with_headers(mut self, mut headers: HeaderMap) -> Self {
        self.headers.append(&mut headers);
        self
    }

    #[cfg(test)]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    #[cfg(test)]
    pub(crate) fn empty_for_test(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: HeaderMap::default(),
            body,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeaderMap(BTreeMap<String, String>);

impl HeaderMap {
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0
            .insert(name.into().to_ascii_lowercase(), value.into());
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub(super) fn append(&mut self, other: &mut HeaderMap) {
        self.0.append(&mut other.0);
    }
}
