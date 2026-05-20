use crate::error::AppError;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, MultiSelect, Select};
use std::io::{self, ErrorKind, IsTerminal};

pub trait Prompt {
    fn select(&mut self, prompt: &str, items: &[String]) -> Result<Option<usize>, AppError>;

    fn multi_select(
        &mut self,
        prompt: &str,
        items: &[String],
    ) -> Result<Option<Vec<usize>>, AppError>;

    fn confirm(&mut self, prompt: &str, default: bool) -> Result<Option<bool>, AppError>;
}

#[derive(Default)]
pub struct DialoguerPrompt {
    theme: ColorfulTheme,
}

impl Prompt for DialoguerPrompt {
    fn select(&mut self, prompt: &str, items: &[String]) -> Result<Option<usize>, AppError> {
        Select::with_theme(&self.theme)
            .with_prompt(prompt)
            .items(items)
            .default(0)
            .interact_opt()
            .map_err(prompt_error)
    }

    fn multi_select(
        &mut self,
        prompt: &str,
        items: &[String],
    ) -> Result<Option<Vec<usize>>, AppError> {
        MultiSelect::with_theme(&self.theme)
            .with_prompt(prompt)
            .items(items)
            .interact_opt()
            .map_err(prompt_error)
    }

    fn confirm(&mut self, prompt: &str, default: bool) -> Result<Option<bool>, AppError> {
        Confirm::with_theme(&self.theme)
            .with_prompt(prompt)
            .default(default)
            .interact_opt()
            .map_err(prompt_error)
    }
}

pub fn stdin_and_stderr_are_terminals() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

fn prompt_error(error: dialoguer::Error) -> AppError {
    match error {
        dialoguer::Error::IO(error) if error.kind() == ErrorKind::Interrupted => {
            AppError::new("Prompt cancelled.")
        }
        error => AppError::new(format!("Prompt failed: {error}")),
    }
}
