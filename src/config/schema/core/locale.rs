use super::Config;

fn detect_system_locale() -> Option<String> {
    std::env::var("LANG")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .ok()
        .map(|lang| lang.trim().to_lowercase())
        .filter(|lang| !lang.is_empty())
}

/// Detect locale: `ASTERONIRIS_LANG` env -> config value -> system `LANG` -> `"en"`.
fn detect_locale(config_locale: &str) -> String {
    if let Ok(lang) = std::env::var("ASTERONIRIS_LANG") {
        let lang = lang.trim().to_lowercase();
        if !lang.is_empty() {
            return normalise_locale(&lang);
        }
    }

    if config_locale != "en" && !config_locale.is_empty() {
        return normalise_locale(config_locale);
    }

    if let Some(system_locale) = detect_system_locale() {
        return normalise_locale(&system_locale);
    }

    "en".into()
}

/// Normalise `"ja_JP.UTF-8"` -> `"ja"`, `"en_US"` -> `"en"`, passthrough `"ja"`.
fn normalise_locale(raw: &str) -> String {
    let base = raw.split('.').next().unwrap_or(raw);
    let lang = base.split('_').next().unwrap_or(base);
    lang.to_string()
}

impl Config {
    /// Detect locale from env -> config -> system, then set `rust_i18n::set_locale`.
    pub fn apply_locale(&self) {
        let locale = detect_locale(&self.locale);
        rust_i18n::set_locale(&locale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            // SAFETY: Test-only helper. All tests using EnvVarGuard acquire
            // ENV_LOCK first, serialising concurrent env-var access.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            // SAFETY: Test-only helper. ENV_LOCK serialises access;
            // the guard restores the original value on drop.
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.previous {
                // SAFETY: Test-only restoration. ENV_LOCK is still held by
                // the enclosing test, so no concurrent env mutation.
                unsafe {
                    std::env::set_var(self.key, value);
                }
            } else {
                // SAFETY: Test-only cleanup. Removes a variable introduced
                // by EnvVarGuard::set; ENV_LOCK serialises access.
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    #[test]
    fn detect_locale_uses_expected_priority_order() {
        let _lock = ENV_LOCK.lock().unwrap();

        let _lang = EnvVarGuard::set("LANG", "pt_BR.UTF-8");
        let _lc_messages = EnvVarGuard::set("LC_MESSAGES", "es_ES.UTF-8");

        let _asteroniris_lang = EnvVarGuard::set("ASTERONIRIS_LANG", "ja_JP.UTF-8");
        assert_eq!(detect_locale("fr_FR"), "ja");
        drop(_asteroniris_lang);

        assert_eq!(detect_locale("fr_FR"), "fr");
        assert_eq!(detect_locale("en"), "pt");

        let _lang_unset = EnvVarGuard::unset("LANG");
        assert_eq!(detect_locale("en"), "es");

        let _lc_messages_unset = EnvVarGuard::unset("LC_MESSAGES");
        assert_eq!(detect_locale("en"), "en");
    }

    #[test]
    fn normalise_locale_handles_common_formats() {
        assert_eq!(normalise_locale("ja_JP.UTF-8"), "ja");
        assert_eq!(normalise_locale("en_US"), "en");
        assert_eq!(normalise_locale("en"), "en");
        assert_eq!(normalise_locale(""), "");
    }
}
