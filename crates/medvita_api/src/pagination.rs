//! The two list shapes of the server: numbered pages (core) and cursors (flows and the med apps).

use serde::{Deserialize, Serialize};

/// `{count, page, page_size, results}`: default 25, at most 100 per page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub count: u64,
    pub page: u64,
    pub page_size: u64,
    pub results: Vec<T>,
}

impl<T> Page<T> {
    /// `true` when another page exists after this one.
    pub fn has_next(&self) -> bool {
        self.page.saturating_mul(self.page_size) < self.count
    }
}

/// `{results, next_cursor}`; `page_size` comes back on some endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorPage<T> {
    pub results: Vec<T>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub page_size: Option<u64>,
}

/// `{results}` without pagination (e.g. `organizations/`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Results<T> {
    pub results: Vec<T>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_knows_whether_another_page_exists() {
        let page: Page<u8> =
            serde_json::from_str(r#"{"count": 30, "page": 1, "page_size": 25, "results": [1, 2]}"#)
                .unwrap();
        assert!(page.has_next());
        let last = Page { page: 2, ..page };
        assert!(!last.has_next());
    }

    #[test]
    fn cursor_page_tolerates_missing_cursor() {
        let page: CursorPage<u8> = serde_json::from_str(r#"{"results": []}"#).unwrap();
        assert!(page.next_cursor.is_none());
    }
}
