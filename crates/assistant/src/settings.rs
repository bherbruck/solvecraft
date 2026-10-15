//! The assistant's settings: which backend, endpoints, models, keys and agent commands.
//!
//! They live in their own file in the data folder (`<data>/assistant/settings.json`, readable
//! only by the user), never in the app's preferences: those go to a plain-text store, and keys
//! and agent environment values must not. Loading is lenient, as everywhere in SolidCraft: a
//! missing file, unknown fields and wrong types fall back to the defaults, and sizes are capped,
//! because the file may have been edited by hand or by something else.

use std::path::PathBuf;

use serde_json::{Map, Value, json};

/// A string that must not leak: [`Debug`](std::fmt::Debug) hides it, and it has no `Serialize`.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The text, for the one place that sends it (a request header, a child's environment).
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "Secret(empty)" } else { "Secret(…)" })
    }
}

/// Which kind of assistant answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackendKind {
    #[default]
    Anthropic,
    /// An OpenAI-compatible chat-completions endpoint (OpenAI, Ollama, LM Studio, …).
    OpenAi,
    /// An external agent program chosen from [`Settings::agents`].
    Agent,
}

impl BackendKind {
    pub fn id(self) -> &'static str {
        match self {
            BackendKind::Anthropic => "anthropic",
            BackendKind::OpenAi => "openai",
            BackendKind::Agent => "agent",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "anthropic" => Some(BackendKind::Anthropic),
            "openai" => Some(BackendKind::OpenAi),
            "agent" => Some(BackendKind::Agent),
            _ => None,
        }
    }
}

/// An API endpoint with its model and key.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiProfile {
    pub base_url: String,
    pub model: String,
    pub api_key: Secret,
    /// The model accepts images (screenshots are offered to it).
    pub vision: bool,
    /// Plain `http://` is acceptable for this host (a server on the local network).
    pub allow_http: bool,
    /// Tokens the model can read, for trimming the history.
    pub context_window: u32,
    /// Price per million input and output tokens (0 = unknown, no cost shown).
    pub price_in: f64,
    pub price_out: f64,
}

pub const MAX_FIELD: usize = 4096;
pub const MAX_AGENTS: usize = 32;
pub const MAX_ARGS: usize = 64;
pub const MAX_ENV: usize = 64;
pub const MAX_FOLDERS: usize = 32;
const MAX_FILE: u64 = 256 * 1024;
const MIN_CONTEXT: u32 = 2_000;
const MAX_CONTEXT: u32 = 10_000_000;

impl ApiProfile {
    fn anthropic() -> Self {
        Self {
            base_url: "https://api.anthropic.com".into(),
            model: "claude-sonnet-5-5".into(),
            api_key: Secret::default(),
            vision: true,
            allow_http: false,
            context_window: 200_000,
            price_in: 0.0,
            price_out: 0.0,
        }
    }

    fn openai() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            model: String::new(),
            api_key: Secret::default(),
            vision: false,
            allow_http: false,
            context_window: 128_000,
            price_in: 0.0,
            price_out: 0.0,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "base_url": self.base_url,
            "model": self.model,
            "api_key": self.api_key.expose(),
            "vision": self.vision,
            "allow_http": self.allow_http,
            "context_window": self.context_window,
            "price_in": self.price_in,
            "price_out": self.price_out,
        })
    }

    fn merge_json(&mut self, v: &Value) {
        if let Some(s) = text(v, "base_url") {
            self.base_url = s;
        }
        if let Some(s) = text(v, "model") {
            self.model = s;
        }
        if let Some(s) = text(v, "api_key") {
            self.api_key = Secret::new(s);
        }
        if let Some(b) = v.get("vision").and_then(Value::as_bool) {
            self.vision = b;
        }
        if let Some(b) = v.get("allow_http").and_then(Value::as_bool) {
            self.allow_http = b;
        }
        if let Some(n) = v.get("context_window").and_then(Value::as_u64) {
            self.context_window = n.clamp(MIN_CONTEXT as u64, MAX_CONTEXT as u64) as u32;
        }
        for (key, slot) in [("price_in", &mut self.price_in), ("price_out", &mut self.price_out)] {
            if let Some(n) = v.get(key).and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0 && *n < 1.0e6) {
                *slot = n;
            }
        }
    }

    /// The key to send: the one saved here, else the provider's usual environment variables.
    pub fn resolved_key(&self, kind: BackendKind) -> Secret {
        if !self.api_key.is_empty() {
            return self.api_key.clone();
        }
        let names: &[&str] = match kind {
            BackendKind::Anthropic => &["ANTHROPIC_API_KEY"],
            BackendKind::OpenAi => &["OPENAI_API_KEY"],
            BackendKind::Agent => &[],
        };
        let prefixed = match kind {
            BackendKind::Anthropic => solidcraft_engine::geom::env::var("ANTHROPIC_API_KEY"),
            BackendKind::OpenAi => solidcraft_engine::geom::env::var("OPENAI_API_KEY"),
            BackendKind::Agent => None,
        };
        let from_env = prefixed.or_else(|| names.iter().find_map(|n| std::env::var(n).ok()));
        Secret::new(from_env.map(|k| k.trim().to_string()).unwrap_or_default())
    }
}

/// How an external agent is driven.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgentMode {
    /// Agent Client Protocol over the program's stdin/stdout.
    #[default]
    Acp,
    /// One prompt in, plain text out.
    Print,
}

/// An external agent program.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentProfile {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    /// Environment for the program; values may be keys, so they are secrets.
    pub env: Vec<(String, Secret)>,
    pub mode: AgentMode,
}

impl AgentProfile {
    fn to_json(&self) -> Value {
        let env: Map<String, Value> = self.env.iter().map(|(k, v)| (k.clone(), Value::String(v.expose().to_string()))).collect();
        json!({
            "name": self.name,
            "command": self.command,
            "args": self.args,
            "env": env,
            "mode": match self.mode { AgentMode::Acp => "acp", AgentMode::Print => "print" },
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let name = text(v, "name")?;
        let command = text(v, "command")?;
        if name.trim().is_empty() || command.trim().is_empty() {
            return None;
        }
        let args = v
            .get("args")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).filter(|s| s.len() <= MAX_FIELD).take(MAX_ARGS).map(str::to_string).collect())
            .unwrap_or_default();
        let env = v
            .get("env")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| Some((k.clone(), Secret::new(v.as_str()?))))
                    .filter(|(k, v)| valid_env_name(k) && v.expose().len() <= MAX_FIELD)
                    .take(MAX_ENV)
                    .collect()
            })
            .unwrap_or_default();
        let mode = if v.get("mode").and_then(Value::as_str) == Some("print") { AgentMode::Print } else { AgentMode::Acp };
        Some(Self { name, command, args, env, mode })
    }
}

/// A name usable as an environment variable: letters, digits and `_`, not starting with a digit.
pub fn valid_env_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Everything the assistant remembers between launches.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub backend: BackendKind,
    pub anthropic: ApiProfile,
    pub openai: ApiProfile,
    pub agents: Vec<AgentProfile>,
    /// Index into [`agents`](Self::agents).
    pub agent: usize,
    /// Pick "allow once" automatically when an agent asks to run one of SolidCraft's own tools.
    pub auto_allow_tools: bool,
    /// Folders an external agent may read and write in besides the design's own.
    pub folders: Vec<String>,
    /// The `solidcraft-cli` program agents start as their MCP server (empty: next to the app,
    /// then on `PATH`).
    pub cli: String,
    /// The user has seen what is sent where.
    pub privacy_notice_seen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            backend: BackendKind::Anthropic,
            anthropic: ApiProfile::anthropic(),
            openai: ApiProfile::openai(),
            agents: Vec::new(),
            agent: 0,
            auto_allow_tools: true,
            folders: Vec::new(),
            cli: String::new(),
            privacy_notice_seen: false,
        }
    }
}

impl Settings {
    /// The active API profile (the Anthropic one for the agent backend, which has none).
    pub fn api(&self) -> &ApiProfile {
        match self.backend {
            BackendKind::OpenAi => &self.openai,
            _ => &self.anthropic,
        }
    }

    pub fn selected_agent(&self) -> Option<&AgentProfile> {
        self.agents.get(self.agent)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "version": 1,
            "backend": self.backend.id(),
            "anthropic": self.anthropic.to_json(),
            "openai": self.openai.to_json(),
            "agents": self.agents.iter().map(AgentProfile::to_json).collect::<Vec<_>>(),
            "agent": self.agent,
            "auto_allow_tools": self.auto_allow_tools,
            "folders": self.folders,
            "cli": self.cli,
            "privacy_notice_seen": self.privacy_notice_seen,
        })
    }

    /// Settings from JSON text; anything wrong falls back to the defaults, field by field.
    pub fn from_json(v: &Value) -> Self {
        let mut s = Settings::default();
        if let Some(k) = v.get("backend").and_then(Value::as_str).and_then(BackendKind::from_id) {
            s.backend = k;
        }
        if let Some(p) = v.get("anthropic") {
            s.anthropic.merge_json(p);
        }
        if let Some(p) = v.get("openai") {
            s.openai.merge_json(p);
        }
        if let Some(a) = v.get("agents").and_then(Value::as_array) {
            s.agents = a.iter().filter_map(AgentProfile::from_json).take(MAX_AGENTS).collect();
        }
        if let Some(i) = v.get("agent").and_then(Value::as_u64) {
            s.agent = usize::try_from(i).unwrap_or(0);
        }
        if s.agent >= s.agents.len() {
            s.agent = 0;
        }
        if let Some(b) = v.get("auto_allow_tools").and_then(Value::as_bool) {
            s.auto_allow_tools = b;
        }
        if let Some(f) = v.get("folders").and_then(Value::as_array) {
            s.folders =
                f.iter().filter_map(Value::as_str).filter(|p| !p.is_empty() && p.len() <= MAX_FIELD).take(MAX_FOLDERS).map(str::to_string).collect();
        }
        if let Some(c) = text(v, "cli") {
            s.cli = c;
        }
        if let Some(b) = v.get("privacy_notice_seen").and_then(Value::as_bool) {
            s.privacy_notice_seen = b;
        }
        s
    }

    /// `<data folder>/assistant/settings.json` (`SOLIDCRAFT_ASSISTANT_DIR` replaces the folder).
    pub fn path() -> Option<PathBuf> {
        Some(data_folder()?.join("settings.json"))
    }

    /// Read the settings file; a missing, unreadable, oversized or malformed one gives the
    /// defaults.
    pub fn load() -> Self {
        Self::path().map(|p| Self::load_from(&p)).unwrap_or_default()
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        let ok = std::fs::metadata(path).map(|m| m.len() <= MAX_FILE).unwrap_or(false);
        if !ok {
            return Self::default();
        }
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| Self::from_json(&v)).unwrap_or_default()
    }

    /// Write the settings file, readable only by the user.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no data folder")?;
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        let text = serde_json::to_vec_pretty(&self.to_json()).map_err(|e| e.to_string())?;
        solidcraft_engine::control_auth::write_private_file(path, &text).map_err(|e| format!("cannot save the assistant settings: {e}"))
    }
}

/// The assistant's folder under the data folder (also where conversations go).
pub fn data_folder() -> Option<PathBuf> {
    match solidcraft_engine::geom::env::var_os("ASSISTANT_DIR").filter(|d| !d.is_empty()) {
        Some(d) => Some(PathBuf::from(d)),
        None => Some(solidcraft_engine::recovery::data_dir()?.join("assistant")),
    }
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).filter(|s| s.len() <= MAX_FIELD).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_never_print() {
        let p = ApiProfile { api_key: Secret::new("sk-live-abc"), ..ApiProfile::anthropic() };
        let a = AgentProfile {
            name: "x".into(),
            command: "x".into(),
            args: vec![],
            env: vec![("KEY".into(), Secret::new("hunter2"))],
            mode: AgentMode::Acp,
        };
        let shown = format!("{p:?} {a:?} {:?}", Settings { anthropic: p.clone(), agents: vec![a.clone()], ..Settings::default() });
        assert!(!shown.contains("sk-live-abc") && !shown.contains("hunter2"), "{shown}");
    }

    #[test]
    fn settings_round_trip() {
        let mut s = Settings { backend: BackendKind::Agent, ..Settings::default() };
        s.openai.base_url = "http://localhost:11434/v1".into();
        s.openai.model = "qwen3".into();
        s.openai.api_key = Secret::new("k");
        s.openai.context_window = 32_000;
        s.agents.push(AgentProfile {
            name: "Open".into(),
            command: "opencode".into(),
            args: vec!["acp".into()],
            env: vec![("A_B".into(), Secret::new("v"))],
            mode: AgentMode::Print,
        });
        s.agent = 0;
        s.folders.push("/home/me/designs".into());
        s.cli = "/opt/solidcraft/solidcraft-cli".into();
        s.privacy_notice_seen = true;
        assert_eq!(Settings::from_json(&s.to_json()), s);
    }

    #[test]
    fn garbage_falls_back_field_by_field() {
        let d = Settings::default();
        assert_eq!(Settings::from_json(&json!("nonsense")), d);
        assert_eq!(Settings::from_json(&json!([1, 2])), d);
        let v = json!({
            "backend": 7,
            "anthropic": {"model": 5, "context_window": -3, "price_in": "x", "vision": "yes"},
            "openai": {"context_window": 1, "price_out": -1.0, "model": "m"},
            "agents": [1, {"name": "", "command": "x"}, {"name": "ok", "command": "c", "args": [1, "a"], "env": {"1BAD": "x", "OK": "y", "N": 3}}],
            "agent": 99,
            "folders": ["", 5, "/x"],
        });
        let s = Settings::from_json(&v);
        assert_eq!(s.backend, BackendKind::Anthropic);
        assert_eq!(s.anthropic, d.anthropic);
        assert_eq!(s.openai.model, "m");
        assert_eq!(s.openai.context_window, MIN_CONTEXT);
        assert_eq!(s.openai.price_out, 0.0);
        assert_eq!(s.agents.len(), 1);
        assert_eq!(s.agents[0].args, vec!["a".to_string()]);
        assert_eq!(s.agents[0].env, vec![("OK".to_string(), Secret::new("y"))]);
        assert_eq!(s.agent, 0);
        assert_eq!(s.folders, vec!["/x".to_string()]);
    }

    #[test]
    fn lists_are_capped() {
        let agents: Vec<Value> = (0..200).map(|i| json!({"name": format!("a{i}"), "command": "c"})).collect();
        let folders: Vec<Value> = (0..200).map(|i| json!(format!("/f{i}"))).collect();
        let s = Settings::from_json(&json!({"agents": agents, "folders": folders}));
        assert_eq!(s.agents.len(), MAX_AGENTS);
        assert_eq!(s.folders.len(), MAX_FOLDERS);
    }

    #[test]
    fn env_names_are_checked() {
        for ok in ["PATH", "A_1", "_x"] {
            assert!(valid_env_name(ok), "{ok}");
        }
        for bad in ["", "1A", "A-B", "A B", "A=B", "ä"] {
            assert!(!valid_env_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_missing_or_broken_file_gives_defaults() {
        let dir = std::env::temp_dir().join(format!("solidcraft-assistant-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        assert_eq!(Settings::load_from(&path), Settings::default());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());
        std::fs::write(&path, vec![b' '; (MAX_FILE + 1) as usize]).unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());
        let mut s = Settings::default();
        s.anthropic.api_key = Secret::new("sk-test");
        s.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).anthropic.api_key, Secret::new("sk-test"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_saved_key_wins_over_the_environment() {
        let p = ApiProfile { api_key: Secret::new("saved"), ..ApiProfile::openai() };
        assert_eq!(p.resolved_key(BackendKind::OpenAi), Secret::new("saved"));
    }
}
