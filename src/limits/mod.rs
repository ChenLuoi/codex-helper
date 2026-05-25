mod cli;
mod events;
mod formatters;
mod reports;
mod scan;

pub use cli::{run_limit_command, LimitCommand, LimitCommandOptions, LimitFormat};
pub use events::{parse_rate_limit_line, RateLimitLineContext};
pub use reports::{
    attach_usage_to_limit_current, attach_usage_to_limit_windows, build_limit_current_report,
    build_limit_resets_report, build_limit_samples_report, build_limit_trend_report,
    build_limit_windows_report, limit_current_usage_range, limit_windows_usage_range,
    LimitCurrentReport, LimitCurrentWindow, LimitReportDiagnostics, LimitReportOptions,
    LimitResetEvent, LimitResetsReport, LimitSamplesReport, LimitSourceEvidence, LimitTrendChange,
    LimitTrendReport, LimitWindow, LimitWindowSelector, LimitWindowsReport, RateLimitDiagnostics,
    RateLimitParseDiagnostics, RateLimitSample, RateLimitSamplesReadOptions,
    RateLimitSamplesReport, SourceSpan,
};
pub use scan::read_rate_limit_samples_report;
