#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] //this gets rid of the terminal popup

use eframe::egui;
use once_cell::sync::Lazy;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};
use tray_icon::TrayIcon;
mod dashboard; //the entire dashboard with all options
mod login; //Login Page
mod network; //BY FAR most important. Interact with API
//-------------------------------------------------------------------
// -- Ini Files --
const CONFIG_FILE: &str = "clientConfig.ini";
const STYLE_FILE: &str = "style.ini";
const PROTECTIONS_FILE: &str = "protections.ini";
// --- Data Structures ---
#[derive(Serialize)]
struct AuthRequest {
    password: String,
}
#[derive(serde::Deserialize, serde::Serialize, Clone, Default)]
struct AppConfig {
    max_upload_size: u64,
    upload_speed_bps: u64,
}
#[derive(Deserialize)]
struct AuthResponse {
    token: Option<String>,
}

pub struct QueuedUpload {
    pub file_path: PathBuf,
    pub target_folder: String,
}

#[derive(Deserialize, Clone)]
struct FileInfo {
    name: String,
    size: u64,
    is_dir: bool,
    #[serde(default)]
    modified: u64, // Unix timestamp from the host
}

#[derive(PartialEq, Clone, Copy)]
enum SortMode {
    NameAsc,
    NewestFirst,
    OldestFirst,
    SizeDesc,
}

#[derive(Deserialize)]
struct ListResponse {
    files: Vec<FileInfo>,
}

#[derive(Serialize)]
struct MoveRequest {
    source_path: String,
    destination_path: String,
}

#[derive(Clone, Copy)]
struct AppTheme {
    background: egui::Color32,
    text_primary: egui::Color32,
    text_dashboard: egui::Color32,
    text_title: egui::Color32,
    list_txt: egui::Color32,
    download_btn: egui::Color32,
    upload_btn: egui::Color32,
    delete_btn: egui::Color32,
    move_btn: egui::Color32,
    logout_btn: egui::Color32,
    connect_btn: egui::Color32,
    open_btn: egui::Color32,
    refresh_btn: egui::Color32,
    path_btn: egui::Color32,
    folder_btn: egui::Color32,
    theme_btn: egui::Color32,
} //add onto these as you need to.
//------------------------------------------------------------------------------------------
// --- Background Worker Messages ---
enum AppMsg {
    LoginSuccess(String),
    LoginFailed(String),
    FilesLoaded(Vec<FileInfo>),
    ActionSuccess(String), // Used for upload/delete/create success
    Error(String),
    UploadProgress(f32), //for the upload status
    ImageLoaded(String, egui::ColorImage),
    ImageFailed(String),
    ConfigLoaded(AppConfig),
    ConfigSaved,
    RestoreWindow,
    SessionExpired, // New as of 7/30/2026
    SingleUploadFinished(String),
    SingleUploadFailed(String),
}
enum ImageState {
    Loading,
    Loaded(egui::TextureHandle), // The actual GPU texture egui uses
    Failed,
}

#[derive(PartialEq)]
enum ViewState {
    Login,
    Dashboard,
}
//--------------------------------------------------------------------------------
/// - This should replace all the other HTTP client instances...
/// - Part of the stupid memory leak update
/// - Use the pointer 'let client = HTTP_CLIENT.clone(); instead of those new calls'
static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("Failed to build the global HTTP client")
});

/// - The Main App State
/// - [x] this holds all the states. Simple!
/// each time it's called, It's probably getting a new state.
struct NasClientApp {
    view: ViewState,
    tx: Sender<AppMsg>,
    rx: Receiver<AppMsg>,

    // Login Data
    ip_input: String,
    password_input: String,

    // Dashboard Data
    token: String,
    current_path: String,
    files: Vec<FileInfo>,
    new_folder_name: String,

    // UI States
    status_message: String,
    is_loading: bool,

    moving_item: Option<String>,
    move_target_folder: String,

    item_pending_deletion: Option<String>, //new
    image_cache: std::collections::HashMap<String, ImageState>,

    upload_progress: Option<f32>,

    theme: AppTheme,
    active_theme_name: String,
    available_themes: HashMap<String, AppTheme>,

    selected_files: HashSet<String>,

    show_config_modal: bool,
    active_config: Option<AppConfig>,

    _tray_icon: Option<TrayIcon>, // warning is killed by the underscore. Good job team.
    tray_listener_spawned: bool,
    is_hidden: bool,
    locked_items: std::collections::HashSet<String>,
    show_batch_move_modal: bool,

    sort_mode: SortMode,

    upload_queue: VecDeque<QueuedUpload>,
    is_uploading: bool,
}
/// - Give Default values to the app to prevent bugs.
/// - Each new value for the NasClientApp struct needs a default!
impl Default for NasClientApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        // --- 1. BUILD THE TRAY ICON HERE ---
        let quit_i =
            tray_icon::menu::MenuItem::with_id("quit_solnas", "Quit the SolNAS Client", true, None);
        //now put in the icon
        let icon_bytes = include_bytes!("../Assets/QuitIcon.ico");
        let image = image::load_from_memory(icon_bytes)
            .expect("Failed to load tray icon")
            .into_rgba8();

        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        let tray_icon_image =
            tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to format tray icon");
        //old stuff
        let tray_menu = tray_icon::menu::Menu::new();
        let _ = tray_menu.append(&quit_i);

        let tray_icon_instance = tray_icon::TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_icon(tray_icon_image)
            .with_tooltip("SolNAS Client Quitter")
            .build()
            .unwrap();
        //now return the the defaults

        // Load the templates from the text file first
        let available_themes = network::load_theme_templates();

        // Pass them into your newly upgraded loader
        let (initial_theme_name, initial_theme) = network::load_theme(&available_themes);

        Self {
            theme: initial_theme,
            active_theme_name: initial_theme_name,
            available_themes, // Store the dictionary in your app state for the dropdown menu!
            view: ViewState::Login,
            tx,
            rx,
            ip_input: network::load_config(),
            password_input: String::new(),
            token: String::new(),
            current_path: String::new(),
            files: Vec::new(),
            new_folder_name: String::new(),
            status_message: String::new(),
            is_loading: false,
            moving_item: None,
            move_target_folder: String::new(),
            item_pending_deletion: None,
            image_cache: std::collections::HashMap::new(),
            upload_progress: None, //don't forget the defaults.
            selected_files: HashSet::new(),
            show_config_modal: false,
            active_config: None,
            _tray_icon: Some(tray_icon_instance),
            tray_listener_spawned: false,
            is_hidden: false,
            locked_items: network::load_protections(),
            show_batch_move_modal: false,
            sort_mode: SortMode::NameAsc,
            upload_queue: VecDeque::new(),
            is_uploading: false,
        }
    }
}

impl Default for AppTheme {
    fn default() -> Self {
        Self {
            background: egui::Color32::from_rgb(37, 37, 37), // Fallback dark gray
            text_primary: egui::Color32::WHITE,
            text_dashboard: egui::Color32::WHITE,
            text_title: egui::Color32::WHITE,
            list_txt: egui::Color32::WHITE,
            download_btn: egui::Color32::GRAY,
            upload_btn: egui::Color32::GRAY,
            delete_btn: egui::Color32::GRAY,
            move_btn: egui::Color32::GRAY,
            logout_btn: egui::Color32::GRAY,
            connect_btn: egui::Color32::GRAY,
            open_btn: egui::Color32::GRAY,
            refresh_btn: egui::Color32::GRAY,
            path_btn: egui::Color32::GRAY,
            folder_btn: egui::Color32::GRAY,
            theme_btn: egui::Color32::GRAY,
        }
    }
}

// The method to make the app pretty
// this implements the UI details for the dashboard and lock screen.
impl eframe::App for NasClientApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ==========================================
        // 1. BACKGROUND TRAY THREAD
        // ==========================================
        if !self.tray_listener_spawned {
            self.tray_listener_spawned = true;
            let tx_clone = self.tx.clone();
            let ctx_clone = ctx.clone();

            std::thread::spawn(move || {
                let tray_rx = tray_icon::TrayIconEvent::receiver();
                let menu_rx = tray_icon::menu::MenuEvent::receiver();

                loop {
                    // Right-Click Menu
                    while let Ok(event) = menu_rx.try_recv() {
                        if event.id.as_ref() == "quit_solnas" {
                            std::process::exit(0);
                        }
                    }

                    // Icon Clicks
                    while let Ok(event) = tray_rx.try_recv() {
                        match event {
                            tray_icon::TrayIconEvent::DoubleClick { .. }
                            | tray_icon::TrayIconEvent::Click {
                                button_state: tray_icon::MouseButtonState::Up,
                                ..
                            } => {
                                let _ = tx_clone.send(AppMsg::RestoreWindow);
                                ctx_clone.request_repaint(); // Jolt eframe awake
                            }
                            _ => {}
                        }
                    }

                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            });
        }

        // 2. INTERCEPT THE CLOSE BUTTON (THE FIX)

        #[cfg(not(target_os = "linux"))]
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        // 3. PROCESS BACKGROUND MESSAGES

        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                AppMsg::LoginSuccess(token) => {
                    self.is_loading = false;
                    self.token = token;
                    self.view = ViewState::Dashboard;
                    self.status_message = "Logged in!".to_string();
                    self.refresh_files(ctx);
                }
                AppMsg::LoginFailed(err) => {
                    self.is_loading = false;
                    self.status_message = err;
                }
                AppMsg::FilesLoaded(file_list) => {
                    self.is_loading = false;
                    self.files = file_list;
                    self.sort_files();
                }
                AppMsg::ActionSuccess(msg) => {
                    self.is_loading = false;
                    self.status_message = msg;
                    self.upload_progress = None;
                    self.refresh_files(ctx);
                }
                AppMsg::Error(err) => {
                    self.is_loading = false;
                    self.status_message = err;
                    self.upload_progress = None;
                }
                AppMsg::ImageLoaded(name, color_image) => {
                    let texture =
                        ctx.load_texture(&name, color_image, egui::TextureOptions::LINEAR);
                    self.image_cache.insert(name, ImageState::Loaded(texture));
                }
                AppMsg::ImageFailed(name) => {
                    self.image_cache.insert(name, ImageState::Failed);
                }
                AppMsg::UploadProgress(pct) => {
                    self.upload_progress = Some(pct);
                }
                AppMsg::ConfigLoaded(config) => {
                    self.is_loading = false;
                    self.active_config = Some(config);
                    self.show_config_modal = true;
                    self.status_message = "✅ Config received! Opening modal...".into();
                }
                AppMsg::ConfigSaved => {
                    self.is_loading = false;
                    self.show_config_modal = false;
                    self.status_message = "Server configuration updated successfully!".into();
                }

                // --- THE CLEAN WAKE-UP LOGIC ---
                AppMsg::RestoreWindow => {
                    //println!("successfully contacted the frontend!");
                    self.is_hidden = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                // -- The session booting the user out to login
                AppMsg::SessionExpired => {
                    self.is_loading = false;
                    self.token.clear(); // Nuke the dead token
                    self.files.clear(); // Wipe the directory list
                    self.image_cache.clear(); // Free up the RAM

                    // THIS IS THE FIX: Physically kick them back to the login screen
                    self.view = ViewState::Login;

                    self.status_message = "Session expired. Please log in again.".into();
                }
                AppMsg::SingleUploadFinished(name) => {
                    self.is_uploading = false;
                    self.upload_progress = None;
                    self.refresh_files(ctx);

                    if self.upload_queue.is_empty() {
                        self.status_message =
                            format!("Finished uploading '{}'. All uploads complete!", name);
                    } else {
                        self.process_next_upload(ctx);
                    }
                }
                AppMsg::SingleUploadFailed(err) => {
                    self.is_uploading = false;
                    self.upload_progress = None;
                    self.status_message = err;
                    // Move on to the next queued file instead of stalling the whole buffer
                    self.process_next_upload(ctx);
                }
            }
        }

        // ==========================================
        // 4. DRAW THE USER INTERFACE ALWAYS
        // ==========================================
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(15.0, 15.0);
        style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
        style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
        ctx.set_style(style);

        let custom_frame = egui::Frame::default()
            .fill(self.theme.background)
            .inner_margin(20.0);

        egui::CentralPanel::default()
            .frame(custom_frame)
            .show(ctx, |ui| match self.view {
                ViewState::Login => self.render_login(ui),
                ViewState::Dashboard => self.render_dashboard(ui, ctx),
            });
    }
}

// --- Main Entry ---
fn main() {
    // 1. Wake up GTK for the system tray (Linux only!)
    #[cfg(target_os = "linux")]
    {
        if let Err(err) = gtk::init() {
            eprintln!("Failed to initialize GTK: {}", err);
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 800.0])
            .with_title("SolNAS Client")
            .with_transparent(true)
            .with_resizable(false), //allow transparency
        ..Default::default()
    };

    let _ = eframe::run_native(
        "SolNAS Client",
        options,
        Box::new(|_cc| Box::new(NasClientApp::default()) as Box<dyn eframe::App>),
    );
}
//Now finally sectioned into separate parts. I know you wanted that.
