//! Case-insensitive substring search with * and ? wildcards; terms are ANDed.
pub struct Filter(Vec<Vec<char>>);

impl Filter {
    pub fn new(query: &str) -> Self {
        Self(
            query
                .split_whitespace()
                .map(|term| format!("*{}*", term.to_lowercase()).chars().collect())
                .collect(),
        )
    }

    pub fn matches(&self, fields: &[&str]) -> bool {
        let fields: Vec<Vec<char>> = fields
            .iter()
            .map(|s| s.to_lowercase().chars().collect())
            .collect();
        self.0
            .iter()
            .all(|pattern| fields.iter().any(|field| wildcard(pattern, field)))
    }
}

fn wildcard(pattern: &[char], text: &[char]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star = None;
    let mut retry = 0;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            retry = t;
        } else if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if let Some(at) = star {
            retry += 1;
            t = retry;
            p = at + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patterns_match_names_routes_and_paths_without_regex_syntax() {
        let fields = [
            "http POST /orders/:id",
            "OrdersController.create",
            "src/orders.ts",
        ];
        for query in [
            "",
            "ORDERS",
            "*controller.cre*",
            "POST cre?te",
            "src/*.ts",
            "orders**create",
        ] {
            assert!(Filter::new(query).matches(&fields), "{query}");
        }
        assert!(!Filter::new("payments").matches(&fields));
        assert!(!Filter::new("GET create").matches(&fields));
        assert!(Filter::new("[id]").matches(&["/orders/[id]"]));
        assert!(!Filter::new("[id]").matches(&["/orders/id"]));
        assert!(Filter::new("caf?").matches(&["CAFÉ"]));
        assert!(Filter::new("a*b?d").matches(&["aZZbcbXd"]));
        assert!(!Filter::new("a*b?d").matches(&["aZZbcbd"]));
    }
}
