//! The eight providers, `provider::model` names and the saved settings.

use serde::{Deserialize, Serialize};

/// A provider, with the ids of Eagle's `ai-sdk`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum ProviderId {
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "google")]
    Google,
    #[serde(rename = "deepseek")]
    DeepSeek,
    #[serde(rename = "tongyi")]
    Tongyi,
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "lmstudio")]
    LmStudio,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

/// Where a provider runs, by what it is rather than by its base URL.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Cloud,
    Local,
    Custom,
}

impl ProviderId {
    pub const ALL: [ProviderId; 8] = [
        ProviderId::OpenAi,
        ProviderId::Anthropic,
        ProviderId::Google,
        ProviderId::DeepSeek,
        ProviderId::Tongyi,
        ProviderId::Ollama,
        ProviderId::LmStudio,
        ProviderId::OpenAiCompatible,
    ];

    /// The id in `provider::model` names and saved settings.
    pub fn id(self) -> &'static str {
        match self {
            ProviderId::OpenAi => "openai",
            ProviderId::Anthropic => "anthropic",
            ProviderId::Google => "google",
            ProviderId::DeepSeek => "deepseek",
            ProviderId::Tongyi => "tongyi",
            ProviderId::Ollama => "ollama",
            ProviderId::LmStudio => "lmstudio",
            ProviderId::OpenAiCompatible => "openai-compatible",
        }
    }

    pub fn from_id(id: &str) -> Option<ProviderId> {
        ProviderId::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The product name, shown untranslated.
    pub fn name(self) -> &'static str {
        match self {
            ProviderId::OpenAi => "OpenAI",
            ProviderId::Anthropic => "Anthropic",
            ProviderId::Google => "Google Gemini",
            ProviderId::DeepSeek => "DeepSeek",
            ProviderId::Tongyi => "Tongyi Qwen",
            ProviderId::Ollama => "Ollama",
            ProviderId::LmStudio => "LM Studio",
            ProviderId::OpenAiCompatible => "OpenAI-compatible",
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            ProviderId::Ollama | ProviderId::LmStudio => Kind::Local,
            ProviderId::OpenAiCompatible => Kind::Custom,
            _ => Kind::Cloud,
        }
    }

    /// The base URL when the user gives none; none for a custom server.
    pub fn default_base(self) -> Option<&'static str> {
        match self {
            ProviderId::OpenAi => Some("https://api.openai.com/v1"),
            ProviderId::Anthropic => Some("https://api.anthropic.com/v1"),
            ProviderId::Google => Some("https://generativelanguage.googleapis.com/v1beta"),
            ProviderId::DeepSeek => Some("https://api.deepseek.com/v1"),
            ProviderId::Tongyi => Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
            ProviderId::Ollama => Some("http://localhost:11434/v1"),
            ProviderId::LmStudio => Some("http://localhost:1234/v1"),
            ProviderId::OpenAiCompatible => None,
        }
    }

    /// Whether requests can't go without a key.
    pub fn needs_key(self) -> bool {
        self.kind() == Kind::Cloud
    }

    /// Whether the provider takes a key at all.
    pub fn takes_key(self) -> bool {
        self.kind() != Kind::Local
    }
}

/// A model named `provider::model`. The model part may hold single colons
/// (`ollama::qwen3.8:27b`), so the name splits at the first `::`.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct ModelRef {
    pub provider: ProviderId,
    pub model: String,
}

impl ModelRef {
    pub fn parse(name: &str) -> Option<ModelRef> {
        let (provider, model) = name.trim().split_once("::")?;
        let model = model.trim();
        if model.is_empty() {
            return None;
        }
        Some(ModelRef { provider: ProviderId::from_id(provider.trim())?, model: model.to_string() })
    }

    pub fn name(&self) -> String {
        format!("{}::{}", self.provider.id(), self.model)
    }
}

/// One provider the user added. Never holds its key.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ProviderSettings {
    pub id: ProviderId,
    /// As typed; empty means the provider's default.
    #[serde(default)]
    pub base_url: String,
    /// The list last fetched from the provider.
    #[serde(default)]
    pub models: Vec<String>,
}

/// What Preferences ▸ AI Models keeps: no providers by default, so nothing
/// can be sent anywhere until the user adds one.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Settings {
    pub providers: Vec<ProviderSettings>,
    /// `provider::model`, or empty.
    pub default_chat: String,
    /// `provider::model`, or empty.
    pub default_vision: String,
}

impl Settings {
    pub fn provider(&self, id: ProviderId) -> Option<&ProviderSettings> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Adds a provider with its default base URL. A second add changes nothing.
    pub fn add(&mut self, id: ProviderId) {
        if self.provider(id).is_none() {
            self.providers.push(ProviderSettings { id, base_url: String::new(), models: Vec::new() });
        }
    }

    /// Removes a provider and any default that names it.
    pub fn remove(&mut self, id: ProviderId) {
        self.providers.retain(|p| p.id != id);
        let names_it = |name: &str| ModelRef::parse(name).is_some_and(|m| m.provider == id);
        if names_it(&self.default_chat) {
            self.default_chat.clear();
        }
        if names_it(&self.default_vision) {
            self.default_vision.clear();
        }
    }

    /// A default model, if it names a provider that was added.
    fn added_model(&self, name: &str) -> Option<ModelRef> {
        ModelRef::parse(name).filter(|m| self.provider(m.provider).is_some())
    }

    pub fn chat_model(&self) -> Option<ModelRef> {
        self.added_model(&self.default_chat)
    }

    pub fn vision_model(&self) -> Option<ModelRef> {
        self.added_model(&self.default_vision)
    }

    /// Every fetched model as `provider::model`, in provider order.
    pub fn all_models(&self) -> Vec<String> {
        self.providers.iter().flat_map(|p| p.models.iter().map(|m| format!("{}::{m}", p.id.id()))).collect()
    }

    /// The base URL requests to `id` use, checked with [`check_base`].
    pub fn base_url(&self, id: ProviderId) -> Result<String, BaseError> {
        let typed = self.provider(id).map_or("", |p| p.base_url.trim());
        let base = if typed.is_empty() { id.default_base().ok_or(BaseError::Missing)? } else { typed };
        check_base(base)
    }
}

/// Why a base URL can't be used.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseError {
    /// The custom provider has no default.
    Missing,
    /// Not http or https, or it holds a user name, a query or a fragment.
    Invalid,
}

/// Accepts `http` and `https` URLs with a host and no user name, query or
/// fragment (a key belongs in the Keychain, not in a URL). Returns the URL
/// without trailing slashes.
pub fn check_base(base: &str) -> Result<String, BaseError> {
    let base = base.trim().trim_end_matches('/');
    let rest = base.strip_prefix("https://").or_else(|| base.strip_prefix("http://")).ok_or(BaseError::Invalid)?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || rest.contains(['@', '?', '#']) || rest.contains(char::is_whitespace) {
        return Err(BaseError::Invalid);
    }
    Ok(base.to_string())
}

/// The host and port of a checked base URL, for saying where items go.
pub fn host_of(base: &str) -> &str {
    let rest = base.strip_prefix("https://").or_else(|| base.strip_prefix("http://")).unwrap_or(base);
    rest.split('/').next().unwrap_or(rest)
}

/// Whether a checked base URL reaches this Mac only.
pub fn is_loopback(base: &str) -> bool {
    let host = host_of(base);
    let name = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host.split(':').next().unwrap_or(""),
    };
    let dotted_127 = name.starts_with("127.") && name.split('.').count() == 4 && name.split('.').all(|p| p.parse::<u8>().is_ok());
    name.eq_ignore_ascii_case("localhost") || name == "::1" || dotted_127
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_the_saved_names() {
        for p in ProviderId::ALL {
            assert_eq!(ProviderId::from_id(p.id()), Some(p));
            assert_eq!(serde_json::to_string(&p).unwrap(), format!("\"{}\"", p.id()));
        }
        assert_eq!(ProviderId::from_id("nope"), None);
    }

    #[test]
    fn only_the_custom_provider_lacks_a_default_base() {
        for p in ProviderId::ALL {
            assert_eq!(p.default_base().is_none(), p == ProviderId::OpenAiCompatible);
        }
        assert_eq!(ProviderId::Ollama.default_base(), Some("http://localhost:11434/v1"));
    }

    #[test]
    fn model_names_split_at_the_first_double_colon() {
        let m = ModelRef::parse("ollama::qwen3.8:27b").unwrap();
        assert_eq!((m.provider, m.model.as_str()), (ProviderId::Ollama, "qwen3.8:27b"));
        assert_eq!(m.name(), "ollama::qwen3.8:27b");
        let c = ModelRef::parse("openai-compatible::a::b").unwrap();
        assert_eq!(c.model, "a::b");
        for bad in ["", "ollama", "ollama::", "::x", "nope::x", "ollama:x"] {
            assert_eq!(ModelRef::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn defaults_need_their_provider_added() {
        let mut s = Settings { default_vision: "ollama::m".into(), default_chat: "google::g".into(), ..Default::default() };
        assert_eq!(s.vision_model(), None);
        s.add(ProviderId::Ollama);
        s.add(ProviderId::Ollama);
        assert_eq!(s.providers.len(), 1);
        assert_eq!(s.vision_model().unwrap().model, "m");
        assert_eq!(s.chat_model(), None);
        s.remove(ProviderId::Ollama);
        assert_eq!(s.default_vision, "");
        assert_eq!(s.default_chat, "google::g", "another provider's default stays");
    }

    #[test]
    fn nothing_is_configured_by_default_and_settings_have_no_key() {
        let mut s = Settings::default();
        assert!(s.providers.is_empty() && s.chat_model().is_none() && s.vision_model().is_none());
        s.add(ProviderId::OpenAi);
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.to_lowercase().contains("key"), "{text}");
        assert_eq!(serde_json::from_str::<Settings>(&text).unwrap(), s);
        assert_eq!(serde_json::from_str::<Settings>("{}").unwrap(), Settings::default());
    }

    #[test]
    fn base_urls_are_checked() {
        let mut s = Settings::default();
        s.add(ProviderId::Ollama);
        assert_eq!(s.base_url(ProviderId::Ollama).unwrap(), "http://localhost:11434/v1");
        s.providers[0].base_url = " http://127.0.0.1:9/v1/ ".into();
        assert_eq!(s.base_url(ProviderId::Ollama).unwrap(), "http://127.0.0.1:9/v1");
        assert_eq!(s.base_url(ProviderId::OpenAiCompatible), Err(BaseError::Missing));
        for bad in ["ftp://x", "http://", "https://u:p@x/v1", "https://x/v1?key=1", "https://x/#a", "x.com", "https://a b/"] {
            assert_eq!(check_base(bad), Err(BaseError::Invalid), "{bad}");
        }
    }

    #[test]
    fn loopback_hosts() {
        for b in ["http://localhost:11434/v1", "http://127.0.0.1/v1", "http://[::1]:80/v1", "https://LOCALHOST"] {
            assert!(is_loopback(b), "{b}");
        }
        for b in ["http://192.168.1.2:1/v1", "https://api.openai.com/v1", "http://localhost.evil.example/v1", "http://127.example.com/"] {
            assert!(!is_loopback(b), "{b}");
        }
        assert_eq!(host_of("http://localhost:11434/v1"), "localhost:11434");
    }
}
