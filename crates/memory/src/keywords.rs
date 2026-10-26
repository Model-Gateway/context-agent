//! Keyword extraction shared by task-boundary detection and memory search.

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "of", "to", "in", "on", "for", "with", "that", "this", "these", "those", "it", "its",
    "is", "are", "was", "were", "be", "been", "being", "have", "has", "had", "do", "does", "did", "can", "could", "should",
    "would", "will", "shall", "may", "might", "must", "please", "make", "add", "update", "change", "fix", "use", "set", "get",
    "into", "from", "as", "at", "by", "so", "if", "then", "than", "also", "too", "just", "now", "very", "more", "less", "some",
    "all", "any", "not", "no", "yes", "you", "we", "i", "me", "my", "our", "your", "their", "them", "they", "he", "she", "him",
    "her", "there", "here", "when", "where", "which", "what", "who", "how", "why", "again", "still", "new", "one", "way",
];

/// Content words and identifiers from free text, lowercased and deduplicated.
/// Quoted strings and file-like tokens are kept whole as well as split.
pub fn extract_keywords(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |s: String| {
        if s.len() >= 2 && !STOPWORDS.contains(&s.as_str()) && seen.insert(s.clone()) {
            out.push(s);
        }
    };
    // Quoted strings as whole phrases.
    let mut chars = text.chars().peekable();
    let mut buf = String::new();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => {
                let phrase = buf.trim().to_lowercase();
                if !phrase.is_empty() {
                    push(phrase);
                }
                buf.clear();
                quote = None;
            }
            Some(_) => buf.push(c),
            None if c == '"' || c == '\'' || c == '`' => {
                // Apostrophes inside words are not quotes.
                let prev_alnum = !buf.is_empty();
                if c == '\'' && prev_alnum && chars.peek().map(|n| n.is_alphanumeric()).unwrap_or(false) {
                    buf.push(c);
                } else {
                    buf.clear();
                    quote = Some(c);
                }
            }
            None => buf.push(c),
        }
    }
    for raw in text.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '`' | '?' | '!')) {
        let token = raw.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.' || c == '/' || c == '-'));
        if token.is_empty() {
            continue;
        }
        let lower = token.to_lowercase();
        let is_pathlike = token.contains('/') || (token.contains('.') && !token.ends_with('.'));
        if is_pathlike {
            push(lower.trim_end_matches('.').to_string());
            if let Some(base) = lower.rsplit('/').next() {
                push(base.trim_end_matches('.').to_string());
                if let Some(stem) = base.split('.').next() {
                    push(stem.to_string());
                }
            }
        }
        let cleaned: String = lower.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
        if cleaned.is_empty() || cleaned.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        push(cleaned.clone());
        // camelCase / snake_case parts.
        let mut part = String::new();
        let chars: Vec<char> = token.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            if !c.is_alphanumeric() {
                if !part.is_empty() {
                    push(part.to_lowercase());
                    part.clear();
                }
                continue;
            }
            if !part.is_empty() && c.is_uppercase() && chars[i - 1].is_lowercase() {
                push(part.to_lowercase());
                part.clear();
            }
            part.push(c);
        }
        if !part.is_empty() && part.to_lowercase() != cleaned {
            push(part.to_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_identifiers_paths_and_quotes() {
        let k = extract_keywords("Change the primary button in src/components/Button.tsx to say \"Sign in\" and fix handleLoginClick");
        assert!(k.contains(&"primary".into()));
        assert!(k.contains(&"button".into()));
        assert!(k.contains(&"src/components/button.tsx".into()));
        assert!(k.contains(&"button.tsx".into()));
        assert!(k.contains(&"sign in".into()));
        assert!(k.contains(&"handleloginclick".into()));
        assert!(k.contains(&"login".into()));
        assert!(!k.contains(&"the".into()));
    }
}
