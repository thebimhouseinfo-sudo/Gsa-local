use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    Agent(Option<String>),
    Model(Option<String>),
    Config,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputLine {
    Command(SlashCommand),
    UserText(String),
    Empty,
}

pub fn parse_line(input: &str) -> Result<InputLine> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(InputLine::Empty);
    }
    if !trimmed.starts_with('/') {
        return Ok(InputLine::UserText(trimmed.to_owned()));
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or_default();
    let arg = parts.next().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);

    match command {
        "/agent" => Ok(InputLine::Command(SlashCommand::Agent(arg))),
        "/model" => Ok(InputLine::Command(SlashCommand::Model(arg))),
        "/config" if arg.is_none() => Ok(InputLine::Command(SlashCommand::Config)),
        "/config" => bail!("/config does not accept inline arguments"),
        other => bail!("unknown command {other}; available commands: /agent, /model, /config"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_three_slash_commands_are_recognized() {
        assert!(matches!(parse_line("/agent").unwrap(), InputLine::Command(SlashCommand::Agent(None))));
        assert!(matches!(parse_line("/model foo").unwrap(), InputLine::Command(SlashCommand::Model(Some(_)))));
        assert!(matches!(parse_line("/config").unwrap(), InputLine::Command(SlashCommand::Config)));
        assert!(parse_line("/status").is_err());
        assert!(parse_line("/cr").is_err());
    }
}
