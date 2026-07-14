#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] //this gets rid of the terminal popup
//use
use eframe::egui;
use egui::Color32;

use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::Read,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

const CONFIG_FILE: &str = "config.ini";

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

#[derive(Deserialize, Clone)]
struct FileInfo {
    name: String,
    size: u64,
    is_dir: bool,
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
} //add onto these as you need to.

const STYLE_FILE: &str = "style.ini";
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

// --- The Main App State ---
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
    selected_files: HashSet<String>,

    show_config_modal: bool,
    active_config: Option<AppConfig>,
}
// -- Give Default values to the app to prevent bugs.
impl Default for NasClientApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            theme: load_theme(), // Load it exactly once when the app starts
            view: ViewState::Login,
            tx,
            rx,
            ip_input: load_config(),
            password_input: String::new(),
            token: String::new(),
            current_path: String::new(),
            files: Vec::new(),
            new_folder_name: String::new(),
            status_message: String::new(),
            is_loading: false,
            moving_item: None,
            move_target_folder: String::new(),
            item_pending_deletion: None, //new
            image_cache: std::collections::HashMap::new(),
            upload_progress: None, //don't forget the defaults.
            selected_files: HashSet::new(),
            show_config_modal: false,
            active_config: None,
        }
    }
}

/// --- Helper for HTTPS ---
/// ### This function does as it says. It builds a client and ignores the self signed certificates.
fn get_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
}
// The method to make the app pretty
impl eframe::App for NasClientApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Process any messages from background threads

        let mut style = (*ctx.style()).clone();

        style.spacing.item_spacing = egui::vec2(15.0, 15.0);
        style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
        style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);

        ctx.set_style(style); //actualy save the changes

        while let Ok(msg) = self.rx.try_recv() {
            self.is_loading = false;
            match msg {
                AppMsg::LoginSuccess(token) => {
                    self.token = token;
                    self.view = ViewState::Dashboard;
                    self.status_message = "Logged in!".to_string();
                    self.refresh_files(); // Fetch root folder immediately
                }
                AppMsg::LoginFailed(err) => self.status_message = err,
                AppMsg::FilesLoaded(file_list) => self.files = file_list,
                AppMsg::ActionSuccess(msg) => {
                    self.status_message = msg;
                    self.is_loading = false;

                    self.upload_progress = None; //this is new!

                    self.refresh_files();
                }
                AppMsg::Error(err) => {
                    self.status_message = err;
                    self.is_loading = false; //sets the loading state
                    self.upload_progress = None; //this kills the upload bar
                }
                AppMsg::ImageLoaded(name, color_image) => {
                    // Send the pixels to the graphics card
                    let texture = ctx.load_texture(
                        &name,
                        color_image,
                        egui::TextureOptions::LINEAR, // Makes scaled-down thumbnails look smooth
                    );
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
                    self.show_config_modal = true; // Open the window!
                    self.status_message = "✅ Config received! Opening modal...".into();
                }
                AppMsg::ConfigSaved => {
                    self.is_loading = false;
                    self.show_config_modal = false; // Close the window!
                    self.status_message = "Server configuration updated successfully!".into();
                }
            }
        }
        let custom_frame = egui::Frame::default()
            .fill(self.theme.background)
            .inner_margin(20.0);

        // Draw the screen
        egui::CentralPanel::default()
            .frame(custom_frame)
            .show(ctx, |ui| match self.view {
                ViewState::Login => self.render_login(ui),
                ViewState::Dashboard => self.render_dashboard(ui),
            });
    }
}

// basically implements the whole entire app. have fun.
impl NasClientApp {
    // ==========================================
    // UI RENDERING
    // ==========================================
    /// # Render Login
    ///
    ///  * This function puts each of the buttons and text on the login page.
    ///  * I tried to make it easy to edit if you want any changes, and it's easy to change things, but its very wordy, I know.
    ///
    /// ### Changing the look of the buttons
    ///
    /// > To change the look of the buttons, edit the "let" expressions.
    /// * It's simple: size is size of the button, colors are for the fill of the buttons and of the text inside the buttons.
    /// * Let the rust compiler tell you if you've done something wrong
    ///
    /// **As of the style update, all of the special colors can be edited from the style.ini file.**
    fn render_login(&mut self, ui: &mut egui::Ui) {
        let login_title = egui::RichText::new("SolNAS Login")
            .color(self.theme.text_title)
            .size(24.0); //start replacing

        let connect_button_raw = egui::RichText::new("Connect")
            .color(self.theme.text_primary)
            .size(20.0);
        let connect_button = egui::Button::new(connect_button_raw).fill(self.theme.connect_btn);

        ui.vertical_centered(|ui| {
            ui.add_space(50.0);
            ui.heading(login_title);
            ui.add_space(20.0);
        });

        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("NAS IP Address:").strong().size(24.0));

            ui.add(
                egui::TextEdit::singleline(&mut self.ip_input) // change the size of the text bar
                    .font(egui::FontId::proportional(24.0)),
            );
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Password:").strong().size(24.0));

            ui.add(
                egui::TextEdit::singleline(&mut self.password_input) // change the size of the text bar
                    .font(egui::FontId::proportional(24.0)),
            );
        });

        ui.add_space(20.0);

        ui.vertical_centered(|ui| {
            if self.is_loading {
                ui.spinner();
            } else {
                let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let button_clicked = ui.add(connect_button).clicked();

                // 3. Trigger the logic if EITHER action happened
                if button_clicked || enter_pressed {
                    self.is_loading = true;
                    save_config(self.ip_input.trim());
                    let ip = self.ip_input.trim().to_string();
                    let pwd = self.password_input.clone();
                    let tx = self.tx.clone();

                    thread::spawn(move || {
                        let client = get_client();
                        let url = format!("https://{}:8080/api/auth", ip);
                        let payload = AuthRequest { password: pwd };

                        match client.post(&url).json(&payload).send() {
                            Ok(res) => {
                                if res.status().is_success() {
                                    if let Ok(data) = res.json::<AuthResponse>() {
                                        tx.send(AppMsg::LoginSuccess(
                                            data.token.unwrap_or_default(),
                                        ))
                                        .unwrap();
                                    }
                                } else {
                                    tx.send(AppMsg::LoginFailed("Invalid password".to_string()))
                                        .unwrap();
                                }
                            }
                            Err(_) => tx
                                .send(AppMsg::LoginFailed(
                                    "Network error ( Is the server running? )".to_string(),
                                ))
                                .unwrap(),
                        }
                    });
                }
            }
        });

        if !self.status_message.is_empty() {
            ui.add_space(10.0);
            ui.label(egui::RichText::new(&self.status_message).color(egui::Color32::BLACK));
        }
    }

    fn render_dashboard(&mut self, ui: &mut egui::Ui) {
        let back_button_raw = egui::RichText::new("⬅ Path Back")
            .color(self.theme.text_dashboard)
            .size(20.0);
        let back_button = egui::Button::new(back_button_raw).fill(self.theme.path_btn);

        let upload_button_raw = egui::RichText::new("📤 Upload File")
            .color(self.theme.text_dashboard)
            .size(20.0);
        let upload_button = egui::Button::new(upload_button_raw).fill(self.theme.upload_btn);

        let folder_make_button_raw = egui::RichText::new("📁 Create Folder")
            .color(self.theme.text_dashboard)
            .size(20.0);
        let folder_make_button =
            egui::Button::new(folder_make_button_raw).fill(self.theme.folder_btn);

        let refresh_raw = egui::RichText::new("🔄 Refresh")
            .color(self.theme.text_dashboard)
            .size(16.0);
        let refresh_button = egui::Button::new(refresh_raw).fill(self.theme.folder_btn);

        let logout_raw = egui::RichText::new("Log Out")
            .color(self.theme.text_dashboard)
            .size(14.0);
        let logout_button = egui::Button::new(logout_raw).fill(self.theme.logout_btn);
        // Top Navigation Bar
        ui.horizontal(|ui| {
            if ui.add(logout_button).clicked() {
                self.token.clear();
                self.view = ViewState::Login;
            }
            ui.separator();
            if ui.button("⚙ Server Config").clicked() {
                self.fetch_remote_config();
            }
            if !self.current_path.is_empty() {
                if ui.add(back_button).clicked() {
                    let mut parts: Vec<&str> = self.current_path.split('/').collect();
                    parts.pop();
                    self.current_path = parts.join("/");
                    self.refresh_files();
                }
            }
            ui.label(egui::RichText::new(format!("/{}", self.current_path)).strong());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(refresh_button).clicked() {
                    self.refresh_files();
                }
            });
        });

        ui.separator();

        // Toolbar (Upload & Create Folder)
        ui.horizontal(|ui| {
            if ui.add(upload_button).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_files() {
                    self.upload_files(path);
                }
            }

            ui.separator();

            ui.add(
                egui::TextEdit::singleline(&mut self.new_folder_name)
                    .hint_text("New folder name...")
                    .text_color(Color32::WHITE)
                    .font(egui::FontId::proportional(24.0)),
            );
            if ui.add(folder_make_button).clicked() {
                if !self.new_folder_name.is_empty() {
                    self.create_folder(self.new_folder_name.clone());
                    self.new_folder_name.clear();
                }
            }
        });

        if let Some(progress) = self.upload_progress {
            ui.add_space(5.0);

            ui.add(
                egui::ProgressBar::new(progress)
                    .show_percentage()
                    .animate(true),
            );
            ui.add_space(5.0);
        }
        ui.separator();

        // System Status / Errors
        if !self.status_message.is_empty() {
            ui.label(egui::RichText::new(&self.status_message).color(self.theme.text_dashboard));
            ui.separator();
        }

        if self.is_loading {
            ui.spinner();
        }
        //All Brand New Stuff 7/6/2026
        if !self.selected_files.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} files selected", self.selected_files.len()))
                        .strong(),
                );

                if ui.button("⬇ Download All").clicked() {
                    // Ask the user for a FOLDER, not a file!
                    if let Some(save_folder) = rfd::FileDialog::new().pick_folder() {
                        // Convert the HashSet into a Vector so we can pass it to our background thread
                        let files_to_download: Vec<String> =
                            self.selected_files.clone().into_iter().collect();

                        // Fire off the new batch download function
                        self.download_multiple_files(files_to_download, save_folder);

                        // Clear the checkboxes immediately so the UI resets
                        self.selected_files.clear();
                    }
                }

                if ui.button("Clear Selection").clicked() {
                    self.selected_files.clear();
                }
            });
            ui.separator();
        }

        // File List Area
        egui::ScrollArea::vertical().show(ui, |ui| {
            for file in self.files.clone() {
                ui.horizontal(|ui| {
                    if file.is_dir {
                        ui.label(egui::RichText::new("📁").font(egui::FontId::proportional(24.0)));

                        ui.label(
                            egui::RichText::new(&file.name)
                                .strong()
                                .font(egui::FontId::proportional(24.0)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let folder_delete_button_raw = egui::RichText::new("🗑 delete")
                                .color(self.theme.text_primary)
                                .size(15.0);
                            let folder_delete_button = egui::Button::new(folder_delete_button_raw)
                                .fill(self.theme.delete_btn);

                            let folder_open_button_raw = egui::RichText::new("Open Folder")
                                .color(self.theme.text_primary)
                                .size(24.0);
                            let folder_open_button =
                                egui::Button::new(folder_open_button_raw).fill(self.theme.open_btn);
                            let folder_move_button_raw = egui::RichText::new("Move")
                                .color(self.theme.text_primary)
                                .size(24.0);
                            let folder_move_button =
                                egui::Button::new(folder_move_button_raw).fill(self.theme.move_btn);

                            if ui.add(folder_delete_button).clicked() {
                                //improved delete logic

                                if self.item_pending_deletion.as_deref() == Some(&file.name) {
                                    // SECOND CLICK: Execute the actual delete function
                                    self.delete_item(&file.name);

                                    // Clear the state and the message
                                    self.item_pending_deletion = None;
                                    self.status_message = String::new();
                                } else {
                                    // FIRST CLICK: Queue it up and show the system message
                                    self.item_pending_deletion = Some(file.name.clone());
                                    self.status_message = format!(
                                        "Delete '{}'? Click 🗑 again to confirm.",
                                        &file.name
                                    );
                                }
                            }

                            ui.add_space(20.0);
                            if ui.add(folder_open_button).clicked() {
                                if self.current_path.is_empty() {
                                    self.current_path = file.name.clone();
                                } else {
                                    self.current_path =
                                        format!("{}/{}", self.current_path, file.name);
                                }
                                self.refresh_files();
                            }
                            ui.add_space(20.0);
                            if ui.add(folder_move_button).clicked() {
                                self.moving_item = Some(file.name.clone());
                            }
                        });
                    } else {
                        let mut is_selected = self.selected_files.contains(&file.name);
                        if ui.checkbox(&mut is_selected, "").clicked() {
                            if is_selected {
                                self.selected_files.insert(file.name.clone());
                            } else {
                                self.selected_files.remove(&file.name);
                            }
                        }
                        //get a preview of the pic
                        let lower_name = file.name.to_lowercase();
                        let is_image = lower_name.ends_with(".png")
                            || lower_name.ends_with(".jpg")
                            || lower_name.ends_with(".jpeg");

                        if is_image {
                            // Check our cache
                            match self.image_cache.get(&file.name) {
                                Some(ImageState::Loaded(texture)) => {
                                    // Draw the thumbnail! (Restricted to 32x32 pixels, with slightly rounded corners)
                                    ui.add(
                                        egui::Image::new(texture)
                                            .max_width(64.0)
                                            .max_height(64.0)
                                            .rounding(4.0),
                                    );
                                }
                                Some(ImageState::Loading) => {
                                    ui.spinner(); // Show a loading wheel while it downloads
                                }
                                Some(ImageState::Failed) => {
                                    ui.label("📄"); // Fallback to emoji if it broke
                                }
                                None => {
                                    // We haven't asked for this image yet! Queue it up and show a spinner for now.
                                    self.fetch_preview(file.name.clone());
                                    ui.spinner();
                                }
                            }
                        } else {
                            // Standard file icon for PDFs, TXTs, etc.
                            ui.label(
                                egui::RichText::new("📄").font(egui::FontId::proportional(24.0)),
                            );
                        }

                        ui.label(
                            egui::RichText::new(&file.name)
                                .strong()
                                .font(egui::FontId::proportional(24.0)),
                        );

                        let mb = file.size as f64 / 1_048_576.0;
                        ui.label(format!("({:.2} MB)", mb));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let file_delete_button_raw = egui::RichText::new("🗑 delete")
                                .color(self.theme.text_primary)
                                .size(15.0);
                            let file_delete_button = egui::Button::new(file_delete_button_raw)
                                .fill(self.theme.delete_btn);

                            let file_move_raw = egui::RichText::new("Move")
                                .color(self.theme.text_primary)
                                .size(24.0);
                            let file_move_button =
                                egui::Button::new(file_move_raw).fill(self.theme.move_btn);

                            let file_download_raw = egui::RichText::new("⬇ Download")
                                .color(self.theme.text_primary)
                                .size(24.0);
                            let file_download_button =
                                egui::Button::new(file_download_raw).fill(self.theme.download_btn);

                            if ui.add(file_delete_button).clicked() {
                                //file deletion improved logic
                                if self.item_pending_deletion.as_deref() == Some(&file.name) {
                                    // SECOND CLICK: Execute the actual delete function
                                    self.delete_item(&file.name);

                                    // Clear the state and the message
                                    self.item_pending_deletion = None;
                                    self.status_message = String::new();
                                } else {
                                    // FIRST CLICK: Queue it up and show the system message
                                    self.item_pending_deletion = Some(file.name.clone());
                                    self.status_message = format!(
                                        "Delete '{}'? Click 🗑 again to confirm.",
                                        &file.name
                                    );
                                }
                            }
                            ui.add_space(20.0);
                            if ui.add(file_download_button).clicked() {
                                // Prompt user for where to save the file!
                                if let Some(save_path) =
                                    rfd::FileDialog::new().set_file_name(&file.name).save_file()
                                {
                                    self.download_file(&file.name, save_path);
                                }
                            }
                            // For Files:
                            if ui.add(file_move_button).clicked() {
                                self.moving_item = Some(file.name.clone());
                            }
                        });
                    }
                });
                ui.separator();
            }
        });
        if let Some(item_name) = self.moving_item.clone() {
            egui::Window::new("Move Item")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Moving: {}", item_name));
                    ui.add_space(10.0);

                    ui.label("Select destination:");

                    // --- THE DROPDOWN MENU ---
                    egui::ComboBox::from_id_source("move_dropdown")
                        .selected_text(if self.move_target_folder.is_empty() {
                            "Root (/)".to_string()
                        } else {
                            format!("/{}", self.move_target_folder)
                        })
                        .width(250.0)
                        .show_ui(ui, |ui| {
                            // 1. Always offer the Root folder
                            ui.selectable_value(
                                &mut self.move_target_folder,
                                String::new(),
                                "Root (/)",
                            );

                            // 2. Offer the Parent folder (if we are currently inside a folder)
                            if !self.current_path.is_empty() {
                                let mut parts: Vec<&str> = self.current_path.split('/').collect();
                                parts.pop(); // Go up one level
                                let parent_path = parts.join("/");

                                let display_name = if parent_path.is_empty() {
                                    "Root (/)".to_string()
                                } else {
                                    format!("/{}", parent_path)
                                };

                                ui.selectable_value(
                                    &mut self.move_target_folder,
                                    parent_path,
                                    format!("⬆ Parent ({})", display_name),
                                );
                            }

                            // 3. Offer any Subfolders visible on the current screen
                            for f in &self.files {
                                if f.is_dir && f.name != item_name {
                                    let target_path = if self.current_path.is_empty() {
                                        f.name.clone()
                                    } else {
                                        format!("{}/{}", self.current_path, f.name)
                                    };
                                    ui.selectable_value(
                                        &mut self.move_target_folder,
                                        target_path.clone(),
                                        format!("📁 /{}", target_path),
                                    );
                                }
                            }
                        });

                    ui.add_space(15.0);

                    // --- MOVED INSIDE THE WINDOW CLOSURE ---
                    ui.horizontal(|ui| {
                        if ui.button("Confirm Move").clicked() {
                            let source = if self.current_path.is_empty() {
                                item_name.clone()
                            } else {
                                format!("{}/{}", self.current_path, item_name)
                            };

                            self.move_item(source, self.move_target_folder.clone(), item_name);

                            self.moving_item = None;
                            self.move_target_folder.clear();
                        }

                        if ui.button("Cancel").clicked() {
                            self.moving_item = None;
                            self.move_target_folder.clear();
                        }
                    });
                });
        }

        // ==========================================
        // 2. THE SERVER CONFIG MODAL (Completely independent)
        // ==========================================
        if self.show_config_modal {
            // 1. Create temporary flags outside the closure
            let mut trigger_save = false;
            let mut close_modal = false;

            egui::Window::new("⚙ Server Configuration")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    // 2. We do the mutable borrow INSIDE the window now
                    if let Some(config) = &mut self.active_config {
                        ui.label("Max Upload Size:");

                        // 1. Convert the backend's bytes into a temporary Megabyte decimal
                        let mut size_in_mb = config.max_upload_size as f64 / 1_048_576.0;

                        // 2. Draw the DragValue using the MB variable
                        // We also add a visual " MB" suffix so it looks professional!
                        if ui
                            .add(
                                egui::DragValue::new(&mut size_in_mb)
                                    .speed(100.0) // Dragging moves it by 1 MB at a time
                                    .suffix(" MB"),
                            )
                            .changed()
                        {
                            // 3. If the user edits the number, convert it back to bytes immediately!
                            config.max_upload_size = (size_in_mb * 1_048_576.0) as u64;
                        }

                        ui.add_space(10.0);

                        ui.label("Upload Speed Limit (Bytes per second): (0 = Unlimited)");
                        ui.add(egui::DragValue::new(&mut config.upload_speed_bps).speed(1024.0));

                        ui.add_space(20.0);

                        ui.horizontal(|ui| {
                            // 3. We ONLY flip the booleans inside the closure. No `self` calls here!
                            if ui.button("Save & Apply").clicked() {
                                trigger_save = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_modal = true;
                            }
                        });
                    }
                });

            // 4. Safely outside the closure, the borrow is dropped, so we can freely use `self` again!
            if trigger_save {
                if let Some(config) = self.active_config.clone() {
                    self.save_remote_config(config);
                }
            }

            if close_modal {
                self.show_config_modal = false;
            }
        }
    }
    // ==========================================
    // API NETWORK COMMANDS
    // ==========================================

    fn refresh_files(&mut self) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let path = self.current_path.clone();

        thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/files?path={}", ip, path);
            match client.get(&url).bearer_auth(token).send() {
                Ok(res) => {
                    if let Ok(data) = res.json::<ListResponse>() {
                        tx.send(AppMsg::FilesLoaded(data.files)).unwrap();
                    }
                }
                Err(_) => tx
                    .send(AppMsg::Error("Failed to fetch files".into()))
                    .unwrap(),
            }
        });
    }

    fn upload_files(&mut self, file_paths: Vec<PathBuf>) {
        self.is_loading = true;
        self.upload_progress = Some(0.0);
        self.status_message = format!("Uploading {} file(s)...", file_paths.len());

        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let current_path = self.current_path.clone();

        thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/upload_chunk", ip);

            for file_path in file_paths {
                let filename = file_path.file_name().unwrap().to_str().unwrap().to_string();
                let target_name = if current_path.is_empty() {
                    filename
                } else {
                    format!("{}/{}", current_path, filename)
                };

                let mut file = match std::fs::File::open(&file_path) {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = tx.send(AppMsg::Error(format!("Could not read local file: {}", e)));
                        continue;
                    }
                };

                let file_size = file.metadata().unwrap().len();

                const CHUNK_SIZE: u64 = 100 * 1024 * 1024; // 100 Megabytes per chunk

                //the edge case: Uploading a completely empty file (0 bytes)
                let total_chunks = if file_size == 0 {
                    1
                } else {
                    (file_size as f64 / CHUNK_SIZE as f64).ceil() as u64
                };

                for chunk_index in 0..total_chunks {
                    // Determine how big this specific chunk should be (the last chunk is usually smaller than 10MB)
                    let current_chunk_size =
                        std::cmp::min(CHUNK_SIZE, file_size - (chunk_index * CHUNK_SIZE));
                    let offset = chunk_index * CHUNK_SIZE;

                    let mut buffer = vec![0; current_chunk_size as usize];

                    if let Err(e) = file.read_exact(&mut buffer) {
                        let _ =
                            tx.send(AppMsg::Error(format!("Failed to read local chunk: {}", e)));
                        break;
                    }

                    // RETRY LOOP: Try to send the chunk up to 3 times if the network drops
                    let mut attempts = 0;
                    let mut success = false;

                    while attempts < 3 && !success {
                        // We must rebuild the form for every attempt because reqwest consumes it
                        let part = reqwest::blocking::multipart::Part::bytes(buffer.clone())
                            .file_name(target_name.clone());

                        let form = reqwest::blocking::multipart::Form::new()
                            .text("filename", target_name.clone())
                            .text("chunk_index", chunk_index.to_string())
                            .text("total_chunks", total_chunks.to_string())
                            .text("offset", offset.to_string())
                            .part("file", part);

                        match client.post(&url).bearer_auth(&token).multipart(form).send() {
                            Ok(res) if res.status().is_success() => success = true,
                            _ => {
                                attempts += 1;
                                std::thread::sleep(std::time::Duration::from_secs(2)); // Wait 2s before retry
                            }
                        }
                    }

                    if !success {
                        let _ = tx.send(AppMsg::Error(format!(
                            "Upload failed after 3 retries: {}",
                            target_name
                        )));
                        return; // Completely abort the thread
                    }

                    // Calculate total percentage and update UI!
                    let percentage = (chunk_index as f32 + 1.0) / total_chunks as f32;
                    let _ = tx.send(AppMsg::UploadProgress(percentage));
                }
            }

            // All files finished!
            let _ = tx.send(AppMsg::ActionSuccess("Upload complete!".into()));
        });
        self.upload_progress = Some(0.0);
    }

    fn download_file(&mut self, filename: &str, save_path: PathBuf) {
        self.is_loading = true;
        self.status_message = "Downloading...".into();
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let full_remote_path = if self.current_path.is_empty() {
            filename.to_string()
        } else {
            format!("{}/{}", self.current_path, filename)
        };

        thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/download/{}", ip, full_remote_path);

            match client.get(&url).bearer_auth(token).send() {
                Ok(mut res) => {
                    if let Ok(mut file) = std::fs::File::create(save_path) {
                        res.copy_to(&mut file).unwrap();
                        tx.send(AppMsg::ActionSuccess("Download complete!".into()))
                            .unwrap();
                    }
                }
                Err(_) => tx.send(AppMsg::Error("Download failed".into())).unwrap(),
            }
        });
    }
    ///## Fetch Preview
    ///
    /// - A cute little function that gives a preview of the picture
    /// - probably won't work for odd picture formats or Gifs.
    ///
    fn fetch_preview(&mut self, filename: String) {
        // Mark it as loading so we don't spawn 100 threads for the same image
        self.image_cache
            .insert(filename.clone(), ImageState::Loading);

        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();

        let full_path = if self.current_path.is_empty() {
            filename.clone()
        } else {
            format!("{}/{}", self.current_path, filename)
        };

        thread::spawn(move || {
            let client = get_client();
            let url = format!(
                "https://{}:8080/api/download/{}?preview=true",
                ip, full_path
            );

            if let Ok(res) = client.get(&url).bearer_auth(token).send() {
                if let Ok(bytes) = res.bytes() {
                    // Decode the raw web bytes into a dynamic image
                    if let Ok(img) = image::load_from_memory(&bytes) {
                        let size = [img.width() as _, img.height() as _];
                        let image_buffer = img.to_rgba8();
                        let pixels = image_buffer.as_flat_samples();

                        // Convert to egui's specific color format
                        let color_image =
                            egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());

                        tx.send(AppMsg::ImageLoaded(filename, color_image)).unwrap();
                        return;
                    }
                }
            }
            // If anything fails (network error, bad image file), mark it as failed
            tx.send(AppMsg::ImageFailed(filename)).unwrap();
        });
    }
    fn create_folder(&mut self, folder_name: String) {
        if self
            .files
            .iter()
            .any(|f| f.is_dir && f.name.eq_ignore_ascii_case(&folder_name))
        {
            // Abort immediately and display the cute system message!
            self.status_message =
                format!("Cannot create folder: '{}' already exists ☆☆☆", folder_name);
            return;
        } //Prevent the issue that Jakob talked about.
        self.is_loading = true;
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let full_path = if self.current_path.is_empty() {
            folder_name
        } else {
            format!("{}/{}", self.current_path, folder_name)
        };

        thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/folders", ip);
            let payload = serde_json::json!({ "path": full_path });

            if client
                .post(&url)
                .bearer_auth(token)
                .json(&payload)
                .send()
                .is_ok()
            {
                tx.send(AppMsg::ActionSuccess("Folder created".into()))
                    .unwrap();
            }
        });
    }

    fn move_item(&mut self, source_path: String, destination_folder: String, file_name: String) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();

        thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/move", ip);

            // Construct the final destination path
            let destination_path = if destination_folder.is_empty() {
                file_name // Moving it to the root FileStorage
            } else {
                format!("{}/{}", destination_folder, file_name)
            };

            let payload = MoveRequest {
                source_path,
                destination_path,
            };

            match client.post(&url).bearer_auth(token).json(&payload).send() {
                Ok(res) if res.status().is_success() => {
                    tx.send(AppMsg::ActionSuccess("Item moved successfully!".into()))
                        .unwrap();
                }
                Ok(_) => tx
                    .send(AppMsg::Error("Failed to move item.".into()))
                    .unwrap(),
                Err(e) => tx
                    .send(AppMsg::Error(format!("Network error: {}", e)))
                    .unwrap(),
            }
        });
    }

    fn delete_item(&mut self, item_name: &str) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let full_path = if self.current_path.is_empty() {
            item_name.to_string()
        } else {
            format!("{}/{}", self.current_path, item_name)
        };

        thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/delete", ip);
            let payload = serde_json::json!({ "path": full_path });

            if client
                .post(&url)
                .bearer_auth(token)
                .json(&payload)
                .send()
                .is_ok()
            {
                tx.send(AppMsg::ActionSuccess("Deleted successfully".into()))
                    .unwrap();
            }
        });
    }
    // New Stuff 7/6/2026
    fn download_multiple_files(&mut self, files: Vec<String>, save_folder: std::path::PathBuf) {
        self.is_loading = true;
        self.status_message = format!("Downloading {} files...", files.len());

        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let current_path = self.current_path.clone();

        std::thread::spawn(move || {
            let client = get_client();
            //let total_files = files.len();
            //total_files and index are only useful for tracking downloads. This doesn't exist yet.
            for (_index, filename) in files.iter().enumerate() {
                // Determine the correct server path
                let full_path = if current_path.is_empty() {
                    filename.clone()
                } else {
                    format!("{}/{}", current_path, filename)
                };

                let url = format!("https://{}:8080/api/download/{}", ip, full_path);

                // Construct the exact local file path (e.g., C:\Users\You\Downloads\report.pdf)
                let target_file_path = save_folder.join(filename);

                // Fetch the file from the server
                if let Ok(mut res) = client.get(&url).bearer_auth(&token).send() {
                    if res.status().is_success() {
                        // Create the local file and write the stream into it
                        if let Ok(mut local_file) = std::fs::File::create(&target_file_path) {
                            let _ = std::io::copy(&mut res, &mut local_file);
                        }
                    }
                }
            }

            // All files finished!
            let _ = tx.send(AppMsg::ActionSuccess("Batch download complete!".into()));
        });
    }
    fn fetch_remote_config(&mut self) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();

        std::thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/config", ip);

            // We use 'match' here to capture the exact error if it fails
            match client.get(&url).bearer_auth(token).send() {
                Ok(res) => {
                    let status = res.status();

                    match res.text() {
                        Ok(raw_text) => {
                            if status.is_success() {
                                // Try to parse the JSON
                                match serde_json::from_str::<AppConfig>(&raw_text) {
                                    Ok(config) => {
                                        let _ = tx.send(AppMsg::ConfigLoaded(config));
                                    }
                                    Err(e) => {
                                        // Tell the UI that the JSON didn't match our struct
                                        let _ = tx.send(AppMsg::Error(format!(
                                            "JSON Error: {}. Raw data: {}",
                                            e, raw_text
                                        )));
                                    }
                                }
                            } else {
                                // Tell the UI that the server rejected the request (e.g. 404, 401, 500)
                                let _ = tx.send(AppMsg::Error(format!(
                                    "Server Error {}: {}",
                                    status, raw_text
                                )));
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(AppMsg::Error(format!(
                                "Failed to read server response: {}",
                                e
                            )));
                        }
                    }
                }
                Err(e) => {
                    // Tell the UI that we couldn't even reach the server
                    let _ = tx.send(AppMsg::Error(format!("Network Error: {}", e)));
                }
            }
        });
    }

    fn save_remote_config(&mut self, new_config: AppConfig) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();

        std::thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/config", ip);

            if client
                .post(&url)
                .bearer_auth(token)
                .json(&new_config)
                .send()
                .is_ok()
            {
                let _ = tx.send(AppMsg::ConfigSaved);
            } else {
                let _ = tx.send(AppMsg::Error("Failed to save server configuration.".into()));
            }
        });
    }
}

// --- Config Helpers ---
fn load_config() -> String {
    std::fs::read_to_string(CONFIG_FILE)
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix("NAS_IP=").map(|s| s.trim().to_string()))
        .unwrap_or_default()
}

fn save_config(ip: &str) {
    let _ = std::fs::write(CONFIG_FILE, format!("NAS_IP={}\n", ip));
}

// The Style Update
///# Parse Hex
///The Hex Parser for the Color
fn parse_hex(hex: &str, fallback: egui::Color32) -> egui::Color32 {
    let clean_hex = hex.trim();
    egui::Color32::from_hex(clean_hex).unwrap_or(fallback)
}

fn load_theme() -> AppTheme {
    // Define your hardcoded default fallback colors here
    let mut theme = AppTheme {
        background: egui::Color32::from_hex("#250444").unwrap(), // Dark Navy
        text_primary: egui::Color32::BLACK,
        text_dashboard: egui::Color32::WHITE,
        text_title: egui::Color32::from_hex("#FFFFFF").unwrap(),
        download_btn: egui::Color32::from_hex("#28a792").unwrap(),
        upload_btn: egui::Color32::from_hex("#ac5ddc").unwrap(),
        delete_btn: egui::Color32::from_hex("#dc3545").unwrap(),
        move_btn: egui::Color32::from_hex("#f07d12").unwrap(),
        logout_btn: egui::Color32::from_hex("#ac5ddc").unwrap(),
        connect_btn: egui::Color32::from_hex("#fcba00").unwrap(),
        open_btn: egui::Color32::from_hex("#ac5ddc").unwrap(),
        refresh_btn: egui::Color32::from_hex("#ac5ddc").unwrap(),
        path_btn: egui::Color32::from_hex("#5ddcab").unwrap(),
        folder_btn: egui::Color32::from_hex("#ac5ddc").unwrap(),
    };

    if !std::path::Path::new(STYLE_FILE).exists() {
        println!("Style file not found. Creating {}...", STYLE_FILE);
        let default_ini = "[Colors]\n\
                           Background = #250444\n\
                           Text_Primary = #000000\n\
                           Text_dashboard = #FFFFFF\n\
                           text_title =     #FFFFFF\n\
                           Download_Button = #28a792\n\
                           upload_btn = #ac5ddc\n\
                           Delete_Button = #dc3545\n\
                           move_btn = #f07d12\n\
                           logout_btn = #ac5ddc\n\
                           connect_btn = #fcba00\n\
                           open_btn = #ac5ddc\n\
                           refresh_btn = #ac5ddc\n\
                           path_btn = #5ddcab\n\
                           folder_button = #ac5ddc";
        let _ = std::fs::write(STYLE_FILE, default_ini);
        return theme; // Return the defaults since we just created the file
    }

    // Read the file and parse the custom colors
    if let Ok(content) = std::fs::read_to_string(STYLE_FILE) {
        for line in content.lines() {
            if line.trim().starts_with('#') || line.trim().starts_with('[') {
                continue; // Skip comments and section headers
            }

            let parts: Vec<&str> = line.split('=').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_lowercase();
                let hex_val = parts[1].trim();

                match key.as_str() {
                    "background" => theme.background = parse_hex(hex_val, theme.background),
                    "text_primary" => theme.text_primary = parse_hex(hex_val, theme.text_primary),
                    "text_dashboard" => {
                        theme.text_dashboard = parse_hex(hex_val, theme.text_dashboard)
                    }
                    "text_title" => theme.text_title = parse_hex(hex_val, theme.text_title),
                    "download_button" => {
                        theme.download_btn = parse_hex(hex_val, theme.download_btn)
                    }
                    "upload_button" => theme.upload_btn = parse_hex(hex_val, theme.upload_btn),
                    "delete_button" => theme.delete_btn = parse_hex(hex_val, theme.delete_btn),
                    "move_button" => theme.move_btn = parse_hex(hex_val, theme.move_btn),
                    "logout_button" => theme.logout_btn = parse_hex(hex_val, theme.logout_btn),
                    "connect_button" => theme.connect_btn = parse_hex(hex_val, theme.connect_btn),
                    "open_button" => theme.open_btn = parse_hex(hex_val, theme.open_btn),
                    "refresh_button" => theme.refresh_btn = parse_hex(hex_val, theme.refresh_btn),
                    "path_button" => theme.path_btn = parse_hex(hex_val, theme.path_btn),
                    _ => {} // Ignore unknown keys
                }
            }
        }
    }

    theme
}

// --- Main Entry ---
fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 900.0])
            .with_title("SolNAS Client App")
            .with_transparent(true), //allow transparency
        ..Default::default()
    };

    let _ = eframe::run_native(
        "SolNAS Client",
        options,
        Box::new(|_cc| Box::new(NasClientApp::default()) as Box<dyn eframe::App>),
    );
}
