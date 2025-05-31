use std::str::FromStr;

use teloxide::utils::command::ParseError;

#[derive(thiserror::Error, Debug)]
pub enum CommandError {
    #[error("parse error: {0:?}")]
    ParseError(#[from] ParseError),
    #[error("failed to validate command: {0:?}")]
    ValidationError(String),
}

#[derive(Clone)]
pub struct BotCommand {
    command: String,
    args: Option<String>,
}

impl FromStr for BotCommand {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (command, args) = s.split_once(" ").map_or((s, None), |s| (s.0, Some(s.1)));

        match command.strip_prefix("/") {
            Some(command) => Ok(Self {
                command: command.to_string(),
                args: args.map(str::to_string),
            }),
            None => Err(CommandError::ParseError(ParseError::IncorrectFormat(
                "Not a command".into(),
            ))),
        }
    }
}

impl BotCommand {
    pub fn from_validate(s: &str, cmds: &[&str]) -> Result<Self, CommandError> {
        let bc = Self::from_str(s)?;

        if !cmds.contains(&bc.command.as_str()) {
            return Err(CommandError::ValidationError(format!(
                "invalid command {}",
                bc.command
            )));
        };

        Ok(bc)
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn args(&self) -> Option<&str> {
        self.args.as_deref()
    }

    pub fn args_list(&self) -> Vec<&str> {
        let args = match self.args {
            Some(ref args) => args.as_str(),
            None => "",
        };

        args.split_whitespace().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_botcommand_from_str_simple() {
        let cmdstr = "/start";

        let bc = BotCommand::from_str(cmdstr).unwrap();
        assert_eq!(bc.command(), "start");
        assert_eq!(bc.args(), None);
    }

    #[test]
    fn test_botcommand_from_str_with_args() {
        let cmdstr = "/some_long_cmd arg1 arg2";

        let bc = BotCommand::from_str(cmdstr).unwrap();
        assert_eq!(bc.command(), "some_long_cmd");
        assert_eq!(bc.args(), Some("arg1 arg2"));
    }

    #[test]
    fn test_botcommand_arg_list() {
        let cmdstr = "/some_long_cmd arg1 arg2";

        let bc = BotCommand::from_str(cmdstr).unwrap();
        assert_eq!(bc.command(), "some_long_cmd");
        assert_eq!(bc.args(), Some("arg1 arg2"));
        assert_eq!(bc.args_list(), vec!["arg1", "arg2"]);
    }
}
