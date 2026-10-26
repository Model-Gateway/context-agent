//! Argument parsing. Deliberately dependency-free.

use std::fmt;

/// A parsed invocation: the sub-command name plus its positional arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// No sub-command was given at all.
    MissingCommand,
    /// A flag-looking argument (`--foo`) was given; flags are not supported.
    UnexpectedFlag(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::MissingCommand => write!(f, "usage: rust-cli <command> [args...]"),
            ParseError::UnexpectedFlag(flag) => write!(f, "unexpected flag: {flag}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse an argument iterator. The first item is treated as the program name
/// and skipped, mirroring `std::env::args()`.
pub fn parse_args<I>(args: I) -> Result<Command, ParseError>
where
    I: IntoIterator<Item = String>,
{
    let mut iter = args.into_iter().skip(1);
    let name = iter.next().ok_or(ParseError::MissingCommand)?;
    if name.starts_with('-') {
        return Err(ParseError::UnexpectedFlag(name));
    }
    let args: Vec<String> = iter.collect();
    if let Some(flag) = args.iter().find(|a| a.starts_with("--")) {
        return Err(ParseError::UnexpectedFlag(flag.clone()));
    }
    Ok(Command { name, args })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_command_and_args() {
        let cmd = parse_args(argv(&["rust-cli", "greet", "Ada"])).unwrap();
        assert_eq!(cmd.name, "greet");
        assert_eq!(cmd.args, vec!["Ada".to_string()]);
    }

    #[test]
    fn missing_command_is_an_error() {
        assert_eq!(parse_args(argv(&["rust-cli"])), Err(ParseError::MissingCommand));
    }

    #[test]
    fn flags_are_rejected() {
        let err = parse_args(argv(&["rust-cli", "greet", "--loud"])).unwrap_err();
        assert_eq!(err, ParseError::UnexpectedFlag("--loud".into()));
    }
}
