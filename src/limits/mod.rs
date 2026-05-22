mod cli;
mod events;
mod formatters;
mod reports;
mod scan;

pub use cli::{run_limit_command, LimitCommand, LimitCommandOptions, LimitFormat};
pub use events::{parse_rate_limit_line, RateLimitLineContext};
pub use reports::{
    build_limit_current_report, build_limit_resets_report, build_limit_samples_report,
    build_limit_trend_report, build_limit_windows_report, LimitCurrentReport, LimitCurrentWindow,
    LimitReportDiagnostics, LimitReportOptions, LimitResetEvent, LimitResetsReport,
    LimitSamplesReport, LimitSourceEvidence, LimitTrendChange, LimitTrendReport, LimitWindow,
    LimitWindowSelector, LimitWindowsReport, RateLimitDiagnostics, RateLimitParseDiagnostics,
    RateLimitSample, RateLimitSamplesReadOptions, RateLimitSamplesReport, SourceSpan,
};
pub use scan::read_rate_limit_samples_report;
