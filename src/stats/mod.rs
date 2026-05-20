mod accumulators;
mod cli;
mod events;
mod formatters;
mod reports;
mod scan;

pub use crate::time::StatGroupBy;
pub use cli::{
    read_usage_records_report, resolve_stat_range_options_from_raw, run_stat_command,
    ResolvedStatRangeOptions, StatCommandOptions,
};
pub use reports::{
    SkippedEvents, TokenUsage, UsageDiagnostics, UsageRecord, UsageRecordsReadOptions,
    UsageRecordsReport,
};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StatFormat {
    Table,
    Json,
    Csv,
    Markdown,
}

impl StatFormat {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "table" => Ok(Self::Table),
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            "markdown" => Ok(Self::Markdown),
            _ => Err(AppError::invalid_input(
                "Invalid format value. Expected one of: table, json, csv, markdown.",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StatSort {
    Time,
    Tokens,
    Credits,
    Calls,
    Sessions,
}

impl StatSort {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "time" => Ok(Self::Time),
            "tokens" => Ok(Self::Tokens),
            "credits" => Ok(Self::Credits),
            "calls" => Ok(Self::Calls),
            "sessions" => Ok(Self::Sessions),
            _ => Err(AppError::invalid_input(
                "Invalid sort value. Expected one of: time, tokens, credits, calls, sessions.",
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Time => "time",
            Self::Tokens => "tokens",
            Self::Credits => "credits",
            Self::Calls => "calls",
            Self::Sessions => "sessions",
        }
    }
}
