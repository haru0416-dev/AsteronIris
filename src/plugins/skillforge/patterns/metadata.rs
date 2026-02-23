use super::ReasonCode;
use super::shared::{contains_word, levenshtein};

const BAD_NAME_PATTERNS: &[&str] = &[
    "malware",
    "exploit",
    "hack",
    "crack",
    "keygen",
    "ransomware",
    "trojan",
];

const KNOWN_PACKAGES: &[&str] = &[
    "tokio",
    "serde",
    "reqwest",
    "hyper",
    "axum",
    "actix",
    "rocket",
    "diesel",
    "sqlx",
    "clap",
    "tracing",
    "anyhow",
    "thiserror",
    "rand",
    "chrono",
    "uuid",
    "regex",
    "log",
    "env_logger",
    "react",
    "express",
    "lodash",
    "axios",
    "webpack",
    "babel",
    "numpy",
    "pandas",
    "flask",
    "django",
    "requests",
    "tensorflow",
];

const BINARY_EXTENSIONS: &[&str] = &[".so", ".dll", ".dylib", ".wasm", ".exe", ".bin", ".o", ".a"];

pub fn detect_metadata_reasons(
    name: &str,
    description: &str,
    has_license: bool,
    days_since_update: Option<i64>,
    file_names: &[String],
) -> Vec<ReasonCode> {
    let mut reasons = Vec::new();
    let lower_name = name.to_lowercase();
    let lower_desc = description.to_lowercase();

    for pattern in BAD_NAME_PATTERNS {
        if contains_word(&lower_name, pattern) || contains_word(&lower_desc, pattern) {
            reasons.push(ReasonCode::BadPatternName);
            break;
        }
    }

    for known in KNOWN_PACKAGES {
        let distance = levenshtein(&lower_name, known);
        if distance > 0 && distance <= 2 {
            reasons.push(ReasonCode::Typosquatting);
            break;
        }
    }

    for file in file_names {
        let lower = file.to_lowercase();
        if BINARY_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
            reasons.push(ReasonCode::BinaryArtifact);
            break;
        }
    }

    if !has_license {
        reasons.push(ReasonCode::NoLicense);
    }

    if let Some(days) = days_since_update
        && days >= 90
    {
        reasons.push(ReasonCode::Unmaintained);
    }

    reasons
}
