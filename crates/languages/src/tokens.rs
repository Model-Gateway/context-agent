//! Identifier normalization for lexical search (spec §7.4).

/// Split an identifier into lowercase word parts: `getUserName` →
/// `["get", "user", "name"]`, `HTTP_SERVER` → `["http", "server"]`,
/// `parseJSONValue` → `["parse", "json", "value"]`.
pub fn split_identifier(ident: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = ident.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '-' || c == '.' || c == ':' || c == '/' || c.is_whitespace() {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            continue;
        }
        if !current.is_empty() && c.is_uppercase() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).map(|n| n.is_lowercase()).unwrap_or(false);
            // Boundary when previous is lowercase/digit (camelCase) or when
            // an acronym run ends (JSONValue -> JSON | Value).
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                parts.push(std::mem::take(&mut current));
            }
        }
        current.push(c);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts.into_iter().map(|p| p.to_lowercase()).filter(|p| !p.is_empty()).collect()
}

/// Extract identifiers from source text and return a space-joined string of
/// normalized tokens (deduplicated, order preserved) suitable for an FTS
/// column. Includes both the whole lowercase identifier and its parts.
pub fn identifier_tokens(source: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for ident in source.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|s| s.len() > 1) {
        if ident.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let lower = ident.to_lowercase();
        let parts = split_identifier(ident);
        if parts.len() > 1 && seen.insert(lower.clone()) {
            out.push(lower);
        }
        for p in parts {
            if p.len() > 1 && seen.insert(p.clone()) {
                out.push(p);
            }
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_camel_snake_and_acronyms() {
        assert_eq!(split_identifier("getUserName"), vec!["get", "user", "name"]);
        assert_eq!(split_identifier("HTTP_SERVER_PORT"), vec!["http", "server", "port"]);
        assert_eq!(split_identifier("parseJSONValue"), vec!["parse", "json", "value"]);
        assert_eq!(split_identifier("LoginButton"), vec!["login", "button"]);
        assert_eq!(split_identifier("handle_login2FA"), vec!["handle", "login2", "fa"]);
    }

    #[test]
    fn identifier_tokens_dedupe() {
        let t = identifier_tokens("fn handleLogin(user: UserId) { handleLogin(user) }");
        assert!(t.contains("handlelogin"));
        assert!(t.contains("handle"));
        assert!(t.contains("login"));
        assert_eq!(t.matches("handle ").count(), 1);
    }
}
