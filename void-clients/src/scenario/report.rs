use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct LineResult {
    pub(super) line: usize,
    text: String,
    pub(super) passed: bool,
    ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) got: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<Value>,
}

impl LineResult {
    pub(super) fn new(line: usize, text: String, ms: u128, step: Step) -> Self {
        Self {
            line,
            text,
            passed: step.error.is_none(),
            ms,
            got: step.got,
            error: step.error,
            note: step.note,
            detail: step.detail,
        }
    }

    pub(super) fn report(&self) -> String {
        let mut text = format!(
            "{} {:>3} {:>7.3}s {}",
            if self.passed { "ok  " } else { "FAIL" },
            self.line,
            self.ms as f64 / 1000.0,
            self.text
        );
        if let Some(got) = &self.got {
            text.push_str(&format!("  got {got}"));
        }
        if let Some(error) = &self.error {
            text.push_str(&format!("\n     {}", error.replace('\n', "\n     ")));
        }
        if let Some(note) = &self.note {
            text.push_str(&format!("\n     {note}"));
        }
        text
    }
}

#[derive(Debug, Default)]
pub(super) struct Step {
    pub(super) got: Option<Value>,
    pub(super) error: Option<String>,
    pub(super) note: Option<String>,
    pub(super) detail: Option<Value>,
}

impl Step {
    pub(super) fn failed(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::default()
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub(super) client: String,
    pub(super) level: &'static str,
    pub(super) total: usize,
    pub(super) played: usize,
    pub(super) failed: usize,
    pub(super) seconds: f64,
    pub(super) passed: bool,
    pub(super) lines: Vec<LineResult>,
}

impl Report {
    pub(super) fn text(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            text.push_str(&line.report());
            text.push('\n');
        }
        text.push_str(&self.verdict());
        text.push('\n');
        text
    }

    pub(super) fn verdict(&self) -> String {
        format!(
            "{}: {} of {} lines played, {} failed, {:.2} s",
            if self.passed { "PASSED" } else { "FAILED" },
            self.played,
            self.total,
            self.failed,
            self.seconds
        )
    }

    pub(super) fn write(&self, out: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(out.join("report.json"), json).map_err(|e| e.to_string())?;
        fs::write(out.join("report.txt"), self.text()).map_err(|e| e.to_string())
    }
}
