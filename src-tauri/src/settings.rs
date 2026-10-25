use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InsertMode {
    /// Clipboard + Ctrl+V (fast, works almost everywhere).
    Paste,
    /// Simulated Unicode keystrokes (slower; for apps that block paste).
    Type,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub stt_provider: String,
    pub stt_model: String,
    pub cleanup_provider: String,
    pub cleanup_model: String,
    /// ISO-639-1 code to force a language; empty = auto-detect (needed for mixing).
    pub language: String,
    /// Mic by name; empty = system default.
    pub mic_device: String,
    pub insert_mode: InsertMode,
    pub restore_clipboard: bool,
    pub restore_delay_ms: u64,
    pub openai_base_url: String,
    pub groq_base_url: String,
    /// Custom URL provider (OpenAI-compatible server); shared by STT and cleanup.
    pub custom_base_url: String,
    /// Base URL overrides for the other cloud providers, by id; missing = official endpoint.
    pub base_urls: std::collections::BTreeMap<String, String>,
    pub keep_recordings: usize,
    /// Speech gate (dBFS). Quieter clips are discarded instead of sent to the API.
    pub silence_threshold_dbfs: f32,
    /// UI accent: azure | coral | tangerine | sunflower | mint | violet | chalk
    pub accent: String,
    /// Dictation chord, 1-3 key names (see hotkey::keys::parse_key).
    pub hotkey: Vec<String>,
    /// Other hotkey actions by name (hands_free, cancel, paste_last, toggle_pause, command_mode).
    /// Missing = default; present but empty = disabled.
    pub hotkeys: std::collections::BTreeMap<String, Vec<String>>,
    /// Hands-free starts with a double-tap of the dictate chord (true) or with its own hotkey,
    /// `hotkeys["hands_free"]` (false; double-tap is then off).
    pub hands_free_double_tap: bool,
    /// UI theme: system | light | dark
    pub theme: String,
    /// Start/stop ticks (placeholder: the sounds themselves land later).
    pub sounds_enabled: bool,
    /// First-run onboarding finished.
    pub onboarded: bool,
    /// How the user types by hand: full | light | none (shapes the cleanup prompt).
    pub typing_style: String,
    /// Ids ("r1".."r12") of default cleanup rules the user switched off.
    pub disabled_rules: Vec<String>,
    /// Extra plain-English cleanup rules the user wrote.
    pub custom_rules: Vec<String>,
    /// "Work in admin windows": relaunch elevated at startup (UAC prompt).
    pub admin_mode: bool,
    /// "Start with Windows" / "Open at login". Re-applied at every launch: reinstalling over an
    /// older version runs its uninstaller, which deletes the Run entry.
    pub start_with_windows: bool,
    /// About › "Install updates automatically": download in the background, then offer Restart now.
    pub auto_update: bool,
    /// About › Update channel: stable | beta (beta also gets prereleases).
    pub update_channel: String,
    /// llama-server binary for local cleanup; empty = next to the app exe (Tauri sidecar spot).
    pub llama_server_path: String,
    /// Local models: always | idle (unload after 15 min idle) | after_use. Default: always on a GPU build.
    pub keep_models_loaded: String,
    /// General › "Use text near the cursor": send up to 500 chars before the cursor to cleanup as
    /// context. Off by default: that text goes to the cleanup provider. Never stored or logged.
    pub cursor_context: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            stt_provider: "openai".into(),
            stt_model: "gpt-transcribe".into(),
            cleanup_provider: "openai".into(),
            cleanup_model: "gpt-6-luna".into(),
            language: String::new(),
            mic_device: String::new(),
            insert_mode: InsertMode::Paste,
            restore_clipboard: true,
            restore_delay_ms: 250,
            openai_base_url: "https://api.openai.com/v1".into(),
            groq_base_url: "https://api.groq.com/openai/v1".into(),
            custom_base_url: String::new(),
            base_urls: Default::default(),
            keep_recordings: 50,
            silence_threshold_dbfs: -45.0,
            accent: "azure".into(),
            hotkey: crate::hotkey::keys::chord_names(&crate::hotkey::actions::ActionId::Dictate.default_codes()),
            hotkeys: Default::default(),
            hands_free_double_tap: true,
            onboarded: false,
            typing_style: "full".into(),
            disabled_rules: Vec::new(),
            custom_rules: Vec::new(),
            admin_mode: false,
            start_with_windows: true,
            auto_update: true,
            cursor_context: false,
            update_channel: "stable".into(),
            llama_server_path: String::new(),
            keep_models_loaded: if crate::models::GPU_BUILD { "always" } else { "idle" }.into(),
            theme: "system".into(),
            sounds_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn start_with_windows_is_on_for_settings_saved_before_it_existed() {
        let s: super::Settings = serde_json::from_str(r#"{"onboarded": true}"#).unwrap();
        assert!(s.start_with_windows);
    }
}
