mod admin;
mod commands;
mod database;
mod error;
mod proxy;
mod runtime_mode;
mod services;
mod state_version;

use admin::AdminServer;
use database::{AppSettings, Database};
use proxy::ProxyServer;
use runtime_mode::{ModeSource, RuntimeMode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

pub use error::AppError;

/// Latest translation relay result cached in memory for the Web Admin display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationRelayPayload {
    pub source_text: String,
    pub translated_text: String,
    pub source_lang: Option<String>,
    pub target_lang: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub updated_at: i64,
}

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub settings: Arc<tokio::sync::RwLock<AppSettings>>,
    pub proxy: Arc<tokio::sync::RwLock<Option<ProxyServer>>>,
    pub admin: Arc<tokio::sync::RwLock<Option<AdminServer>>>,
    pub translation_relay: Arc<tokio::sync::RwLock<Option<TranslationRelayPayload>>>,
    pub failure_counts: Arc<tokio::sync::RwLock<std::collections::HashMap<String, u32>>>,
    pub runtime_mode: RuntimeMode,
}

pub(crate) const TRAY_ID: &str = "api-switch-tray";
pub(crate) const EXPERIMENTAL_LAZY_TRAY_REFRESH: bool = false;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (runtime_mode, mode_source) = runtime_mode::detect_runtime_mode();
    log::info!(
        "Runtime mode: {:?} (source: {:?})",
        runtime_mode,
        mode_source
    );

    // 无桌面环境 → headless 模式（只启动转发+Web，不走 Tauri GUI）
    if should_run_headless(runtime_mode, mode_source) {
        run_headless();
        return;
    }

    let _app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            // Initialize database
            let db = Database::open()?;
            db.create_tables()?;
            let mut settings_cache = db.get_settings().unwrap_or_default();
            admin::apply_admin_env(&mut settings_cache);
            db.update_settings(&settings_cache)?;

        let state = AppState {
            db: Arc::new(db),
            settings: Arc::new(tokio::sync::RwLock::new(settings_cache)),
            proxy: Arc::new(tokio::sync::RwLock::new(None)),
            admin: Arc::new(tokio::sync::RwLock::new(None)),
            translation_relay: Arc::new(tokio::sync::RwLock::new(None)),
            failure_counts: Arc::new(
                tokio::sync::RwLock::new(std::collections::HashMap::new()),
            ),
            runtime_mode,
        };
            app.manage(state);

            // Auto-start proxy if proxy_enabled is set
            let handle = app.handle().clone();
            tauri::async_runtime::block_on(async {
                let app_state = handle.state::<AppState>();
                let settings = app_state.settings.read().await.clone();
                let admin_router = admin::build_combined_router(
                    &settings,
                    admin::AdminState::new_runtime(app_state.inner().clone(), handle.clone()),
                );
                if settings.proxy_enabled {
                    let port = settings.listen_port;
                    let server = ProxyServer::new(
                        port,
                        app_state.db.clone(),
                        app_state.settings.clone(),
Some(handle.clone()),
                        app_state.failure_counts.clone(),
                    );
                    if let Err(e) = server.start_with_admin(admin_router).await {
                        log::error!("Failed to auto-start proxy: {e}");
                    } else {
                        let mut proxy_guard = app_state.proxy.write().await;
                        *proxy_guard = Some(server);
                        log::info!("Proxy auto-started on port {port}");
                    }
                } else if admin_router.is_some() {
                    log::warn!(
                        "Web Admin single-port mode requires the proxy server to be running"
                    );
                }

                if let Err(e) = admin::start_admin_if_enabled(
                    app_state.inner().clone(),
                    handle.clone(),
                    app_state.admin.clone(),
                )
                .await
                {
                    log::error!("Failed to auto-start admin server: {e}");
                }
            });

        // Read settings to decide startup behavior
        let settings = app.state::<AppState>().settings.blocking_read().clone();
        let runtime_mode = app.state::<AppState>().runtime_mode;

        // Build tray icon and window management (Combined mode only)
        if runtime_mode == RuntimeMode::Combined {
            // Build tray icon (ref: cc-switch/src/lib.rs)
            // If tray build fails, fall back to Standalone behavior (skip tray/window but continue)
            match build_tray_menu(app.handle()) {
                Ok(tray_menu) => {
                    match tauri::tray::TrayIconBuilder::with_id(TRAY_ID)
                        .icon(app.default_window_icon().cloned().unwrap())
                        .menu(&tray_menu)
                        .show_menu_on_left_click(true)
                        .on_tray_icon_event(|tray, event| match event {
                            tauri::tray::TrayIconEvent::Click {
                                button: tauri::tray::MouseButton::Right,
                                button_state: tauri::tray::MouseButtonState::Up,
                                ..
                            } => {
                                if EXPERIMENTAL_LAZY_TRAY_REFRESH {
                                    let app = tray.app_handle().clone();
                                    tauri::async_runtime::spawn(async move {
                                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                                        if let Some(tray) = app.tray_by_id(TRAY_ID) {
                                            if let Ok(new_menu) = build_tray_menu(&app) {
                                                let _ = tray.set_menu(Some(new_menu));
                                            }
                                        }
                                    });
                                }
                            }
                            tauri::tray::TrayIconEvent::DoubleClick { .. } => {
                                if let Some(window) = tray.app_handle().get_webview_window("main") {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                            _ => {}
                        })
                        .on_menu_event(move |app, event| {
                            handle_tray_menu_event(app, &event.id.0);
                        })
                        .build(app)
                    {
                        Ok(_tray) => {
                            log::info!("Tray icon built successfully");
                        }
                        Err(e) => {
                            log::warn!("Failed to build tray icon: {e}. Falling back to Standalone behavior (no tray/window).");
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to build tray menu: {e}. Falling back to Standalone behavior (no tray/window).");
                }
            }

            // Show or keep hidden based on settings (only if tray succeeded)
            // Note: We still try to show the window even if tray failed, but the window close handler won't work properly
            if let Some(window) = app.get_webview_window("main") {
                if !settings.start_minimized {
                    let _ = window.show();
                }

                // Intercept window close → hide to tray instead of exiting
                let win = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win.hide();
                    }
                });
            }
        } else {
            log::info!("Running in Standalone mode - skipping tray and window management");
        }

            log::info!("API Switch initialized");
            Ok(())
        })
    .invoke_handler(tauri::generate_handler![
        commands::channel::list_channels,
        commands::channel::list_channels_paginated,
        commands::channel::create_channel,
        commands::channel::update_channel,
        commands::channel::update_channel_response_ms,
        commands::channel::delete_channel,
        commands::channel::fetch_models,
        commands::channel::fetch_models_direct,
        commands::channel::probe_url,
        commands::channel::test_channel,
        commands::channel::select_models,
        commands::pool::list_entries,
        commands::pool::list_entries_paginated,
        commands::pool::toggle_entry,
        commands::pool::batch_toggle_entries,
        commands::pool::reorder_entries,
        commands::pool::delete_entry,
        commands::pool::create_entry,
        commands::pool::backfill_entry_catalog_meta,
        commands::pool::test_entry_latency,
        commands::pool::update_entry_response_ms,
        commands::pool::get_all_groups,
        commands::pool::update_entry_group,
        commands::token::list_access_keys,
        commands::token::list_access_keys_paginated,
        commands::token::create_access_key,
        commands::token::delete_access_key,
        commands::token::toggle_access_key,
        commands::usage::get_usage_logs,
        commands::usage::get_dashboard_stats,
        commands::usage::get_model_consumption,
        commands::usage::get_call_trend,
        commands::usage::get_model_distribution,
        commands::usage::get_model_ranking,
        commands::usage::get_user_ranking,
        commands::usage::get_user_trend,
        commands::config::get_settings,
        commands::config::update_settings,
        commands::config::check_update,
        commands::proxy_cmd::start_proxy,
        commands::proxy_cmd::stop_proxy,
        commands::proxy_cmd::get_proxy_status,
        commands::proxy_cmd::refresh_tray_menu,
        commands::test_chat::test_chat,
        commands::cli::set_user_env_vars,
        commands::cli::get_cli_data,
        commands::limit::query_limit,
        commands::admin_cmd::get_admin_status,
        commands::translation::translate_and_relay,
        commands::translation::get_translation_relay,
    ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub(crate) fn build_tray_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let app_state = app.state::<AppState>();

    // Tray only reflects AUTO-group priority shortcuts.
    // It does not select groups or write active_group.
    let entries = app_state
        .db
        .get_enabled_entries_for_group("auto")
        .unwrap_or_default();

    // Use DB sort_index ordering for tray entries (no default sort mode applied)
    // Entries are already ordered by sort_index from the database
    // No additional sorting is performed here
    let top5: Vec<_> = entries.into_iter().take(5).collect();

    // 1. Show main window (top of menu)
    let show_item = MenuItem::with_id(app, "show_main", "Open Main Window", true, None::<String>)?;
    let separator1 = PredefinedMenuItem::separator(app)?;

    // 2. Top N AUTO-group entries (CheckMenuItem)
    let check_items: Vec<CheckMenuItem<tauri::Wry>> = top5
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let checked = i == 0;
            let label = match &entry.channel_name {
                Some(ch) => format!("{} / {}", entry.display_name, ch),
                None => entry.display_name.clone(),
            };
            CheckMenuItem::with_id(
                app,
                &format!("model:{}", entry.id),
                &label,
                true,
                checked,
                None::<String>,
            )
            .unwrap()
        })
        .collect();

    // 3. Quit
    let separator2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Exit", true, None::<String>)?;

    // Assemble menu
    let mut all: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = Vec::with_capacity(top5.len() + 5);
    all.push(&show_item as &dyn tauri::menu::IsMenuItem<_>);
    all.push(&separator1 as &dyn tauri::menu::IsMenuItem<_>);
    for item in &check_items {
        all.push(item);
    }
    all.push(&separator2 as &dyn tauri::menu::IsMenuItem<_>);
    all.push(&quit as &dyn tauri::menu::IsMenuItem<_>);

    Menu::with_items(app, &all)
}

use std::sync::OnceLock;

const TRAY_DEBOUNCE_MS: u64 = 1500;
static LAST_TRAY_REFRESH: OnceLock<std::sync::Mutex<std::time::Instant>> = OnceLock::new();

fn tray_debounce_check() -> bool {
    let now = std::time::Instant::now();
    let lock = LAST_TRAY_REFRESH.get_or_init(|| std::sync::Mutex::new(now));
    let Ok(mut last) = lock.lock() else { return false };
    if now.duration_since(*last).as_millis() < TRAY_DEBOUNCE_MS as u128 {
        return false; // 防抖：500ms 内不重复重建
    }
    *last = now;
    true
}

pub(crate) fn refresh_tray_if_enabled(app: &tauri::AppHandle) {
    if EXPERIMENTAL_LAZY_TRAY_REFRESH {
        return;
    }
    if !tray_debounce_check() {
        return;
    }
    if let Ok(new_menu) = build_tray_menu(app) {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_menu(Some(new_menu));
        }
    }
}

fn handle_tray_menu_event(app: &tauri::AppHandle, event_id: &str) {
    log::info!("[tray] menu event: {event_id}");

    match event_id {
        "quit" => {
            app.exit(0);
        }
        "show_main" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        _ => {
            if event_id.starts_with("model:") {
                // Tray click only reprioritizes an existing AUTO-group entry.
                // It does not change group routing or persist active_group.
                let entry_id = event_id
                    .strip_prefix("model:")
                    .unwrap_or(event_id)
                    .to_string();
                log::info!("[tray] setting AUTO-group priority for entry={entry_id}");

                {
                    let app_state = app.state::<AppState>();
                    let guard = app_state.db.conn.lock();
                    if let Ok(conn) = guard {
                        let now = chrono::Utc::now().timestamp();
                        let _ = conn.execute(
                            "UPDATE api_entries SET sort_index = sort_index + 1, updated_at = ?1 WHERE id != ?2 AND COALESCE(group_name, 'auto') = 'auto'",
                            rusqlite::params![now, entry_id],
                        );
                        let _ = conn.execute(
                            "UPDATE api_entries SET sort_index = 0, updated_at = ?1 WHERE id = ?2 AND COALESCE(group_name, 'auto') = 'auto'",
                            rusqlite::params![now, entry_id],
                        );
                    }
                }

                // Rebuild tray menu with updated AUTO-group priority.
                refresh_tray_if_enabled(app);

                // Notify frontend to refresh API Pool list
                let _ = app.emit("tray-priority-changed", ());
                crate::state_version::bump();
            }
        }
    }
}

/// 检测是否进入 headless 模式
fn should_run_headless(mode: RuntimeMode, source: ModeSource) -> bool {
    match source {
        // 用户明确指定了 --headless/--nodisktop 或 API_SWITCH_HEADLESS=1
        ModeSource::Cli | ModeSource::Env => mode == RuntimeMode::Standalone,
        // 没指定参数，自动检测桌面环境
        ModeSource::Auto => !has_desktop(),
    }
}

/// 桌面环境检测（仅 Linux 需要检查，Win/Mac 默认有桌面）
fn has_desktop() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// 无头模式入口：只启动转发+Web，不走 Tauri GUI
fn run_headless() {
    use admin::AdminState;
    use tokio::sync::RwLock;

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
        // DB 初始化
        let db = Database::open().expect("Failed to open database");
        db.create_tables().expect("Failed to create tables");
        let mut settings = db.get_settings().unwrap_or_default();
        admin::apply_admin_env(&mut settings);
        db.update_settings(&settings).ok();

        let settings = Arc::new(RwLock::new(settings));
        let db = Arc::new(db);

        // AppState（不需要 Tauri）
        let app_state = AppState {
            db: db.clone(),
            settings: settings.clone(),
            proxy: Arc::new(RwLock::new(None)),
            admin: Arc::new(RwLock::new(None)),
            translation_relay: Arc::new(RwLock::new(None)),
            failure_counts: Arc::new(RwLock::new(HashMap::new())),
            runtime_mode: RuntimeMode::Standalone,
        };

        // 启动转发（app_handle = None）
        // 无头模式：强制开启 Web Admin，不管配置里是 0 还是 1
        {
            let mut w = settings.write().await;
            w.web_admin_enabled = true;
        }
        let settings_snapshot = settings.read().await.clone();

        let admin_router = admin::build_combined_router(
            &settings_snapshot,
            AdminState {
                db: db.clone(),
                settings: settings.clone(),
                login_sessions: Arc::new(RwLock::new(HashMap::new())),
                login_failures: Arc::new(Mutex::new(HashMap::new())),
                runtime: Some(app_state.clone()),
                app_handle: None,
            },
        );

        if settings_snapshot.proxy_enabled {
            let server = ProxyServer::new(
                settings_snapshot.listen_port,
                db,
                settings,
                None,
                app_state.failure_counts.clone(),
            );
            if let Err(e) = server.start_with_admin(admin_router).await {
                log::error!("Failed to start proxy: {e}");
            } else {
                let mut proxy_guard = app_state.proxy.write().await;
                *proxy_guard = Some(server);
            }
        }

        let port = settings_snapshot.listen_port;
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("  API Switch is running");
        println!("  Proxy:      http://127.0.0.1:{}/v1/...", port);
        println!("  Web Admin:  http://127.0.0.1:{}", port);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("  Press Ctrl+C to stop");

        // 等待 Ctrl+C
        tokio::signal::ctrl_c().await.expect("Failed to listen for ctrl+c");
        println!("\nShutting down...");

        // 优雅停止代理
        {
            let mut proxy_guard = app_state.proxy.write().await;
            if let Some(server) = proxy_guard.take() {
                let _ = server.stop().await;
            }
        }
    });
}
