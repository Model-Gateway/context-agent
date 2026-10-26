//! Credential redaction.

use regex::Regex;
use std::sync::OnceLock;

/// Removes secrets from text before it is persisted or displayed.
#[derive(Debug, Clone)]
pub struct Redactor {
    /// Exact secret values known to the process (e.g. the configured API key).
    known_values: Vec<String>,
    enabled: bool,
}

pub const REDACTED: &str = "[REDACTED]";

fn patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            // OpenAI-style keys
            r"\bsk-[A-Za-z0-9_\-]{16,}\b",
            // Anthropic-style keys
            r"\bsk-ant-[A-Za-z0-9_\-]{16,}\b",
            // AWS access key ids
            r"\bAKIA[0-9A-Z]{16}\b",
            // GitHub tokens
            r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}\b",
            r"\bgithub_pat_[A-Za-z0-9_]{20,}\b",
            // Slack tokens
            r"\bxox[baprs]-[A-Za-z0-9\-]{10,}\b",
            // Google API keys
            r"\bAIza[0-9A-Za-z_\-]{30,}\b",
            // JWTs
            r"\beyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\b",
            // Bearer headers
            r"(?i)\bbearer\s+[A-Za-z0-9._~+/=\-]{16,}",
            // Private key blocks
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            // URL credentials: scheme://user:pass@host
            r"(?i)\b([a-z][a-z0-9+.\-]*://)[^\s/:@]+:[^\s/@]+@",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("valid redaction regex"))
        .collect()
    })
}

fn assignment_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)\b((?:api[_\-]?key|secret[_\-]?key|access[_\-]?token|auth[_\-]?token|refresh[_\-]?token|client[_\-]?secret|secret|token|password|passwd|pwd|private[_\-]?key)\s*[=:]\s*["']?)([A-Za-z0-9_\-./+=~!@#$%^&*]{8,})"#,
        )
        .expect("valid assignment regex")
    })
}

impl Redactor {
    pub fn new(enabled: bool) -> Self {
        Self { known_values: Vec::new(), enabled }
    }

    pub fn disabled() -> Self {
        Self::new(false)
    }

    /// Register an exact secret value (e.g. the API key read from the
    /// environment). Values shorter than 8 characters are ignored to avoid
    /// mangling ordinary text.
    pub fn add_known_value(&mut self, value: impl Into<String>) {
        let v = value.into();
        if v.len() >= 8 && !self.known_values.contains(&v) {
            self.known_values.push(v);
        }
    }

    /// Register every environment variable whose name suggests a secret.
    /// Short or dictionary-like values (`postgres`, `password`) are skipped:
    /// replacing them globally would corrupt ordinary code shown to the
    /// model, and they are not meaningful secrets.
    pub fn with_environment_secrets(mut self) -> Self {
        for (name, value) in std::env::vars() {
            let upper = name.to_ascii_uppercase();
            if upper.ends_with("_KEY")
                || upper.ends_with("_TOKEN")
                || upper.ends_with("_SECRET")
                || upper.ends_with("_PASSWORD")
                || upper.contains("API_KEY")
                || upper.contains("ACCESS_KEY")
            {
                if Self::looks_like_real_secret(&value) {
                    self.add_known_value(value);
                }
            }
        }
        self
    }

    fn looks_like_real_secret(value: &str) -> bool {
        let v = value.trim();
        if v.len() < 12 {
            return false;
        }
        let all_lower_alpha = v.chars().all(|c| c.is_ascii_lowercase());
        let all_upper_alpha = v.chars().all(|c| c.is_ascii_uppercase());
        !(all_lower_alpha || all_upper_alpha)
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Full redaction: known values, token-shaped patterns, and
    /// `key = value` assignments whose value looks like a secret. Use for
    /// logs, events, and diagnostics.
    pub fn redact(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut out = self.redact_credentials_only(text);
        if assignment_pattern().is_match(&out) {
            out = assignment_pattern()
                .replace_all(&out, |caps: &regex::Captures| {
                    let value = &caps[2];
                    if looks_like_secret(value) {
                        format!("{}{}", &caps[1], REDACTED)
                    } else {
                        caps[0].to_string()
                    }
                })
                .into_owned();
        }
        out
    }

    /// Conservative redaction: only exact known values and high-confidence
    /// token shapes. Use for tool output that the model must still be able
    /// to read as code.
    pub fn redact_credentials_only(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut out = text.to_string();
        for v in &self.known_values {
            if out.contains(v.as_str()) {
                out = out.replace(v.as_str(), REDACTED);
            }
        }
        for re in patterns() {
            if re.is_match(&out) {
                out = re
                    .replace_all(&out, |caps: &regex::Captures| {
                        if let Some(prefix) = caps.get(1) {
                            format!("{}{}@", prefix.as_str(), REDACTED)
                        } else {
                            REDACTED.to_string()
                        }
                    })
                    .into_owned();
            }
        }
        out
    }

    /// Redact a JSON value in place (strings only).
    pub fn redact_json(&self, value: &mut serde_json::Value) {
        if !self.enabled {
            return;
        }
        match value {
            serde_json::Value::String(s) => {
                let r = self.redact(s);
                if r != *s {
                    *s = r;
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| self.redact_json(v)),
            serde_json::Value::Object(map) => map.values_mut().for_each(|v| self.redact_json(v)),
            _ => {}
        }
    }
}

/// Heuristic: secrets tend to contain digits or be long; identifiers and
/// plain words assigned to `token`/`secret` variables usually do not.
fn looks_like_secret(value: &str) -> bool {
    let has_digit = value.chars().any(|c| c.is_ascii_digit());
    let has_mixed = value.chars().any(|c| c.is_ascii_uppercase()) && value.chars().any(|c| c.is_ascii_lowercase());
    (has_digit && value.len() >= 8) || (has_mixed && value.len() >= 16) || value.len() >= 24
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_patterns() {
        let r = Redactor::default();
        let text = "key sk-abcdefghijklmnopqrstuvwxyz0123 and AKIAABCDEFGHIJKLMNOP and ghp_abcdefghijklmnopqrstuvwxyz";
        let out = r.redact(text);
        assert!(!out.contains("sk-abc"));
        assert!(!out.contains("AKIA"));
        assert!(!out.contains("ghp_"));
        assert_eq!(out.matches(REDACTED).count(), 3);
    }

    #[test]
    fn redacts_assignments_and_urls() {
        let r = Redactor::default();
        let out = r.redact("API_KEY=supersecretvalue123\npassword: hunter2hunter2\nhttps://user:pa55word@example.com/x");
        assert!(out.contains("API_KEY=[REDACTED]"));
        assert!(out.contains("password: [REDACTED]"));
        assert!(out.contains("https://[REDACTED]@example.com/x"));
    }

    #[test]
    fn redacts_known_values() {
        let mut r = Redactor::default();
        r.add_known_value("my-literal-secret-value");
        assert_eq!(r.redact("token is my-literal-secret-value ok"), "token is [REDACTED] ok");
    }

    #[test]
    fn environment_scan_skips_dictionary_values() {
        assert!(!Redactor::looks_like_real_secret("postgres"));
        assert!(!Redactor::looks_like_real_secret("password"));
        assert!(!Redactor::looks_like_real_secret("developmentonly"));
        assert!(Redactor::looks_like_real_secret("sk-live-7fK2q9ZpL0"));
        assert!(Redactor::looks_like_real_secret("AKIA0123456789ABCDEF"));
    }

    #[test]
    fn leaves_ordinary_code_alone() {
        let r = Redactor::default();
        let code = "fn secret_sauce() { let token = parse(input); let password = user_password; }";
        assert_eq!(r.redact(code), code);
        assert_eq!(r.redact_credentials_only("password = hunter2hunter2"), "password = hunter2hunter2");
    }
}
