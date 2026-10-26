//! Library core for the `rust-cli` binary. Everything testable lives here.

pub mod commands;
pub mod parser;

pub use parser::{parse_args, Command, ParseError};

/// Execute a parsed command and return the text that should be printed.
pub fn run(command: &Command) -> Result<String, String> {
    match command.name.as_str() {
        "greet" => {
            let name = command.args.first().map(String::as_str).unwrap_or("World");
            Ok(commands::greet(name))
        }
        "shout" => Ok(commands::shout(&command.args.join(" "))),
        other => Err(format!("Unknown command: {other}")),
    }
}

/// Convenience wrapper used by `main`: parse then run.
pub fn run_args<I>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = String>,
{
    let command = parse_args(args).map_err(|e| e.to_string())?;
    run(&command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_command_is_an_error() {
        let cmd = Command { name: "dance".into(), args: vec![] };
        assert_eq!(run(&cmd), Err("Unknown command: dance".to_string()));
    }

    #[test]
    fn greet_defaults_to_world() {
        let cmd = Command { name: "greet".into(), args: vec![] };
        assert_eq!(run(&cmd).unwrap(), "Hello, World!");
    }
}
