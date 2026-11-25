use crate::{
    AppConfig, AppMsg, AppTheme, CONFIG_FILE, HTTP_CLIENT, HashMap, ImageState, ListResponse,
    MoveRequest, NasClientApp, PROTECTIONS_FILE, QueuedUpload, STYLE_FILE, SortMode,
};
use eframe::egui;
use std::{path::PathBuf, thread};

impl NasClientApp {
    // This is where all of the network functions are called.
    //-----------------------------------------------------------------------------

    pub fn refresh_files(&mut self, ctx: &egui::Context) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let path = self.current_path.clone();

        std::thread::spawn(move || {
            let client = HTTP_CLIENT.clone();
            let url = format!("https://{}:8080/api/files?path={}", ip, path);

            match client.get(&url).bearer_auth(token).send() {
                Ok(res) if res.status().is_success() => {
                    // Status is 200 OK. Try to parse the files.
                    match res.json::<ListResponse>() {
                        Ok(data) => {
                            let _ = tx.send(AppMsg::FilesLoaded(data.files));
                        }
                        Err(_) => {
                            let _ = tx.send(AppMsg::Error("Failed to read server data.".into()));
                        }
                    }
                }
                Ok(res) if res.status() == 401 => {
                    // Token expired! Explicitly kick them to the error state.
                    let _ = tx.send(AppMsg::SessionExpired);
                }
                Ok(res) => {
                    // Catch-all for other server errors (404 Not Found, 500 Internal Server Error)
                    let _ = tx.send(AppMsg::Error(format!(
                        "Server returned error code: {}",
                        res.status()
                    )));
                }
                Err(_) => {
                    // Complete network failure (server offline, no wifi)
                    let _ = tx.send(AppMsg::Error("Network connection failed :(.".into()));
                }
            }

            // CRITICAL: Unconditionally wake up egui!
            // Placing this outside the match block guarantees that no matter what the refresh WILL be called.
            // Even if the server returns an error, we still want to wake up egui so it can show the error message.
            ctx_clone.request_repaint();
        });
    }

    pub fn upload_files(&mut self, file_paths: Vec<PathBuf>, ctx: &egui::Context) {
        for file_path in file_paths {
            self.upload_queue.push_back(QueuedUpload {
                file_path,
                target_folder: self.current_path.clone(),
            });
        }

        // Start immediately if idle; otherwise, the active upload will pick it up next
        self.process_next_upload(ctx);
    }

    pub fn process_next_upload(&mut self, ctx: &egui::Context) {
        if self.is_uploading {
            return;
        }

        let Some(job) = self.upload_queue.pop_front() else {
            self.upload_progress = None;
            return;
        };

        self.is_uploading = true;
        self.upload_progress = Some(0.0);

        let filename = job
            .file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let remaining = self.upload_queue.len();
        self.status_message = if remaining > 0 {
            format!("Uploading '{}' ({} more in queue)...", filename, remaining)
        } else {
            format!("Uploading '{}'...", filename)
        };

        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();

        std::thread::spawn(move || {
            let client = HTTP_CLIENT.clone();
            let url = format!("https://{}:8080/api/upload_chunk", ip);

            let target_name = if job.target_folder.is_empty() {
                filename.clone()
            } else {
                format!("{}/{}", job.target_folder, filename)
            };

            let mut file = match std::fs::File::open(&job.file_path) {
                Ok(f) => f,
                Err(e) => {
                    let _ = tx.send(AppMsg::SingleUploadFailed(format!(
                        "Could not read '{}': {}",
                        filename, e
                    )));
                    ctx_clone.request_repaint();
                    return;
                }
            };

            let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
            let chunk_size: u64 = if file_size <= 50 * 1024 * 1024 {
                file_size
            } else if file_size <= 1024 * 1024 * 1024 {
                25 * 1024 * 1024
            } else {
                100 * 1024 * 1024
            };

            let total_chunks = if file_size == 0 {
                1
            } else {
                (file_size as f64 / chunk_size as f64).ceil() as u64
            };

            for chunk_index in 0..total_chunks {
                let current_chunk_size =
                    std::cmp::min(chunk_size, file_size - (chunk_index * chunk_size));
                let offset = chunk_index * chunk_size;
                let mut buffer = vec![0; current_chunk_size as usize];

                if let Err(e) = std::io::Read::read_exact(&mut file, &mut buffer) {
                    let _ = tx.send(AppMsg::SingleUploadFailed(format!(
                        "Failed reading '{}': {}",
                        filename, e
                    )));
                    ctx_clone.request_repaint();
                    return;
                }

                let mut attempts = 0;
                let mut success = false;

                while attempts < 3 && !success {
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
                            std::thread::sleep(std::time::Duration::from_secs(2));
                        }
                    }
                }

                if !success {
                    let _ = tx.send(AppMsg::SingleUploadFailed(format!(
                        "Upload failed after 3 retries: {}",
                        filename
                    )));
                    ctx_clone.request_repaint();
                    return;
                }

                let pct = (chunk_index as f32 + 1.0) / total_chunks as f32;
                let _ = tx.send(AppMsg::UploadProgress(pct));
                ctx_clone.request_repaint();
            }

            let _ = tx.send(AppMsg::SingleUploadFinished(filename));
            ctx_clone.request_repaint();
        });
    }

    pub fn download_file(&mut self, filename: &str, save_path: PathBuf, ctx: &egui::Context) {
        self.is_loading = true;
        self.status_message = "Downloading...".into();

        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let full_remote_path = if self.current_path.is_empty() {
            filename.to_string()
        } else {
            format!("{}/{}", self.current_path, filename)
        };

        thread::spawn(move || {
            let client = HTTP_CLIENT.clone();
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
            ctx_clone.request_repaint();
        });
    }
    ///## Fetch Preview
    ///
    /// - A cute little function that gives a preview of the picture
    /// - probably won't work for odd picture formats or Gifs.
    /// - Been tweaked many times for its impact on the app's ram usage (high)
    ///
    pub fn fetch_preview(&mut self, filename: String, ctx: &egui::Context) {
        // Mark it as loading so we don't spawn 100 threads for the same image
        self.image_cache
            .insert(filename.clone(), ImageState::Loading);

        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();

        let full_path = if self.current_path.is_empty() {
            filename.clone()
        } else {
            format!("{}/{}", self.current_path, filename)
        };

        std::thread::spawn(move || {
            let client = HTTP_CLIENT.clone();
            let url = format!(
                "https://{}:8080/api/download/{}?preview=true",
                ip, full_path
            );

            if let Ok(res) = client.get(&url).bearer_auth(token).send() {
                // Ensure we got a 200 OK before we try to parse bytes
                if res.status().is_success() {
                    if let Ok(bytes) = res.bytes() {
                        // Decode the raw web bytes into a dynamic image
                        //now with the fix this is different.
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            // The bytes are now a tiny thumbnail from the server!
                            let size = [img.width() as _, img.height() as _];
                            let image_buffer = img.to_rgba8();
                            let pixels = image_buffer.as_flat_samples();

                            // Convert to egui's specific color format
                            let color_image =
                                egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());

                            // Safely send the lightweight image back to the main thread
                            let _ = tx.send(AppMsg::ImageLoaded(filename, color_image));
                            ctx_clone.request_repaint();
                            return;
                        }
                    }
                }
            }

            // If anything fails, safely send the failure message without panicking
            let _ = tx.send(AppMsg::ImageFailed(filename));
            ctx_clone.request_repaint();
        });
    }

    pub fn create_folder(&mut self, folder_name: String, ctx: &egui::Context) {
        if self
            .files
            .iter()
            .any(|f| f.is_dir && f.name.eq_ignore_ascii_case(&folder_name))
        {
            // Abort immediately and display the cute system message!
            self.status_message =
                format!("Cannot create folder: '{}' already exists ☆☆☆", folder_name);
            return;
        } //Prevent the issue that Jakob talked about, where folders with the same name overwrite.
        self.is_loading = true;
        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let full_path = if self.current_path.is_empty() {
            folder_name
        } else {
            format!("{}/{}", self.current_path, folder_name)
        };

        thread::spawn(move || {
            let client = HTTP_CLIENT.clone();
            let url = format!("https://{}:8080/api/folders", ip);
            let payload = serde_json::json!({ "path": full_path });

            if client
                .post(&url)
                .bearer_auth(token)
                .json(&payload)
                .send()
                .is_ok()
            {
                tx.send(AppMsg::ActionSuccess("☆ Folder created ☆".into()))
                    .unwrap();
            }
            ctx_clone.request_repaint();
        });
    }
    pub fn sort_files(&mut self) {
        let mode = self.sort_mode;
        self.files.sort_by(|a, b| {
            // Always keep folders above files
            b.is_dir.cmp(&a.is_dir).then_with(|| match mode {
                SortMode::NameAsc => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortMode::NewestFirst => b.modified.cmp(&a.modified),
                SortMode::OldestFirst => a.modified.cmp(&b.modified),
                SortMode::SizeDesc => b.size.cmp(&a.size),
            })
        });
    }
    pub fn move_item(
        &mut self,
        source_path: String,
        destination_folder: String,
        file_name: String,
        ctx: &egui::Context,
    ) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();

        thread::spawn(move || {
            let client = HTTP_CLIENT.clone();
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
                    tx.send(AppMsg::ActionSuccess("Item moved successfully! ☆☆☆".into()))
                        .unwrap();
                }
                Ok(_) => tx
                    .send(AppMsg::Error("Failed to move item.".into()))
                    .unwrap(),
                Err(e) => tx
                    .send(AppMsg::Error(format!("Network error: {}", e)))
                    .unwrap(),
            }
            ctx_clone.request_repaint(); // new!
        });
    }

    pub fn delete_item(&mut self, item_name: &str, ctx: &egui::Context) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let full_path = if self.current_path.is_empty() {
            item_name.to_string()
        } else {
            format!("{}/{}", self.current_path, item_name)
        };

        thread::spawn(move || {
            let client = HTTP_CLIENT.clone();
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
                ctx_clone.request_repaint();
            }
        });
    }
    // New Stuff 7/6/2026
    pub fn download_multiple_files(
        &mut self,
        files: Vec<String>,
        save_folder: std::path::PathBuf,
        ctx: &egui::Context,
    ) {
        self.is_loading = true;
        self.status_message = format!("Downloading {} files...", files.len());

        let tx = self.tx.clone();
        let ctx_clone = ctx.clone(); //new
        let ip = self.ip_input.clone();
        let token = self.token.clone();
        let current_path = self.current_path.clone();

        std::thread::spawn(move || {
            let client = HTTP_CLIENT.clone();
            //total_files and index are only useful for tracking downloads. This doesn't exist yet.
            for (_index, filename) in files.iter().enumerate() {
                // Determine the correct server path
                let full_path = if current_path.is_empty() {
                    filename.clone()
                } else {
                    format!("{}/{}", current_path, filename)
                };

                let url = format!("https://{}:8080/api/download/{}", ip, full_path);

                // Construct the exact local file path C:\Users\You\Downloads\report.pdf
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
            ctx_clone.request_repaint();
        });
    }
    //I am not going to merge the two download functions, that's a lot of change to server and the client.
    pub fn fetch_remote_config(&mut self, ctx: &egui::Context) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();

        std::thread::spawn(move || {
            let client = HTTP_CLIENT.clone();
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
                                        ctx_clone.request_repaint();
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

    pub fn save_remote_config(&mut self, new_config: AppConfig, ctx: &egui::Context) {
        self.is_loading = true;
        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let ip = self.ip_input.clone();
        let token = self.token.clone();

        std::thread::spawn(move || {
            let client = HTTP_CLIENT.clone();
            let url = format!("https://{}:8080/api/config", ip);

            if client
                .post(&url)
                .bearer_auth(token)
                .json(&new_config)
                .send()
                .is_ok()
            {
                let _ = tx.send(AppMsg::ConfigSaved);
                ctx_clone.request_repaint();
            } else {
                let _ = tx.send(AppMsg::Error("Failed to save server configuration.".into()));
            }
        });
    }
}

// --- Config Helpers ------------------------------------------------------------------------------------------
/// This is the config for the IP number input at the start so you don't need to type it a ton.
pub fn load_config() -> String {
    std::fs::read_to_string(CONFIG_FILE)
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix("NAS_IP=").map(|s| s.trim().to_string()))
        .unwrap_or_default()
}
/// This saves the config of the IP as NAS_IP
pub fn save_config(ip: &str) {
    let _ = std::fs::write(CONFIG_FILE, format!("NAS_IP={}\n", ip));
}

// The Style Update
///# Parse Hex
///The Hex Parser for the Color
pub fn parse_hex(hex: &str, fallback: egui::Color32) -> egui::Color32 {
    let clean_hex = hex.trim();
    egui::Color32::from_hex(clean_hex).unwrap_or(fallback)
}

pub fn load_theme(available_themes: &HashMap<String, AppTheme>) -> (String, AppTheme) {
    // 1. Define your default fallback name and hardcoded colors
    let mut active_theme_name = String::from("Nightmare");

    let mut theme = AppTheme {
        background: egui::Color32::from_hex("#211d26").unwrap(),
        text_primary: egui::Color32::from_hex("#cfcccc").unwrap(),
        text_dashboard: egui::Color32::from_hex("#cfcccc").unwrap(),
        text_title: egui::Color32::from_hex("#cfcccc").unwrap(),
        list_txt: egui::Color32::from_hex("#cfcccc").unwrap(),
        download_btn: egui::Color32::from_hex("#7500d7").unwrap(),
        upload_btn: egui::Color32::from_hex("#8b00ff").unwrap(),
        delete_btn: egui::Color32::from_hex("#2c0050").unwrap(),
        move_btn: egui::Color32::from_hex("#9f31fb").unwrap(),
        logout_btn: egui::Color32::from_hex("#8b00ff").unwrap(),
        connect_btn: egui::Color32::from_hex("#8b00ff").unwrap(),
        open_btn: egui::Color32::from_hex("#7500d7").unwrap(),
        refresh_btn: egui::Color32::from_hex("#8b00ff").unwrap(),
        path_btn: egui::Color32::from_hex("#8b00ff").unwrap(),
        folder_btn: egui::Color32::from_hex("#8b00ff").unwrap(),
        theme_btn: egui::Color32::from_hex("#8b00ff").unwrap(),
    };

    if !std::path::Path::new(STYLE_FILE).exists() {
        println!("Style file not found. Creating {}...", STYLE_FILE);
        // Write the new format with the Active_Theme setting at the top
        let default_ini = "[Settings]\n\
                           Active_Theme = Nightmare\n\
                           \n\
                           [Colors]\n\
                           background = #211d26\n\
                           Text_Primary = #cfcccc\n\
                           Text_dashboard = #cfcccc\n\
                           text_title =     #cfcccc\n\
                           list_txt =        #cfcccc\n\
                           download_btn = #7500d7\n\
                           upload_btn = #8b00ff\n\
                           delete_btn = #2c0050\n\
                           move_btn = #9f31fb\n\
                           logout_btn = #8b00ff\n\
                           connect_btn = #8b00ff\n\
                           open_btn = #7500d7\n\
                           refresh_btn = #8b00ff\n\
                           path_btn = #8b00ff\n\
                           folder_btn = #8b00ff\n\
                           theme_btn = #8b00ff";
        let _ = std::fs::write(STYLE_FILE, default_ini);
        return (active_theme_name, theme);
    }

    // Read the file to find the active theme name first
    if let Ok(content) = std::fs::read_to_string(STYLE_FILE) {
        for line in content.lines() {
            let line_clean = line.trim();
            if line_clean.to_lowercase().starts_with("active_theme") {
                if let Some((_, val)) = line_clean.split_once('=') {
                    active_theme_name = val.trim().to_string();
                }
            }
        }

        // 3. Set the base theme to whatever the INI requested (if it exists in our txt file)
        if let Some(template_theme) = available_themes.get(&active_theme_name) {
            theme = *template_theme;
        }

        // 4. Read the file again to apply any manual custom color overrides
        for line in content.lines() {
            let line_clean = line.trim();
            if line_clean.starts_with('#')
                || line_clean.starts_with('[')
                || line_clean.to_lowercase().starts_with("active_theme")
            {
                continue;
            }

            let parts: Vec<&str> = line_clean.split('=').collect();
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
                    "list_txt" => theme.list_txt = parse_hex(hex_val, theme.list_txt),
                    "download_btn" => theme.download_btn = parse_hex(hex_val, theme.download_btn),
                    "upload_btn" => theme.upload_btn = parse_hex(hex_val, theme.upload_btn),
                    "delete_btn" => theme.delete_btn = parse_hex(hex_val, theme.delete_btn),
                    "move_btn" => theme.move_btn = parse_hex(hex_val, theme.move_btn),
                    "logout_btn" => theme.logout_btn = parse_hex(hex_val, theme.logout_btn),
                    "connect_btn" => theme.connect_btn = parse_hex(hex_val, theme.connect_btn),
                    "open_btn" => theme.open_btn = parse_hex(hex_val, theme.open_btn),
                    "refresh_btn" => theme.refresh_btn = parse_hex(hex_val, theme.refresh_btn),
                    "path_btn" => theme.path_btn = parse_hex(hex_val, theme.path_btn),
                    "folder_btn" => theme.folder_btn = parse_hex(hex_val, theme.folder_btn),
                    "theme_btn" => theme.theme_btn = parse_hex(hex_val, theme.theme_btn),
                    _ => {}
                }
            }
        }
    }

    (active_theme_name, theme)
}

pub fn load_theme_templates() -> HashMap<String, AppTheme> {
    let mut themes = HashMap::new();

    // Read the file. If it doesn't exist, return an empty map.
    let content = match std::fs::read_to_string("StyleTemplates.txt") {
        Ok(c) => c,
        Err(_) => return themes,
    };

    let mut current_name = String::new();
    let mut current_theme = AppTheme::default();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }

        // Detect a new theme name (e.g., "PinkPig:")
        if line.ends_with(':') {
            if !current_name.is_empty() {
                themes.insert(current_name.clone(), current_theme);
            }
            current_name = line.trim_end_matches(':').trim().to_string();
            current_theme = AppTheme::default(); // Reset for the new theme
        }
        // Parse the key-value color pairs
        else if let Some((key, val)) = line.split_once('=') {
            let hex = val.trim();
            match key.trim().to_lowercase().as_str() {
                "background" => current_theme.background = parse_hex(hex, current_theme.background),
                "text_primary" => {
                    current_theme.text_primary = parse_hex(hex, current_theme.text_primary)
                }
                "text_dashboard" => {
                    current_theme.text_dashboard = parse_hex(hex, current_theme.text_dashboard)
                }
                "text_title" => current_theme.text_title = parse_hex(hex, current_theme.text_title),
                "list_txt" => current_theme.list_txt = parse_hex(hex, current_theme.list_txt),
                "download_btn" => {
                    current_theme.download_btn = parse_hex(hex, current_theme.download_btn)
                }
                "upload_btn" => current_theme.upload_btn = parse_hex(hex, current_theme.upload_btn),
                "delete_btn" => current_theme.delete_btn = parse_hex(hex, current_theme.delete_btn),
                "move_btn" => current_theme.move_btn = parse_hex(hex, current_theme.move_btn),
                "logout_btn" => current_theme.logout_btn = parse_hex(hex, current_theme.logout_btn),
                "connect_btn" => {
                    current_theme.connect_btn = parse_hex(hex, current_theme.connect_btn)
                }
                "open_btn" => current_theme.open_btn = parse_hex(hex, current_theme.open_btn),
                "refresh_btn" => {
                    current_theme.refresh_btn = parse_hex(hex, current_theme.refresh_btn)
                }
                "path_btn" => current_theme.path_btn = parse_hex(hex, current_theme.path_btn),
                "folder_btn" => current_theme.folder_btn = parse_hex(hex, current_theme.folder_btn),
                "theme_btn" => current_theme.theme_btn = parse_hex(hex, current_theme.theme_btn),
                _ => {}
            }
        }
    }

    // Catch the final theme in the file
    if !current_name.is_empty() {
        themes.insert(current_name, current_theme);
    }

    themes
}

/// Put this helper function right above your save_theme function
pub fn color_to_hex(c: egui::Color32) -> String {
    if c.a() == 255 {
        // Standard 6-character hex for solid colors
        format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
    } else {
        // 8-character hex for transparent colors
        format!("#{:02X}{:02X}{:02X}{:02X}", c.r(), c.g(), c.b(), c.a())
    }
}

pub fn save_theme(theme_name: &str, theme: &AppTheme) {
    let ini_content = format!(
        "[Settings]\n\
         Active_Theme = {}\n\
         \n\
         [Colors]\n\
         background = {}\n\
         Text_Primary = {}\n\
         Text_dashboard = {}\n\
         text_title = {}\n\
         list_txt = {}\n\
         download_btn = {}\n\
         upload_btn = {}\n\
         delete_btn = {}\n\
         move_btn = {}\n\
         logout_btn = {}\n\
         connect_btn = {}\n\
         open_btn = {}\n\
         refresh_btn = {}\n\
         path_btn = {}\n\
         folder_btn = {}\n\
         theme_btn = {}\n
         ",
        theme_name,
        color_to_hex(theme.background),
        color_to_hex(theme.text_primary),
        color_to_hex(theme.text_dashboard),
        color_to_hex(theme.text_title),
        color_to_hex(theme.list_txt),
        color_to_hex(theme.download_btn),
        color_to_hex(theme.upload_btn),
        color_to_hex(theme.delete_btn),
        color_to_hex(theme.move_btn),
        color_to_hex(theme.logout_btn),
        color_to_hex(theme.connect_btn),
        color_to_hex(theme.open_btn),
        color_to_hex(theme.refresh_btn),
        color_to_hex(theme.path_btn),
        color_to_hex(theme.folder_btn),
        color_to_hex(theme.theme_btn),
    );

    let _ = std::fs::write(STYLE_FILE, ini_content);
}
pub fn save_protections(locked_items: &std::collections::HashSet<String>) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::File::create(PROTECTIONS_FILE) {
        let _ = writeln!(file, "[Protected Files]");
        for item in locked_items {
            let _ = writeln!(file, "{}", item);
        }
    }
}

// Reads the text file and returns a populated HashSet
pub fn load_protections() -> std::collections::HashSet<String> {
    let mut locked_items = std::collections::HashSet::new();

    if let Ok(content) = std::fs::read_to_string(PROTECTIONS_FILE) {
        for line in content.lines() {
            let trimmed = line.trim();
            // Skip empty lines and the section header
            if !trimmed.is_empty() && !trimmed.starts_with('[') {
                locked_items.insert(trimmed.to_string());
            }
        }
    }

    locked_items
}
///Is only used in the dashboard file to fix when names are way too long.
/// - Currently truncates to 20 letters and appends the type of file
/// - ex: client_and_other....zip
pub fn format_display_name(name: &str, max_len: usize) -> String {
    let path = std::path::Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = path.extension().and_then(|e| e.to_str());

    if stem.chars().count() <= max_len {
        return name.to_string();
    }

    let truncated: String = stem.chars().take(max_len).collect();

    match ext {
        Some(e) => format!("{}...{}", truncated, e),
        None => format!("{}...", truncated),
    }
}
