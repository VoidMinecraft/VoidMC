//! Prometheus text exposition format, version 0.0.4.
//!
//! <https://prometheus.io/docs/instrumenting/exposition_formats/#text-based-format>

use std::fmt::Write;

/// The `Content-Type` of a text exposition.
pub(crate) const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetricKind {
    Counter,
    Gauge,
    Histogram,
}

impl MetricKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::Gauge => "gauge",
            Self::Histogram => "histogram",
        }
    }
}

/// Writes metric families one after the other: [`family`](Self::family)
/// declares one, the [`sample`](Self::sample)s that follow belong to it.
#[derive(Default)]
pub(crate) struct TextEncoder {
    out: String,
}

impl TextEncoder {
    pub(crate) fn family(&mut self, name: &str, kind: MetricKind, help: &str) {
        debug_assert!(is_metric_name(name), "invalid metric name {name:?}");
        debug_assert!(
            kind != MetricKind::Counter || name.ends_with("_total"),
            "counter {name:?} must end in _total"
        );
        let _ = writeln!(self.out, "# HELP {name} {}", escape_help(help));
        let _ = writeln!(self.out, "# TYPE {name} {}", kind.as_str());
    }

    pub(crate) fn sample(&mut self, name: &str, labels: &[(&str, &str)], value: f64) {
        debug_assert!(is_metric_name(name), "invalid metric name {name:?}");
        self.out.push_str(name);
        if !labels.is_empty() {
            self.out.push('{');
            for (index, (label, value)) in labels.iter().enumerate() {
                debug_assert!(is_label_name(label), "invalid label name {label:?}");
                if index > 0 {
                    self.out.push(',');
                }
                let _ = write!(self.out, "{label}=\"{}\"", escape_label_value(value));
            }
            self.out.push('}');
        }
        let _ = writeln!(self.out, " {}", format_value(value));
    }

    pub(crate) fn finish(self) -> String {
        self.out
    }
}

/// Formats a sample value the way Prometheus parses it: `+Inf`, `-Inf`, `NaN`
/// or a plain decimal number.
pub(crate) fn format_value(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value == f64::INFINITY {
        "+Inf".to_string()
    } else if value == f64::NEG_INFINITY {
        "-Inf".to_string()
    } else {
        value.to_string()
    }
}

fn escape_help(help: &str) -> String {
    help.replace('\\', "\\\\").replace('\n', "\\n")
}

fn escape_label_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn is_metric_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == ':')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

fn is_label_name(name: &str) -> bool {
    let mut chars = name.chars();
    !name.starts_with("__")
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_help_and_type_before_samples() {
        let mut encoder = TextEncoder::default();
        encoder.family("voidmc_ticks_total", MetricKind::Counter, "Ticks run.");
        encoder.sample("voidmc_ticks_total", &[], 42.0);
        encoder.family("voidmc_errors_total", MetricKind::Counter, "Errors.");
        encoder.sample("voidmc_errors_total", &[("kind", "io")], 0.0);
        encoder.sample("voidmc_errors_total", &[("kind", "frame"), ("a", "b")], 3.0);

        assert_eq!(
            encoder.finish(),
            "# HELP voidmc_ticks_total Ticks run.\n\
             # TYPE voidmc_ticks_total counter\n\
             voidmc_ticks_total 42\n\
             # HELP voidmc_errors_total Errors.\n\
             # TYPE voidmc_errors_total counter\n\
             voidmc_errors_total{kind=\"io\"} 0\n\
             voidmc_errors_total{kind=\"frame\",a=\"b\"} 3\n"
        );
    }

    #[test]
    fn formats_special_and_fractional_values() {
        assert_eq!(format_value(f64::INFINITY), "+Inf");
        assert_eq!(format_value(f64::NEG_INFINITY), "-Inf");
        assert_eq!(format_value(f64::NAN), "NaN");
        assert_eq!(format_value(0.0025), "0.0025");
        assert_eq!(format_value(1.0), "1");
        assert_eq!(format_value(1e-9), "0.000000001");
        assert_eq!(format_value(19.75), "19.75");
    }

    #[test]
    fn escapes_help_text_and_label_values() {
        let mut encoder = TextEncoder::default();
        encoder.family("m", MetricKind::Gauge, "a\\b\nc \"q\"");
        encoder.sample("m", &[("l", "x\\y\"z\n")], 1.0);
        assert_eq!(
            encoder.finish(),
            "# HELP m a\\\\b\\nc \"q\"\n# TYPE m gauge\nm{l=\"x\\\\y\\\"z\\n\"} 1\n"
        );
    }

    #[test]
    fn validates_metric_and_label_names() {
        assert!(is_metric_name("voidmc_ticks_total"));
        assert!(is_metric_name("ns:sub_metric"));
        assert!(!is_metric_name("1ticks"));
        assert!(!is_metric_name("ticks-total"));
        assert!(!is_metric_name(""));
        assert!(is_label_name("kind"));
        assert!(is_label_name("_kind"));
        assert!(!is_label_name("__name__"));
        assert!(!is_label_name("a:b"));
    }
}
