use super::*;

fn neighbor(order: &[usize], current: Option<usize>, forward: bool, wrap: bool) -> Option<usize> {
    if order.is_empty() {
        return None;
    }
    let position = current.and_then(|i| order.iter().position(|&item| item == i));
    let next = match position {
        None if forward => 0,
        None => order.len() - 1,
        Some(i) if forward && i + 1 < order.len() => i + 1,
        Some(i) if !forward && i > 0 => i - 1,
        Some(_) if wrap && forward => 0,
        Some(_) if wrap => order.len() - 1,
        Some(i) => i,
    };
    Some(order[next])
}

impl ManagerApp {
    fn visible_slots(&self) -> Vec<usize> {
        if self.preview_active {
            (0..self.previews.len()).collect()
        } else {
            self.current_device()
                .map(|d| {
                    d.slots
                        .iter()
                        .enumerate()
                        .filter_map(|(i, slot)| slot.image.as_ref().map(|_| i))
                        .collect()
                })
                .unwrap_or_default()
        }
    }

    fn navigate(&mut self, key: egui::Key) {
        use egui::Key::*;
        match key {
            ArrowLeft | ArrowRight => {
                if let Some(slot) = neighbor(
                    &self.visible_slots(),
                    Some(self.selected_slot),
                    key == ArrowRight,
                    true,
                ) {
                    if slot != self.selected_slot {
                        self.selected_slot = slot;
                        self.select_file(None);
                        self.card_scroll = true;
                    }
                }
            }
            ArrowUp | ArrowDown => {
                if let Some(image) = self.current_image() {
                    let order = sorting::indices(image, self.sort_column, self.sort_descending);
                    let next = neighbor(&order, self.selected_file, key == ArrowDown, false);
                    if next != self.selected_file {
                        self.select_file(next);
                        self.navigation_scroll = true;
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn keyboard_navigation(&mut self, ctx: &egui::Context) {
        // Run before widgets handle arrows. Menus and dialogs retain their
        // normal keyboard behavior; text fields must not change the selection.
        if self.root_editor.is_some()
            || self.busy
            || self.pending_load.is_some()
            || self.error_popup.is_some()
            || ctx
                .memory(|m| m.focused())
                .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some())
            || ctx.memory(|m| m.any_popup_open())
            || ctx.data(|d| {
                d.get_temp::<bool>(egui::Id::new("menu-open"))
                    .unwrap_or(false)
            })
        {
            return;
        }
        for key in [
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
        ] {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                self.navigate(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_follow_sort_and_leave_tab_to_widget_focus() {
        let ctx = egui::Context::default();
        let mut app = ManagerApp::create_state(None, String::new());
        let mut raw = vmu::tests::formatted(200);
        for (i, name) in [b'Z', b'A', b'M'].iter().enumerate() {
            let block = 199 - i;
            let entry = &mut raw[253 * 512 + i * 32..253 * 512 + (i + 1) * 32];
            entry.fill(0);
            entry[0] = 0x33;
            entry[2..4].copy_from_slice(&(block as u16).to_le_bytes());
            entry[4] = *name;
            entry[24..26].copy_from_slice(&1u16.to_le_bytes());
            raw[254 * 512 + block * 2..254 * 512 + block * 2 + 2]
                .copy_from_slice(&0xfffau16.to_le_bytes());
        }
        app.previews
            .push(("local.bin".into(), vmu::parse_image(&raw).unwrap()));
        app.preview_active = true;
        app.navigate(egui::Key::Tab);
        assert!(app.preview_active, "Tab is reserved for widget focus");
        let press = |app: &mut ManagerApp, key| {
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.keyboard_navigation(ctx));
        };
        press(&mut app, egui::Key::ArrowDown);
        assert_eq!(
            app.current_image().unwrap().files[app.selected_file.unwrap()].name,
            "A"
        );
        app.sort_column = sorting::Column::FirstBlock;
        app.sort_descending = true;
        press(&mut app, egui::Key::ArrowUp);
        assert_eq!(
            app.current_image().unwrap().files[app.selected_file.unwrap()].name,
            "Z"
        );
        assert!(app.navigation_scroll);
        app.devices.push(Device {
            id: DeviceId {
                bus: Some(1),
                address: Some(2),
            },
            firmware: vmu::Firmware::MaplePad20,
            slots: vec![
                device::SlotState {
                    image: None,
                    error: None,
                },
                device::SlotState {
                    image: Some(vmu::parse_image(&vmu::tests::formatted(241)).unwrap()),
                    error: None,
                },
            ],
        });
        press(&mut app, egui::Key::Tab);
        assert!(
            app.preview_active,
            "Tab must not switch sources even when both exist"
        );
        assert_eq!(app.current_image().unwrap().bytes, raw);
        app.error_popup = Some("Dialog".into());
        let selected = app.selected_file;
        press(&mut app, egui::Key::ArrowDown);
        assert_eq!(app.selected_file, selected, "dialogs block navigation");
        app.error_popup = None;
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("menu-open"), true));
        press(&mut app, egui::Key::ArrowDown);
        assert_eq!(app.selected_file, selected, "menus block navigation");
    }
    #[test]
    fn navigation_wraps_cards_but_clamps_saves_and_skips_missing_slots() {
        assert_eq!(neighbor(&[0, 2, 7], Some(7), true, true), Some(0));
        assert_eq!(neighbor(&[0, 2, 7], Some(0), false, true), Some(7));
        assert_eq!(neighbor(&[0, 2, 7], Some(0), true, true), Some(2));
        assert_eq!(neighbor(&[4, 2, 0], Some(4), true, false), Some(2));
        assert_eq!(neighbor(&[4, 2, 0], Some(0), true, false), Some(0));
        assert_eq!(neighbor(&[4, 2, 0], None, true, false), Some(4));
        assert_eq!(neighbor(&[], None, true, true), None);
    }
}
