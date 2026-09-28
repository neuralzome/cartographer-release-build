//! Findings and how they are printed, as in msd: the same `✓ ! ✗ -`.

use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pass,
    /// Legal, and probably not what was intended. Never changes the exit code.
    Warn,
    Fail,
    /// Not checkable yet — reported so a green run is never mistaken for a
    /// complete one.
    Skip,
}

impl Status {
    fn marker(self) -> &'static str {
        match self {
            Status::Pass => "✓",
            Status::Warn => "!",
            Status::Fail => "✗",
            Status::Skip => "-",
        }
    }
}

pub struct Finding {
    pub status: Status,
    pub summary: String,
    /// Why it matters. Printed indented under anything that is not a pass.
    pub why: String,
}

impl Finding {
    pub fn pass(summary: impl Into<String>) -> Self {
        Finding { status: Status::Pass, summary: summary.into(), why: String::new() }
    }

    pub fn warn(summary: impl Into<String>, why: impl Into<String>) -> Self {
        Finding { status: Status::Warn, summary: summary.into(), why: why.into() }
    }

    pub fn fail(summary: impl Into<String>, why: impl Into<String>) -> Self {
        Finding { status: Status::Fail, summary: summary.into(), why: why.into() }
    }

    pub fn skip(summary: impl Into<String>, why: impl Into<String>) -> Self {
        Finding { status: Status::Skip, summary: summary.into(), why: why.into() }
    }
}

#[derive(Default)]
pub struct Report {
    findings: Vec<Finding>,
}

impl Report {
    pub fn new() -> Self {
        Report::default()
    }

    pub fn push(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    pub fn count(&self, status: Status) -> usize {
        self.findings.iter().filter(|finding| finding.status == status).count()
    }

    pub fn failed(&self) -> bool {
        self.count(Status::Fail) > 0
    }

    pub fn render(&self, title: &str) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "── {title} {}", "─".repeat(60usize.saturating_sub(title.len())));
        for finding in &self.findings {
            let _ = writeln!(out, "{} {}", finding.status.marker(), finding.summary);
            if finding.status != Status::Pass && !finding.why.is_empty() {
                for line in finding.why.lines() {
                    let _ = writeln!(out, "    {line}");
                }
            }
        }
        let _ = writeln!(
            out,
            "\n{} passed, {} warned, {} skipped, {} FAILED",
            self.count(Status::Pass),
            self.count(Status::Warn),
            self.count(Status::Skip),
            self.count(Status::Fail),
        );
        out
    }

    pub fn print(&self, title: &str) {
        print!("{}", self.render(title));
    }
}
