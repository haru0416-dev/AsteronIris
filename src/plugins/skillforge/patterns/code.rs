use super::ReasonCode;
use super::shared::has_high_entropy_strings;
#[cfg(test)]
use super::shared::{ENTROPY_THRESHOLD, shannon_entropy};

#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct CodeSignals {
    pub env_access: bool,
    pub network_access: bool,
    pub subprocess: bool,
    pub filesystem_access: bool,
    pub unsafe_block: bool,
    pub deserialization: bool,
    pub decode_exec: bool,
    pub build_script_network: bool,
    pub native_loading: bool,
    pub high_entropy: bool,
    pub obfuscation: bool,
}

const ENV_PATTERNS: &[&str] = &[
    "std::env::var",
    "std::env::vars",
    "env::var(",
    "env::vars(",
    "env!(",
    "option_env!(",
    "std::env::set_var",
    "dotenv",
    "dotenvy",
    "process.env",
    "os.environ",
    "getenv(",
    "ENV[",
];

const NETWORK_PATTERNS: &[&str] = &[
    "reqwest::",
    "hyper::",
    "TcpStream",
    "UdpSocket",
    "tokio::net::",
    "surf::",
    "ureq::",
    "attohttpc::",
    "curl::",
    "HttpClient",
    "fetch(",
    "urllib",
    "requests.get",
    "requests.post",
    "http.client",
    "net/http",
    "socket.connect",
];

const SUBPROCESS_PATTERNS: &[&str] = &[
    "std::process::Command",
    "Command::new",
    "process::Command",
    "tokio::process",
    "subprocess",
    "system(",
    "popen(",
    "child_process",
];

const FILESYSTEM_PATTERNS: &[&str] = &[
    "std::fs::",
    "fs::read",
    "fs::write",
    "fs::create_dir",
    "fs::remove",
    "fs::copy",
    "fs::rename",
    "File::open",
    "File::create",
    "OpenOptions",
    "tokio::fs::",
];

const DESERIALIZATION_PATTERNS: &[&str] = &[
    "serde_json::from_",
    "serde_yaml::from_",
    "bincode::deserialize",
    "ciborium::de",
    "postcard::from_bytes",
    "rmp_serde::from_",
    "pickle.load",
    "marshal.load",
    "yaml.unsafe_load",
];

const DECODE_EXEC_PATTERNS: &[&str] = &[
    "base64::decode",
    "base64::engine",
    "hex::decode",
    "from_base64",
    "atob(",
    "Buffer.from(",
];

const NATIVE_LOADING_PATTERNS: &[&str] = &[
    "libloading::",
    "dlopen",
    "LoadLibrary",
    "ctypes.cdll",
    "ctypes.windll",
    "ffi::dlopen",
];

const OBFUSCATION_PATTERNS: &[&str] = &[
    "char::from(",
    "from_utf8_unchecked",
    "String::from_raw_parts",
    "transmute",
];

pub fn detect_code_signals(code: &str, is_build_script: bool) -> CodeSignals {
    let contains_any = |patterns: &[&str]| patterns.iter().any(|pattern| code.contains(pattern));

    let env_access = contains_any(ENV_PATTERNS);
    let network_access = contains_any(NETWORK_PATTERNS);
    let subprocess = contains_any(SUBPROCESS_PATTERNS);
    let filesystem_access = contains_any(FILESYSTEM_PATTERNS);
    let unsafe_block = code.contains("unsafe {") || code.contains("unsafe fn ");
    let deserialization = contains_any(DESERIALIZATION_PATTERNS);
    let has_decode = contains_any(DECODE_EXEC_PATTERNS);
    let has_exec = subprocess || code.contains("eval(") || code.contains("exec(");
    let decode_exec = has_decode && has_exec;
    let build_script_network = is_build_script && network_access;
    let native_loading = contains_any(NATIVE_LOADING_PATTERNS);
    let high_entropy = has_high_entropy_strings(code);
    let obfuscation = contains_any(OBFUSCATION_PATTERNS);

    CodeSignals {
        env_access,
        network_access,
        subprocess,
        filesystem_access,
        unsafe_block,
        deserialization,
        decode_exec,
        build_script_network,
        native_loading,
        high_entropy,
        obfuscation,
    }
}

pub fn code_reasons(signals: &CodeSignals) -> Vec<ReasonCode> {
    let mut reasons = Vec::new();

    if signals.env_access && signals.network_access {
        reasons.push(ReasonCode::CredentialHarvest);
    }
    if signals.decode_exec {
        reasons.push(ReasonCode::EncodedPayload);
    }
    if signals.build_script_network {
        reasons.push(ReasonCode::BuildScriptAbuse);
    }
    if signals.native_loading {
        reasons.push(ReasonCode::NativeLoading);
    }
    if signals.obfuscation && signals.subprocess {
        reasons.push(ReasonCode::ObfuscationExec);
    }

    if signals.subprocess && !reasons.iter().any(ReasonCode::is_reject) {
        reasons.push(ReasonCode::Subprocess);
    }
    if signals.network_access && !signals.env_access && !signals.build_script_network {
        reasons.push(ReasonCode::UndeclaredNetwork);
    }
    if signals.env_access && !signals.network_access {
        reasons.push(ReasonCode::EnvRead);
    }
    if signals.filesystem_access {
        reasons.push(ReasonCode::UndeclaredFilesystem);
    }
    if signals.unsafe_block {
        reasons.push(ReasonCode::UnsafeBlock);
    }
    if signals.deserialization {
        reasons.push(ReasonCode::Deserialization);
    }
    if signals.high_entropy {
        reasons.push(ReasonCode::HighEntropy);
    }

    reasons
}

#[cfg(test)]
pub(super) fn test_shannon_entropy(value: &str) -> f64 {
    shannon_entropy(value)
}

#[cfg(test)]
pub(super) const fn test_entropy_threshold() -> f64 {
    ENTROPY_THRESHOLD
}
