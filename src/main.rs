#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] //this gets rid of the terminal popup
//use 
use eframe::egui;
use egui::Color32;
use serde::{Deserialize, Serialize};
use std::{thread, path::PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

const CONFIG_FILE: &str = "config.ini";

// --- Data Structures ---
#[derive(Serialize)]
struct AuthRequest { password: String }

#[derive(Deserialize)]
struct AuthResponse { token: Option<String>}

#[derive(Deserialize, Clone)]
struct FileInfo { name: String, size: u64, is_dir: bool }

#[derive(Deserialize)]
struct ListResponse { files: Vec<FileInfo> }

#[derive(Serialize)]
struct MoveRequest {
    source_path: String,
    destination_path: String,
}

// --- Background Worker Messages ---
enum AppMsg {
    LoginSuccess(String),
    LoginFailed(String),
    FilesLoaded(Vec<FileInfo>),
    ActionSuccess(String), // Used for upload/delete/create success
    Error(String),

    ImageLoaded(String, egui::ColorImage), 
    ImageFailed(String),
}
enum ImageState {
    Loading,
    Loaded(egui::TextureHandle), // The actual GPU texture egui uses
    Failed,
}


#[derive(PartialEq)]
enum ViewState { Login, Dashboard }

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
}
// -- Give Default values to the app to prevent bugs.
impl Default for NasClientApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            view: ViewState::Login,
            tx, rx,
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
                    self.refresh_files(); // Auto-refresh the folder view!
                }
                AppMsg::Error(err) => self.status_message = err,
                AppMsg::ImageLoaded(name, color_image) => {
                    // Send the pixels to the graphics card
                    let texture = ctx.load_texture(
                        &name, 
                        color_image, 
                        egui::TextureOptions::LINEAR // Makes scaled-down thumbnails look smooth
                    );
                    self.image_cache.insert(name, ImageState::Loaded(texture));
                }
                AppMsg::ImageFailed(name) => {
                    self.image_cache.insert(name, ImageState::Failed);
                }

            }
        }
        let custom_frame = egui::Frame::default()
        .fill(egui::Color32::from_hex("#1e052e").unwrap()) 
        .inner_margin(20.0);

        // Draw the screen
        egui::CentralPanel::default().frame(custom_frame).show(ctx, |ui| {
            match self.view {
                ViewState::Login => self.render_login(ui),
                ViewState::Dashboard => self.render_dashboard(ui),
            }
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
    fn render_login(&mut self, ui: &mut egui::Ui) {

        let login_title = egui::RichText::new("SolNAS Login").color(egui::Color32::from_hex("#ac5ddc").unwrap()).size(24.0);

        let connect_button_raw = egui::RichText::new("Connect").color(egui::Color32::BLACK).size(20.0);
        let connect_button = egui::Button::new(connect_button_raw).fill(egui::Color32::from_hex("#ac5ddc").unwrap());

        ui.vertical_centered(|ui| {
            ui.add_space(50.0);
            ui.heading(login_title);
            ui.add_space(20.0);
        });

        ui.vertical_centered(|ui| {

            ui.label(egui::RichText::new("NAS IP Address:").strong().size(24.0));

            ui.add(egui::TextEdit::singleline(&mut self.ip_input) // change the size of the text bar
            .font(egui::FontId::proportional(24.0))
        );
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Password:").strong().size(24.0)); 

            ui.add(egui::TextEdit::singleline(&mut self.password_input) // change the size of the text bar
            .font(egui::FontId::proportional(24.0))
        );
        }
    );

        ui.add_space(20.0);

        

        ui.vertical_centered(|ui| {
            if self.is_loading {
                ui.spinner();
            } else{
        
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
                                tx.send(AppMsg::LoginSuccess(data.token.unwrap_or_default())).unwrap();
                            }
                        } else {
                            tx.send(AppMsg::LoginFailed("Invalid password".to_string())).unwrap();
                        }
                    }
                    Err(_) => tx.send(AppMsg::LoginFailed("Network error ( Is the server running? )".to_string())).unwrap(),
                }
            });
        }
    }
});

        if !self.status_message.is_empty() {
            ui.add_space(10.0);
            ui.label(egui::RichText::new(&self.status_message).color(egui::Color32::BLUE));
        }
    }

    fn render_dashboard(&mut self, ui: &mut egui::Ui) {

        let back_button_raw = egui::RichText::new("⬅ Path Back").color(egui::Color32::BLACK).size(20.0);
        let back_button = egui::Button::new(back_button_raw).fill(egui::Color32::from_hex("#ac5ddc").unwrap());

        let upload_button_raw = egui::RichText::new("📤 Upload File").color(egui::Color32::BLACK).size(20.0);
        let upload_button = egui::Button::new(upload_button_raw).fill(egui::Color32::from_hex("#ac5ddc").unwrap());

        let folder_make_button_raw = egui::RichText::new("📁 Create Folder").color(egui::Color32::BLACK).size(20.0);
        let folder_make_button = egui::Button::new(folder_make_button_raw).fill(egui::Color32::from_hex("#ac5ddc").unwrap());

        let refresh_raw = egui::RichText::new("🔄 Refresh").color(egui::Color32::BLACK).size(16.0);
        let refresh_button = egui::Button::new(refresh_raw).fill(egui::Color32::from_hex("#ac5ddc").unwrap());

        let logout_raw = egui::RichText::new("Log Out").color(egui::Color32::BLACK).size(14.0);
        let logout_button = egui::Button::new(logout_raw).fill(egui::Color32::from_hex("#ac5ddc").unwrap());
        // Top Navigation Bar
        ui.horizontal(|ui| {
            if ui.add(logout_button).clicked() {
                self.token.clear();
                self.view = ViewState::Login;
            }
            ui.separator();
            
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
                egui::TextEdit::singleline(&mut self.new_folder_name).hint_text("New folder name...").text_color(Color32::WHITE)
                .font(egui::FontId::proportional(24.0))
            );
            if ui.add(folder_make_button).clicked() {
                if !self.new_folder_name.is_empty() {
                    self.create_folder(self.new_folder_name.clone());
                    self.new_folder_name.clear();
                }
            }
        });

        ui.separator();

        // System Status / Errors
        if !self.status_message.is_empty() {
            ui.label(egui::RichText::new(&self.status_message).color(egui::Color32::from_hex("#ac5ddc").unwrap()));
            ui.separator();
        }

        if self.is_loading {
            ui.spinner();
        }
        

        // File List Area
        egui::ScrollArea::vertical().show(ui, |ui| {
            for file in self.files.clone() {
                ui.horizontal(|ui| {
                    if file.is_dir {

                        ui.label(egui::RichText::new("📁")
                        .font(egui::FontId::proportional(24.0))
                        );

                        ui.label(egui::RichText::new(&file.name).strong()
                        .font(egui::FontId::proportional(24.0))
                    );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {

                            let folder_delete_button_raw = egui::RichText::new("🗑 delete").color(egui::Color32::BLACK).size(15.0);
                            let folder_delete_button = egui::Button::new(folder_delete_button_raw).fill(egui::Color32::LIGHT_RED);

                            let folder_open_button_raw = egui::RichText::new("Open Folder").color(egui::Color32::BLACK).size(24.0);
                            let folder_open_button = egui::Button::new(folder_open_button_raw).fill(egui::Color32::from_hex("#ac5ddc").unwrap());

                            if ui.add(folder_delete_button).clicked() {
                                //improved delete logic

                                if self.item_pending_deletion.as_deref() == Some(&file.name) {
                                    // SECOND CLICK: Execute the actual delete function
                                    self.delete_item(&file.name);
        
                                    // Clear the state and the message
                                    self.item_pending_deletion = None; 
                                    self.status_message = String::new(); 
                                }else{
                                    // FIRST CLICK: Queue it up and show the system message
                                    self.item_pending_deletion = Some(file.name.clone());
                                    self.status_message = format!("Delete '{}'? Click 🗑 again to confirm.", &file.name);
                                }
                                }

                            ui.add_space(20.0);                        
                            if ui.add(folder_open_button).clicked() {
                                if self.current_path.is_empty() {
                                    self.current_path = file.name.clone();
                                } else {
                                    self.current_path = format!("{}/{}", self.current_path, file.name);
                                }
                                self.refresh_files();
                            }
                        });
                    } else { //get a preview of the pic
                            let lower_name = file.name.to_lowercase();
                            let is_image = lower_name.ends_with(".png") || lower_name.ends_with(".jpg") || lower_name.ends_with(".jpeg");

                            if is_image {
                                // Check our cache
                                match self.image_cache.get(&file.name) {
                                    Some(ImageState::Loaded(texture)) => {
                                        // Draw the thumbnail! (Restricted to 32x32 pixels, with slightly rounded corners)
                                        ui.add(egui::Image::new(texture).max_width(64.0).max_height(64.0).rounding(4.0));
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
                            } 
                            else {
                                // Standard file icon for PDFs, TXTs, etc.
                                ui.label(egui::RichText::new("📄")
                                .font(egui::FontId::proportional(24.0))
                                );
                            }

                        ui.label(
                            egui::RichText::new(&file.name).strong()
                            .font(egui::FontId::proportional(24.0))
                        );

                        let mb = file.size as f64 / 1_048_576.0;
                        ui.label(format!("({:.2} MB)", mb));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {

                                let file_delete_button_raw = egui::RichText::new("🗑 delete").color(egui::Color32::BLACK).size(15.0);
                                let file_delete_button = egui::Button::new(file_delete_button_raw).fill(egui::Color32::LIGHT_RED);

                                let file_move_raw = egui::RichText::new("Move").color(egui::Color32::BLACK).size(24.0);
                                let file_move_button = egui::Button::new(file_move_raw).fill(egui::Color32::LIGHT_GREEN);

                                let file_download_raw = egui::RichText::new("⬇ Download").color(egui::Color32::BLACK).size(24.0);
                                let file_download_button = egui::Button::new(file_download_raw).fill(egui::Color32::from_hex("#ac5ddc").unwrap());

                            if ui.add(file_delete_button).clicked() {
                                //file deletion improved logic
                                if self.item_pending_deletion.as_deref() == Some(&file.name) {
                                    // SECOND CLICK: Execute the actual delete function
                                    self.delete_item(&file.name);
        
                                    // Clear the state and the message
                                    self.item_pending_deletion = None; 
                                    self.status_message = String::new(); 
                                }else{
                                    // FIRST CLICK: Queue it up and show the system message
                                    self.item_pending_deletion = Some(file.name.clone());
                                    self.status_message = format!("Delete '{}'? Click 🗑 again to confirm.", &file.name);
                                }
                                }
                            ui.add_space(20.0);
                            if ui.add(file_download_button).clicked() {
                                // Prompt user for where to save the file!
                                if let Some(save_path) = rfd::FileDialog::new().set_file_name(&file.name).save_file() {
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
                    
                    // --- THE NEW DROPDOWN MENU ---
                    egui::ComboBox::from_id_source("move_dropdown")
                        // Display the currently selected target (or "Root" if it's empty)
                        .selected_text(if self.move_target_folder.is_empty() { 
                            "Root (/)".to_string() 
                        } else { 
                            format!("/{}", self.move_target_folder) 
                        })
                        .width(250.0)
                        .show_ui(ui, |ui| {
                            // 1. Always offer the Root folder
                            ui.selectable_value(&mut self.move_target_folder, String::new(), "Root (/)");
                            
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
                                
                                ui.selectable_value(&mut self.move_target_folder, parent_path, format!("⬆ Parent ({})", display_name));
                            }

                            // 3. Offer any Subfolders visible on the current screen
                            for f in &self.files {
                                if f.is_dir && f.name != item_name { // Don't allow moving a folder inside itself!
                                    let target_path = if self.current_path.is_empty() {
                                        f.name.clone()
                                    } else {
                                        format!("{}/{}", self.current_path, f.name)
                                    };
                                    ui.selectable_value(&mut self.move_target_folder, target_path.clone(), format!("📁 /{}", target_path));
                                }
                            }
                        });
                    // -----------------------------

                    ui.add_space(15.0);

                    ui.horizontal(|ui| {
                        if ui.button("Confirm Move").clicked() {
                            
                            // Get the full source path
                            let source = if self.current_path.is_empty() {
                                item_name.clone()
                            } else {
                                format!("{}/{}", self.current_path, item_name)
                            };

                            // Fire the network thread using your existing function!
                            self.move_item(source, self.move_target_folder.clone(), item_name);
                            
                            // Close the modal and reset
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
                Err(_) => tx.send(AppMsg::Error("Failed to fetch files".into())).unwrap(),
            }
        });
    }

   fn upload_files(&mut self, file_paths: Vec<PathBuf>) {
        self.is_loading = true;
        self.status_message = format!("Uploading {} file(s)...", file_paths.len());
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let current_path = self.current_path.clone();

        thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/upload", ip);
            
            // 1. Initialize the empty multipart form
            let mut form = reqwest::blocking::multipart::Form::new();

            // 2. Loop through every file the user selected
            for file_path in file_paths {
                let filename = file_path.file_name().unwrap().to_str().unwrap().to_string();
                
                let target_name = if current_path.is_empty() { 
                    filename 
                } else { 
                    format!("{}/{}", current_path, filename) 
                };

                // 3. Create the part and override its filename
                match reqwest::blocking::multipart::Part::file(&file_path) {
                    Ok(part) => {
                        let file_part = part.file_name(target_name);
                        // Attach the part to the form. We use the key "files" 
                        // because that is what your Axum backend is looking for!
                        form = form.part("files", file_part);
                    },
                    Err(e) => {
                        let _ = tx.send(AppMsg::Error(format!("Could not read file: {}", e)));
                        return; // Abort if a file can't be read
                    }
                };
            }

            // 4. Send the massive form containing all the files at once
            match client.post(&url).bearer_auth(token).multipart(form).send() {
                Ok(_) => tx.send(AppMsg::ActionSuccess("Upload complete!".into())).unwrap(),
                Err(e) => tx.send(AppMsg::Error(format!("Upload failed: {}", e))).unwrap(),
            }
        });
    }

    fn download_file(&mut self, filename: &str, save_path: PathBuf) {
        self.is_loading = true;
        self.status_message = "Downloading...".into();
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let full_remote_path = if self.current_path.is_empty() { filename.to_string() } else { format!("{}/{}", self.current_path, filename) };

        thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/download/{}", ip, full_remote_path);
            
            match client.get(&url).bearer_auth(token).send() {
                Ok(mut res) => {
                    if let Ok(mut file) = std::fs::File::create(save_path) {
                        res.copy_to(&mut file).unwrap();
                        tx.send(AppMsg::ActionSuccess("Download complete!".into())).unwrap();
                    }
                }
                Err(_) => tx.send(AppMsg::Error("Download failed".into())).unwrap(),
            }
        });
    }
    fn fetch_preview(&mut self, filename: String) {
        // Mark it as loading so we don't spawn 100 threads for the same image
        self.image_cache.insert(filename.clone(), ImageState::Loading);
        
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
            let url = format!("https://{}:8080/api/download/{}?preview=true", ip, full_path);
            
            if let Ok(res) = client.get(&url).bearer_auth(token).send() {
                if let Ok(bytes) = res.bytes() {
                    // Decode the raw web bytes into a dynamic image
                    if let Ok(img) = image::load_from_memory(&bytes) {
                        let size = [img.width() as _, img.height() as _];
                        let image_buffer = img.to_rgba8();
                        let pixels = image_buffer.as_flat_samples();
                        
                        // Convert to egui's specific color format
                        let color_image = egui::ColorImage::from_rgba_unmultiplied(
                            size,
                            pixels.as_slice(),
                        );
                        
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
        self.is_loading = true;
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let full_path = if self.current_path.is_empty() { folder_name } else { format!("{}/{}", self.current_path, folder_name) };

        thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/folders", ip);
            let payload = serde_json::json!({ "path": full_path });

            if client.post(&url).bearer_auth(token).json(&payload).send().is_ok() {
                tx.send(AppMsg::ActionSuccess("Folder created".into())).unwrap();
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
                    tx.send(AppMsg::ActionSuccess("Item moved successfully!".into())).unwrap();
                }
                Ok(_) => tx.send(AppMsg::Error("Failed to move item.".into())).unwrap(),
                Err(e) => tx.send(AppMsg::Error(format!("Network error: {}", e))).unwrap(),
            }
        });
    }

    fn delete_item(&mut self, item_name: &str) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let full_path = if self.current_path.is_empty() { item_name.to_string() } else { format!("{}/{}", self.current_path, item_name) };

        thread::spawn(move || {
            let client = get_client();
            let url = format!("https://{}:8080/api/delete", ip);
            let payload = serde_json::json!({ "path": full_path });

            if client.post(&url).bearer_auth(token).json(&payload).send().is_ok() {
                tx.send(AppMsg::ActionSuccess("Deleted successfully".into())).unwrap();
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