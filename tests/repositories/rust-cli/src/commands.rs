//! One pure function per sub-command. Keep these free of I/O so they are
//! trivial to unit test.

/// Produce a friendly greeting.
pub fn greet(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return "Hello, World!".to_string();
    }
    format!("Hello, {}!", name)
}

/// Upper-case the text and add an exclamation mark.
pub fn shout(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return "!".to_string();
    }
    format!("{}!", text.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_uses_the_name() {
        assert_eq!(greet("Ada"), "Hello, Ada!");
    }

    #[test]
    fn greet_falls_back_to_world() {
        assert_eq!(greet("   "), "Hello, World!");
    }

    #[test]
    fn shout_upper_cases() {
        assert_eq!(shout("hi there"), "HI THERE!");
    }
}
