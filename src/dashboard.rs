use crate::{ImageState, NasClientApp, SortMode, ViewState, network};
use eframe::egui;
use egui::Color32;

impl NasClientApp {
    /// # Render dashboard
    ///
    /// This function is important since it is how the buttons and other interactable items are made.
    /// - Lots of variables I know, but it's easier to split the color of the text from the fill of the box.
    ///
    ///
    pub fn render_dashboard(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
        let refresh_button = egui::Button::new(refresh_raw).fill(self.theme.refresh_btn);

        let logout_raw = egui::RichText::new("Log Out")
            .color(self.theme.text_dashboard)
            .size(14.0);
        let logout_button = egui::Button::new(logout_raw).fill(self.theme.logout_btn);

        // Top Navigation Bar
        // the first item is the logout button
        ui.horizontal(|ui| {
            if ui.add(logout_button).clicked() {
                self.token.clear();
                self.view = ViewState::Login;
            }

            ui.separator();
            // next is the server config button.
            if ui.button("⚙ Server Config").clicked() {
                self.fetch_remote_config(ctx);
            }
            // now the back button.
            if !self.current_path.is_empty() {
                if ui.add(back_button).clicked() {
                    let mut parts: Vec<&str> = self.current_path.split('/').collect();
                    parts.pop();
                    self.current_path = parts.join("/");
                    self.refresh_files(ctx);
                }
            }
            ui.label(
                egui::RichText::new(format!("/{}", self.current_path))
                    .strong()
                    .color(self.theme.list_txt),
            );
            let old_mode = self.sort_mode;
            ui.label("Sort By: ");
            egui::ComboBox::from_id_source("sort_mode_dropdown")
                .selected_text(match self.sort_mode {
                    SortMode::NameAsc => "Name (A–Z)",
                    SortMode::NewestFirst => "Upload Time (Newest)",
                    SortMode::OldestFirst => "Upload Time (Oldest)",
                    SortMode::SizeDesc => "Size (Largest)",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.sort_mode,
                        SortMode::NewestFirst,
                        "Upload Time (Newest)",
                    );
                    ui.selectable_value(
                        &mut self.sort_mode,
                        SortMode::OldestFirst,
                        "Upload Time (Oldest)",
                    );
                    ui.selectable_value(&mut self.sort_mode, SortMode::NameAsc, "Name (A–Z)");
                    ui.selectable_value(&mut self.sort_mode, SortMode::SizeDesc, "Size (Largest)");
                });

            if self.sort_mode != old_mode {
                self.sort_files();
            }
            //refresh button is last of the top.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(refresh_button).clicked() {
                    self.refresh_files(ctx);
                }
            });
        });

        ui.separator();

        // Toolbar (Upload & Create Folder)
        ui.horizontal(|ui| {
            //the upload button is first.
            if ui.add(upload_button).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_files() {
                    //this causes the crash
                    self.upload_files(path, ctx);
                }
            }

            ui.separator();
            //now add the 'add folder' button.
            ui.add(
                egui::TextEdit::singleline(&mut self.new_folder_name)
                    .hint_text("New folder name...")
                    .text_color(Color32::WHITE)
                    .font(egui::FontId::proportional(24.0)),
            );
            if ui.add(folder_make_button).clicked() {
                if !self.new_folder_name.is_empty() {
                    self.create_folder(self.new_folder_name.clone(), ctx);
                    self.new_folder_name.clear();
                }
            }

            ui.separator();

            ui.scope(|ui| {
                // 1. Temporarily override the button background colors for this specific menu
                let visuals = ui.visuals_mut();
                visuals.widgets.inactive.bg_fill = self.theme.theme_btn;
                // Optional: You can make the hover state slightly brighter, or keep it the same
                visuals.widgets.hovered.bg_fill = self.theme.theme_btn;
                visuals.widgets.active.bg_fill = self.theme.theme_btn;

                // 2. Build your personalized text using your custom dashboard text color
                let theme_bttn =
                    egui::RichText::new(format!("🎨 Theme: {}", self.active_theme_name))
                        .color(self.theme.text_title)
                        .size(14.0);

                let reload_raw = egui::RichText::new("🔄 Reload Templates")
                    .color(self.theme.text_dashboard)
                    .size(14.0);
                let reload_button = egui::Button::new(reload_raw).fill(self.theme.theme_btn);
                // 3. Draw the actual dropdown menu
                ui.menu_button(theme_bttn, |ui| {
                    if ui.add(reload_button).clicked() {
                        // Re-read the text file from the hard drive
                        self.available_themes = network::load_theme_templates();

                        // If the user edited the currently active theme, apply the changes instantly!
                        if let Some(updated_theme) =
                            self.available_themes.get(&self.active_theme_name)
                        {
                            self.theme = *updated_theme;
                            network::save_theme(&self.active_theme_name, &self.theme);
                        }

                        ui.close_menu();
                    }
                    ui.separator();

                    let mut theme_names: Vec<&String> = self.available_themes.keys().collect();
                    theme_names.sort();

                    for name in theme_names {
                        let is_active = self.active_theme_name == *name;
                        let display_text = if is_active {
                            format!("✔ {}", name)
                        } else {
                            name.to_string()
                        };

                        // Apply the primary text color to the dropdown list items
                        let option_text =
                            egui::RichText::new(display_text).color(self.theme.theme_btn);

                        if ui.button(option_text).clicked() {
                            if let Some(new_theme) = self.available_themes.get(name) {
                                self.theme = *new_theme;
                                self.active_theme_name = name.clone();

                                // Save the selection
                                network::save_theme(&self.active_theme_name, &self.theme);
                            }
                            ui.close_menu();
                        }
                    }
                });
            });
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
            ui.label(egui::RichText::new(&self.status_message).color(self.theme.list_txt));
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
                        self.download_multiple_files(files_to_download, save_folder, ctx);

                        // Clear the checkboxes immediately so the UI resets
                        self.selected_files.clear();
                    }
                }

                if ui.button("📁 Move Selected").clicked() {
                    self.show_batch_move_modal = true;
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
                        ui.label(
                            egui::RichText::new("📁")
                                .font(egui::FontId::proportional(24.0))
                                .color(self.theme.list_txt),
                        );

                        ui.label(
                            egui::RichText::new(&file.name)
                                .strong()
                                .font(egui::FontId::proportional(24.0))
                                .color(self.theme.list_txt),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let is_locked = self.locked_items.contains(&file.name); //new thing

                            let folder_open_button_raw = egui::RichText::new("Open Folder")
                                .color(self.theme.text_primary)
                                .size(24.0);
                            let folder_open_button =
                                egui::Button::new(folder_open_button_raw).fill(self.theme.open_btn);

                            //new item to raise the lock flag
                            //
                            //
                            if !is_locked {
                                let folder_delete_button_raw = egui::RichText::new("🗑 delete")
                                    .color(self.theme.text_primary)
                                    .size(12.0);
                                let folder_delete_button =
                                    egui::Button::new(folder_delete_button_raw)
                                        .fill(self.theme.delete_btn);

                                if ui.add(folder_delete_button).clicked() {
                                    //improved delete logic

                                    if self.item_pending_deletion.as_deref() == Some(&file.name) {
                                        // SECOND CLICK: Execute the actual delete function
                                        self.delete_item(&file.name, ctx);

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

                                ui.add_space(10.0);
                            }

                            if ui.add(folder_open_button).clicked() {
                                if self.current_path.is_empty() {
                                    self.current_path = file.name.clone();
                                } else {
                                    self.current_path =
                                        format!("{}/{}", self.current_path, file.name);
                                }
                                self.refresh_files(ctx);
                            }

                            ui.add_space(5.0);
                            if !is_locked {
                                let folder_move_button_raw = egui::RichText::new("Move")
                                    .color(self.theme.text_primary)
                                    .size(24.0);
                                let folder_move_button = egui::Button::new(folder_move_button_raw)
                                    .fill(self.theme.move_btn);

                                if ui.add(folder_move_button).clicked() {
                                    self.moving_item = Some(file.name.clone());
                                }
                            }
                            let lock_text = if is_locked {
                                "🔓 Unlock"
                            } else {
                                "🔒 Lock"
                            };
                            let lock_button_raw = egui::RichText::new(lock_text)
                                .color(self.theme.text_primary)
                                .size(20.0);

                            // You can use a generic dark grey fill, or add a lock_btn color to your style.ini
                            let lock_button =
                                egui::Button::new(lock_button_raw).fill(self.theme.move_btn);

                            if ui.add(lock_button).clicked() {
                                if is_locked {
                                    self.locked_items.remove(&file.name);
                                } else {
                                    self.locked_items.insert(file.name.clone());

                                    // Optional: Immediately clear any pending deletion state just in case
                                    // the user clicked "🗑 delete" once but hasn't confirmed it yet.
                                    if self.item_pending_deletion.as_deref() == Some(&file.name) {
                                        self.item_pending_deletion = None;
                                        self.status_message = String::new();
                                    }
                                }
                                network::save_protections(&self.locked_items); //save the changes to the ini file.
                            }
                        });
                    } else {
                        let is_locked = self.locked_items.contains(&file.name); //new thing
                        let mut is_selected = self.selected_files.contains(&file.name);
                        if !is_locked {
                            if ui.checkbox(&mut is_selected, "").clicked() {
                                if is_selected {
                                    self.selected_files.insert(file.name.clone());
                                } else {
                                    self.selected_files.remove(&file.name);
                                }
                            }
                        }
                        //get a preview of the pic
                        // put the name of the file to lowercase to check for common file extensions
                        let lower_name = file.name.to_lowercase();
                        let is_previewable = [
                            ".png", ".jpg", ".jpeg", ".gif", ".webp", ".mp4", ".mov", ".mkv",
                            ".avi", ".webm",
                        ]
                        .iter()
                        .any(|ext| lower_name.ends_with(ext));
                        //check for a zip file.
                        let is_zip = [".zip", ".rar", ".7z", ".tar", ".gz", ".tgz"]
                            .iter()
                            .any(|ext| lower_name.ends_with(ext));
                        //check for common text based files.
                        let is_text = [
                            ".txt", ".text", ".asc", ".md", ".rtf", ".docx", ".odt", ".pages",
                            ".tex", ".pdf", ".csv",
                        ]
                        .iter()
                        .any(|ext| lower_name.ends_with(ext));
                        let is_code = [
                            ".json", ".ini", ".dll", ".cfg", ".conf", ".c", ".rs", ".cpp", ".go",
                            ".cc", ".cxx", ".zig", ".js", ".py", ".lua", ".cmd", ".ps1", ".java",
                            ".class", ".cs", ".exe", ".dylib", ".html", ".m",
                        ]
                        .iter()
                        .any(|ext| lower_name.ends_with(ext));
                        if is_previewable {
                            // Check our cache
                            match self.image_cache.get(&file.name) {
                                Some(ImageState::Loaded(texture)) => {
                                    // Draw the thumbnail!
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
                                    ui.label("🖼️"); // Fallback to emoji if it broke
                                }
                                None => {
                                    // We haven't asked for this image yet! Queue it up and show a spinner for now.
                                    self.fetch_preview(file.name.clone(), ctx);
                                    ui.spinner();
                                }
                            }
                        } else if is_zip {
                            ui.label(
                                egui::RichText::new("📦")
                                    .font(egui::FontId::proportional(24.0))
                                    .color(self.theme.list_txt),
                            );
                        } else if is_text {
                            ui.label(
                                egui::RichText::new("📝")
                                    .font(egui::FontId::proportional(24.0))
                                    .color(self.theme.list_txt),
                            );
                        } else if is_code {
                            ui.label(
                                egui::RichText::new("</>")
                                    .font(egui::FontId::proportional(24.0))
                                    .color(self.theme.list_txt),
                            );
                        } else {
                            // Standard file icon BOORINGG
                            ui.label(
                                egui::RichText::new("📄")
                                    .font(egui::FontId::proportional(24.0))
                                    .color(self.theme.list_txt),
                            );
                        }

                        ui.label(
                            egui::RichText::new(&file.name)
                                .strong()
                                .font(egui::FontId::proportional(24.0))
                                .color(self.theme.list_txt),
                        );

                        let mb = file.size as f64 / 1_048_576.0;
                        ui.label(format!("({:.2} MB)", mb));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let is_locked = self.locked_items.contains(&file.name); //new thing

                            let file_download_raw = egui::RichText::new("⬇ Download")
                                .color(self.theme.text_primary)
                                .size(24.0);
                            let file_download_button =
                                egui::Button::new(file_download_raw).fill(self.theme.download_btn);
                            //this is where the second fix begins
                            //
                            //
                            if !is_locked {
                                let folder_delete_button_raw = egui::RichText::new("🗑 delete")
                                    .color(self.theme.text_primary)
                                    .size(12.0);
                                let folder_delete_button =
                                    egui::Button::new(folder_delete_button_raw)
                                        .fill(self.theme.delete_btn);

                                if ui.add(folder_delete_button).clicked() {
                                    //improved delete logic

                                    if self.item_pending_deletion.as_deref() == Some(&file.name) {
                                        // SECOND CLICK: Execute the actual delete function
                                        self.delete_item(&file.name, ctx);

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
                            }

                            ui.add_space(20.0);
                            if ui.add(file_download_button).clicked() {
                                // Prompt user for where to save the file!
                                if let Some(save_path) =
                                    rfd::FileDialog::new().set_file_name(&file.name).save_file()
                                {
                                    self.download_file(&file.name, save_path, ctx);
                                }
                            }
                            // For Files:
                            if !is_locked {
                                let file_move_raw = egui::RichText::new("Move")
                                    .color(self.theme.text_primary)
                                    .size(24.0);
                                let file_move_button =
                                    egui::Button::new(file_move_raw).fill(self.theme.move_btn);
                                if ui.add(file_move_button).clicked() {
                                    self.moving_item = Some(file.name.clone());
                                }
                            }

                            let lock_text = if is_locked {
                                "🔓 Unlock"
                            } else {
                                "🔒 Lock"
                            };
                            let lock_button_raw = egui::RichText::new(lock_text)
                                .color(self.theme.text_primary)
                                .size(20.0);

                            // You can use a generic dark grey fill, or add a lock_btn color to your style.ini
                            let lock_button =
                                egui::Button::new(lock_button_raw).fill(self.theme.move_btn);

                            if ui.add(lock_button).clicked() {
                                if is_locked {
                                    self.locked_items.remove(&file.name);
                                } else {
                                    self.locked_items.insert(file.name.clone());

                                    // Optional: Immediately clear any pending deletion state just in case
                                    // the user clicked "🗑 delete" once but hasn't confirmed it yet.
                                    if self.item_pending_deletion.as_deref() == Some(&file.name) {
                                        self.item_pending_deletion = None;
                                        self.status_message = String::new();
                                    }
                                }
                                network::save_protections(&self.locked_items); //save the changes to the ini file.
                            }
                        }); //end ui horizontal
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

                            self.move_item(source, self.move_target_folder.clone(), item_name, ctx);

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

        // 2. THE SERVER CONFIG MODAL
        if self.show_config_modal {
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

                        // Convert the backend's bytes into a Megabyte decimal
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
                            // ONLY flip the booleans inside the closure. No `self` calls here
                            if ui.button("Save & Apply").clicked() {
                                trigger_save = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_modal = true;
                            }
                        });
                    }
                });

            // Safely outside the closure, the borrow is dropped, so we can freely use `self` again!
            if trigger_save {
                if let Some(config) = self.active_config.clone() {
                    self.save_remote_config(config, ctx);
                }
            }

            if close_modal {
                self.show_config_modal = false;
            }
        }
        if self.show_batch_move_modal {
            egui::Window::new("Move Selected Items")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Moving {} items", self.selected_files.len()));
                    ui.add_space(10.0);

                    ui.label("Select destination:");

                    // Use a unique ID source so it doesn't clash with the single-move dropdown
                    egui::ComboBox::from_id_source("batch_move_dropdown")
                        .selected_text(if self.move_target_folder.is_empty() {
                            "Root (/)".to_string()
                        } else {
                            format!("/{}", self.move_target_folder)
                        })
                        .width(250.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.move_target_folder,
                                String::new(),
                                "Root (/)",
                            );

                            if !self.current_path.is_empty() {
                                let mut parts: Vec<&str> = self.current_path.split('/').collect();
                                parts.pop();
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

                            for f in &self.files {
                                // Prevent moving selected folders inside themselves!
                                if f.is_dir && !self.selected_files.contains(&f.name) {
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

                    ui.horizontal(|ui| {
                        if ui.button("Confirm Move").clicked() {
                            // Iterate through the HashSet and fire a move request for each file
                            for item_name in self.selected_files.clone() {
                                let source = if self.current_path.is_empty() {
                                    item_name.clone()
                                } else {
                                    format!("{}/{}", self.current_path, item_name)
                                };

                                self.move_item(
                                    source,
                                    self.move_target_folder.clone(),
                                    item_name,
                                    ctx,
                                );
                            }

                            // Reset state and close modal
                            self.show_batch_move_modal = false;
                            self.move_target_folder.clear();
                            self.selected_files.clear();
                        }

                        if ui.button("Cancel").clicked() {
                            self.show_batch_move_modal = false;
                            self.move_target_folder.clear();
                        }
                    });
                });
        }
    } //end of "render dashboard"
}
