use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum RuntimeMode {
    Combined,   // GUI + Web Admin
    Standalone, // Web Admin only (no GUI/tray)
}

#[derive(Debug, Clone, Copy)]
pub enum ModeSource {
    Cli,
    Env,
    Auto,
}

pub fn detect_runtime_mode() -> (RuntimeMode, ModeSource) {
    // 1. Check CLI args: --headless or --standalone
    let args: Vec<String> = std::env::args().collect();
    for arg in &args {
        if arg == "--headless" || arg == "--standalone" {
            return (RuntimeMode::Standalone, ModeSource::Cli);
        }
    }

    // 2. Check env vars: API_SWITCH_HEADLESS=1 or API_SWITCH_STANDALONE=1
    if let Ok(val) = std::env::var("API_SWITCH_HEADLESS") {
        if val == "1" || val == "true" {
            return (RuntimeMode::Standalone, ModeSource::Env);
        }
    }
    if let Ok(val) = std::env::var("API_SWITCH_STANDALONE") {
        if val == "1" || val == "true" {
            return (RuntimeMode::Standalone, ModeSource::Env);
        }
    }

    // 3. Default: Combined
    (RuntimeMode::Combined, ModeSource::Auto)
}
