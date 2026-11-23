use eframe::egui;
use std::thread;

use crate::{AppMsg, AuthRequest, AuthResponse, HTTP_CLIENT, NasClientApp, network};

impl NasClientApp {
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
    pub fn render_login(&mut self, ui: &mut egui::Ui) {
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
            ui.label(
                egui::RichText::new("NAS IP Address:")
                    .strong()
                    .size(24.0)
                    .color(self.theme.list_txt),
            );

            ui.add(
                egui::TextEdit::singleline(&mut self.ip_input) // change the size of the text bar
                    .font(egui::FontId::proportional(24.0)),
            );
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Password:")
                    .strong()
                    .size(24.0)
                    .color(self.theme.list_txt),
            );

            ui.add(
                egui::TextEdit::singleline(&mut self.password_input)
                    .password(true) // change the size of the text bar
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
                    network::save_config(self.ip_input.trim());
                    let ip = self.ip_input.trim().to_string();
                    let pwd = self.password_input.clone();
                    let tx = self.tx.clone();

                    thread::spawn(move || {
                        let client = HTTP_CLIENT.clone();
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
            ui.label(egui::RichText::new(&self.status_message).color(self.theme.list_txt));
        }
    }
}
