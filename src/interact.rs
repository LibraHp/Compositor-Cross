impl App {
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        self.canvas_rect = Some(rect);
        if self.editor.is_none() {
            self.welcome(ui, rect);
            return;
        }
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        if self.fit_pending {
            self.fit(rect);
            self.fit_pending = false;
        }
        self.paint_canvas(ui, rect);
        self.canvas_input(ui, rect, &response);
    }

    fn welcome(&mut self, ui: &mut egui::Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, CANVAS_BG);
        let center = rect.center();
        ui.painter().text(
            center + vec2(0.0, -92.0),
            egui::Align2::CENTER_CENTER,
            self.t("Compositor", "Compositor"),
            egui::FontId::proportional(28.0),
            Color32::WHITE,
        );
        ui.painter().text(
            center + vec2(0.0, -58.0),
            egui::Align2::CENTER_CENTER,
            self.t("跨平台图层合成。也可以把图像拖进窗口。", "Cross-platform compositor. You can also drop an image."),
            egui::FontId::proportional(14.0),
            Color32::from_rgb(180, 180, 180),
        );
        let width = 220.0;
        let height = 32.0;
        let origin = center - vec2(width * 0.5, 20.0);
        if ui.put(Rect::from_min_size(origin, vec2(width, height)), egui::Button::new(self.t("新建工程", "New Project"))).clicked() {
            self.modal = Modal::New { width: 1920, height: 1080, ppi: 72.0, white: true };
        }
        if ui.put(Rect::from_min_size(origin + vec2(0.0, 40.0), vec2(width, height)), egui::Button::new(self.t("打开图像", "Open Image"))).clicked() {
            self.pick_image();
        }
        if ui.put(Rect::from_min_size(origin + vec2(0.0, 80.0), vec2(width, height)), egui::Button::new(self.t("打开工程", "Open Project"))).clicked() {
            self.pick_project();
        }
        let recent: Vec<std::path::PathBuf> = self.recent.iter().take(4).cloned().collect();
        for (index, path) in recent.iter().enumerate() {
            let label = path.file_name().and_then(|name| name.to_str()).unwrap_or("recent");
            let y = 132.0 + index as f32 * 28.0;
            if ui.put(Rect::from_min_size(origin + vec2(0.0, y), vec2(width, 24.0)), egui::Button::new(label).frame(false)).clicked() {
                self.open_any(path);
            }
        }
    }

    fn paint_canvas(&mut self, ui: &mut egui::Ui, rect: Rect) {
        if self.live_hold && self.texture_dirty && !self.transform_gesture() {
            self.upload_texture(ui.ctx());
            self.clear_live_drag();
        }
        if self.transform_gesture() {
            self.ensure_live_drag(ui.ctx());
        } else if self.live_under_rx.is_some() {
            self.poll_live_drag(ui.ctx());
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, CANVAS_BG);
        let Some(editor) = &self.editor else { return };
        let doc_w = editor.document.width as f32;
        let doc_h = editor.document.height as f32;
        let (origin_x, origin_y, cover_w, cover_h) = self.preview.as_ref().map(|preview| {
            (
                preview.origin_x,
                preview.origin_y,
                preview.image.width as f32 / preview.scale.max(0.01),
                preview.image.height as f32 / preview.scale.max(0.01),
            )
        }).unwrap_or((0.0, 0.0, doc_w, doc_h));
        let image_rect = Rect::from_min_max(
            self.doc_to_screen((origin_x, origin_y), rect),
            self.doc_to_screen((origin_x + cover_w, origin_y + cover_h), rect),
        );
        let frame_rect = Rect::from_min_max(self.doc_to_screen((0.0, 0.0), rect), self.doc_to_screen((doc_w, doc_h), rect));
        let show_live = (self.live_drag_on || self.live_hold) && !self.live_layers.is_empty();
        if show_live {
            if let Some((texture, ox, oy, scale)) = &self.live_under {
                let cover = vec2(texture.size_vec2().x / scale.max(0.01), texture.size_vec2().y / scale.max(0.01));
                let under_rect = Rect::from_min_max(
                    self.doc_to_screen((*ox, *oy), rect),
                    self.doc_to_screen((*ox + cover.x, *oy + cover.y), rect),
                );
                painter.image(texture.id(), under_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            } else if let Some(texture) = &self.texture {
                painter.image(texture.id(), image_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                self.cover_live_starts(&painter, rect);
            }
            self.paint_live_layers(&painter, rect);
        } else if let Some(texture) = &self.texture {
            painter.image(texture.id(), image_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            painter.rect_filled(image_rect, 0.0, Color32::from_rgb(40, 40, 40));
        }
        painter.rect_stroke(frame_rect, 0.0, egui::Stroke::new(1.0, Color32::from_rgb(80, 80, 80)), egui::StrokeKind::Outside);
        if self.show_grid {
            self.paint_grid(&painter, rect, frame_rect);
        }
        for guide in &editor.document.guides {
            let color = Color32::from_rgb(80, 190, 255);
            match guide.axis {
                GuideAxis::Vertical => {
                    let x = self.doc_to_screen((guide.position, 0.0), rect).x;
                    painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], egui::Stroke::new(1.0, color));
                }
                GuideAxis::Horizontal => {
                    let y = self.doc_to_screen((0.0, guide.position), rect).y;
                    painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], egui::Stroke::new(1.0, color));
                }
            }
        }
        if !self.transform_gesture() {
            self.refresh_contours();
        }
        self.paint_selection(&painter, rect);
        self.paint_gesture(&painter, rect);
        self.paint_crop(&painter, rect);
        self.paint_clone_mark(&painter, rect);
        if self.zoom >= 8.0 {
            self.paint_pixel_grid(&painter, rect);
        }
        if self.tool == Tool::Move {
            self.paint_handles(&painter, rect);
        }
        if self.show_rulers {
            self.paint_rulers(&painter, rect);
        }
        let space = ui.input(|i| i.key_down(egui::Key::Space));
        if !space && matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::Clone | Tool::Heal | Tool::Blur | Tool::Liquify) {
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                if rect.contains(pos) {
                    painter.circle_stroke(pos, self.brush_size * self.zoom, egui::Stroke::new(1.0, Color32::WHITE));
                }
            }
        }
    }

    fn paint_grid(&self, painter: &egui::Painter, rect: Rect, frame: Rect) {
        let step = self.grid * self.zoom;
        if step < 6.0 {
            return;
        }
        let color = Color32::from_rgba_unmultiplied(255, 255, 255, 28);
        let (x0, _) = self.screen_to_doc(rect.min, rect);
        let (x1, _) = self.screen_to_doc(rect.max, rect);
        let mut x = (x0 / self.grid).floor() * self.grid;
        while x < x1 {
            let sx = self.doc_to_screen((x, 0.0), rect).x;
            if sx >= frame.left() && sx <= frame.right() {
                let top = frame.top().max(rect.top());
                let bottom = frame.bottom().min(rect.bottom());
                painter.line_segment([pos2(sx, top), pos2(sx, bottom)], egui::Stroke::new(1.0, color));
            }
            x += self.grid;
        }
        let (_, y0) = self.screen_to_doc(rect.min, rect);
        let (_, y1) = self.screen_to_doc(rect.max, rect);
        let mut y = (y0 / self.grid).floor() * self.grid;
        while y < y1 {
            let sy = self.doc_to_screen((0.0, y), rect).y;
            if sy >= frame.top() && sy <= frame.bottom() {
                let left = frame.left().max(rect.left());
                let right = frame.right().min(rect.right());
                painter.line_segment([pos2(left, sy), pos2(right, sy)], egui::Stroke::new(1.0, color));
            }
            y += self.grid;
        }
    }

    fn paint_gesture(&self, painter: &egui::Painter, rect: Rect) {
        let stroke = egui::Stroke::new(1.0, Color32::from_rgb(120, 190, 255));
        let end = self.hover.map(|(x, y, _)| (x, y));
        match &self.gesture {
            Gesture::Shape { start } => {
                if let Some(end) = end {
                    self.paint_shape_preview(painter, rect, *start, end, stroke);
                }
            }
            Gesture::Gradient { start } => {
                if let Some(end) = end {
                    let a = self.doc_to_screen(*start, rect);
                    let b = self.doc_to_screen(end, rect);
                    painter.line_segment([a, b], stroke);
                    painter.circle_filled(a, 3.0, Color32::WHITE);
                    painter.circle_filled(b, 3.0, Color32::WHITE);
                }
            }
            Gesture::Lasso { points } => {
                self.paint_polyline(painter, rect, points, end, stroke);
            }
            Gesture::Marquee { start, .. } if self.gesture_tool == Tool::Ellipse => {
                if let Some(end) = end {
                    let a = self.doc_to_screen((start.0.min(end.0), start.1.min(end.1)), rect);
                    let b = self.doc_to_screen((start.0.max(end.0), start.1.max(end.1)), rect);
                    let bounds = Rect::from_min_max(a, b);
                    painter.add(egui::Shape::ellipse_stroke(bounds.center(), bounds.size() * 0.5, stroke));
                }
            }
            _ => {}
        }
        if self.poly_points.len() >= 1 {
            self.paint_polyline(painter, rect, &self.poly_points, end, stroke);
        }
    }

    fn paint_shape_preview(&self, painter: &egui::Painter, rect: Rect, start: (f32, f32), end: (f32, f32), stroke: egui::Stroke) {
        match self.shape {
            ShapeKind::Line => {
                painter.line_segment([self.doc_to_screen(start, rect), self.doc_to_screen(end, rect)], stroke);
            }
            ShapeKind::Ellipse => {
                let a = self.doc_to_screen((start.0.min(end.0), start.1.min(end.1)), rect);
                let b = self.doc_to_screen((start.0.max(end.0), start.1.max(end.1)), rect);
                let bounds = Rect::from_min_max(a, b);
                painter.add(egui::Shape::ellipse_stroke(bounds.center(), bounds.size() * 0.5, stroke));
            }
            ShapeKind::Rectangle => {
                let a = self.doc_to_screen((start.0.min(end.0), start.1.min(end.1)), rect);
                let b = self.doc_to_screen((start.0.max(end.0), start.1.max(end.1)), rect);
                painter.rect_stroke(Rect::from_min_max(a, b), self.corner_radius * self.zoom, stroke, egui::StrokeKind::Outside);
            }
        }
    }

    fn paint_polyline(&self, painter: &egui::Painter, rect: Rect, points: &[(f32, f32)], end: Option<(f32, f32)>, stroke: egui::Stroke) {
        let mut screen: Vec<Pos2> = points.iter().copied().map(|p| self.doc_to_screen(p, rect)).collect();
        if let Some(end) = end {
            screen.push(self.doc_to_screen(end, rect));
        }
        if screen.len() >= 2 {
            painter.add(egui::Shape::line(screen.clone(), stroke));
        }
        for point in &screen {
            painter.circle_filled(*point, 2.5, Color32::WHITE);
        }
    }

    fn paint_clone_mark(&self, painter: &egui::Painter, rect: Rect) {
        let Some(mark) = self.clone_mark else { return };
        if self.tool != Tool::Clone {
            return;
        }
        let p = self.doc_to_screen(mark, rect);
        let stroke = egui::Stroke::new(1.0, Color32::from_rgb(255, 80, 80));
        painter.line_segment([p + vec2(-7.0, 0.0), p + vec2(7.0, 0.0)], stroke);
        painter.line_segment([p + vec2(0.0, -7.0), p + vec2(0.0, 7.0)], stroke);
    }

    fn refresh_contours(&mut self) {
        let Some(editor) = &self.editor else {
            self.contour_cache.clear();
            self.contour_key = 0;
            return;
        };
        let key = selection_key(&editor.selection);
        if key == self.contour_key {
            return;
        }
        self.contour_cache = editor.selection.contours();
        self.contour_key = key;
    }

    fn paint_selection(&self, painter: &egui::Painter, rect: Rect) {
        if matches!(self.gesture, Gesture::Marquee { .. }) && self.gesture_tool == Tool::Ellipse {
            return;
        }
        let stroke = egui::Stroke::new(1.25, Color32::from_rgb(90, 180, 255));
        for path in &self.contour_cache {
            if path.len() < 2 {
                continue;
            }
            let points: Vec<Pos2> = path.iter().copied().map(|p| self.doc_to_screen(p, rect)).collect();
            painter.add(egui::Shape::closed_line(points, stroke));
        }
    }

    fn paint_crop(&self, painter: &egui::Painter, rect: Rect) {
        let Some((x, y, w, h)) = self.crop else { return };
        let a = self.doc_to_screen((x, y), rect);
        let b = self.doc_to_screen((x + w, y + h), rect);
        let crop = Rect::from_min_max(a, b);
        painter.rect_filled(crop, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 20));
        painter.rect_stroke(crop, 0.0, egui::Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
    }

    fn paint_handles(&self, painter: &egui::Painter, rect: Rect) {
        let Some(transform) = self.active_transform() else { return };
        let corners = [
            (0.0, 0.0),
            (0.5, 0.0),
            (1.0, 0.0),
            (1.0, 0.5),
            (1.0, 1.0),
            (0.5, 1.0),
            (0.0, 1.0),
            (0.0, 0.5),
        ];
        let pts: Vec<Pos2> = corners.iter().map(|(u, v)| self.doc_to_screen(transform.unit_to_doc(*u, *v), rect)).collect();
        painter.add(egui::Shape::closed_line(pts.clone(), egui::Stroke::new(1.0, Color32::from_rgb(90, 170, 255))));
        for p in &pts {
            painter.rect_filled(Rect::from_center_size(*p, vec2(7.0, 7.0)), 1.0, Color32::WHITE);
        }
        let top = self.doc_to_screen(transform.unit_to_doc(0.5, 0.0), rect);
        let stem = top + vec2(0.0, -22.0);
        painter.line_segment([top, stem], egui::Stroke::new(1.0, Color32::from_rgb(90, 170, 255)));
        painter.circle_filled(stem, 5.0, Color32::WHITE);
    }

    fn paint_pixel_grid(&self, painter: &egui::Painter, rect: Rect) {
        let Some(ed) = &self.editor else { return };
        let (x0, y0) = self.screen_to_doc(rect.min, rect);
        let (x1, y1) = self.screen_to_doc(rect.max, rect);
        if (x1 - x0) > 80.0 || (y1 - y0) > 80.0 {
            return;
        }
        let color = Color32::from_rgba_unmultiplied(255, 255, 255, 40);
        let mut x = x0.floor();
        while x < x1 {
            let sx = self.doc_to_screen((x, 0.0), rect).x;
            painter.line_segment([pos2(sx, rect.top()), pos2(sx, rect.bottom())], egui::Stroke::new(1.0, color));
            x += 1.0;
        }
        let mut y = y0.floor();
        while y < y1 {
            let sy = self.doc_to_screen((0.0, y), rect).y;
            painter.line_segment([pos2(rect.left(), sy), pos2(rect.right(), sy)], egui::Stroke::new(1.0, color));
            y += 1.0;
        }
        let _ = ed;
    }

    fn paint_rulers(&self, painter: &egui::Painter, rect: Rect) {
        let h = 18.0;
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), h)), 0.0, Color32::from_rgb(30, 30, 30));
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(h, rect.height())), 0.0, Color32::from_rgb(30, 30, 30));
        let step = nice_step(80.0 / self.zoom);
        let (x0, y0) = self.screen_to_doc(rect.min, rect);
        let (x1, y1) = self.screen_to_doc(rect.max, rect);
        let mut x = (x0 / step).floor() * step;
        while x < x1 {
            let sx = self.doc_to_screen((x, 0.0), rect).x;
            painter.line_segment([pos2(sx, rect.top() + 10.0), pos2(sx, rect.top() + h)], egui::Stroke::new(1.0, Color32::from_rgb(160, 160, 160)));
            painter.text(pos2(sx + 2.0, rect.top() + 1.0), egui::Align2::LEFT_TOP, format!("{x:.0}"), egui::FontId::monospace(9.0), Color32::from_rgb(180, 180, 180));
            x += step;
        }
        let mut y = (y0 / step).floor() * step;
        while y < y1 {
            let sy = self.doc_to_screen((0.0, y), rect).y;
            painter.line_segment([pos2(rect.left() + 10.0, sy), pos2(rect.left() + h, sy)], egui::Stroke::new(1.0, Color32::from_rgb(160, 160, 160)));
            y += step;
        }
    }

    fn canvas_input(&mut self, ui: &mut egui::Ui, rect: Rect, response: &egui::Response) {
        if response.hovered() {
            let (scroll, zoom_delta, ctrl) = ui.input(|i| (i.smooth_scroll_delta(), i.zoom_delta(), i.modifiers.command));
            if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                let pinch = (zoom_delta - 1.0).abs() > 0.001;
                if pinch {
                    self.zoom_at(pointer, rect, zoom_delta);
                } else if ctrl && scroll.y.abs() > 0.0 {
                    self.zoom_at(pointer, rect, 1.0015_f32.powf(scroll.y));
                } else if scroll.length_sq() > 0.0 {
                    self.pan += scroll;
                }
            }
        }
        if let Some(pointer) = response.hover_pos().or(response.interact_pointer_pos()) {
            let (x, y) = self.screen_to_doc(pointer, rect);
            let moved = self.hover.is_none_or(|(px, py, _)| (px - x).abs() > 0.5 || (py - y).abs() > 0.5);
            if moved {
                self.hover = Some((x, y, self.sample_hover(x, y)));
            }
        }
        if !matches!(self.modal, Modal::None) {
            return;
        }
        let space = ui.input(|i| i.key_down(egui::Key::Space));
        let alt = ui.input(|i| i.modifiers.alt);
        let shift = ui.input(|i| i.modifiers.shift);
        let primary_pressed = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
        let primary_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
        let primary_released = ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
        let middle_pressed = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Middle));
        let middle_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
        let middle_released = ui.input(|i| i.pointer.button_released(egui::PointerButton::Middle));

        if (primary_pressed || middle_pressed) && response.hovered() {
            self.release_keyboard_focus(ui.ctx());
        }
        if matches!(self.gesture, Gesture::None) {
            if middle_pressed && response.hovered() {
                if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                    self.gesture_tool = Tool::Hand;
                    self.gesture = Gesture::Pan { last: pointer };
                }
            } else if primary_pressed && response.hovered() {
                if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                    let local = pointer - rect.min;
                    let tool = if space {
                        Tool::Hand
                    } else if alt && matches!(self.tool, Tool::Brush | Tool::Eraser) {
                        Tool::Eyedropper
                    } else {
                        self.tool
                    };
                    let on_ruler = self.show_rulers && (local.y < 18.0 || local.x < 18.0);
                    if on_ruler {
                        let horizontal = local.y < 18.0 && local.x >= 18.0;
                        self.gesture_tool = tool;
                        self.gesture = Gesture::GuidePending { horizontal, start: pointer };
                    } else {
                        let doc = self.screen_to_doc(pointer, rect);
                        self.begin_gesture(tool, doc, pointer, rect, shift, alt);
                    }
                }
            }
        }
        let just_pressed = primary_pressed || middle_pressed;
        let pointer_down = primary_down || middle_down;
        if !matches!(self.gesture, Gesture::None) && (pointer_down || primary_released || middle_released) && !just_pressed {
            let trail = self.pointer_trail(ui, rect);
            if let Gesture::GuidePending { horizontal, start } = self.gesture {
                if let Some(pointer) = trail.last().copied().or(ui.input(|i| i.pointer.interact_pos())) {
                    if (pointer - start).length() >= 4.0 {
                        self.begin_guide(horizontal, pointer, rect);
                    }
                }
            }
            if !matches!(self.gesture, Gesture::GuidePending { .. }) {
                let stroke = matches!(
                    self.gesture,
                    Gesture::Brush { .. } | Gesture::Clone { .. } | Gesture::Heal { .. } | Gesture::BlurPaint { .. } | Gesture::Liquify { .. }
                );
                if stroke && !trail.is_empty() {
                    for pointer in trail {
                        let doc = self.screen_to_doc(pointer, rect);
                        self.drag_gesture(doc, pointer, rect, shift, alt);
                    }
                } else if let Some(pointer) = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) {
                    let doc = self.screen_to_doc(pointer, rect);
                    self.drag_gesture(doc, pointer, rect, shift, alt);
                }
            }
        }
        if !matches!(self.gesture, Gesture::None) {
            ui.ctx().request_repaint();
        }
        let stop = match &self.gesture {
            Gesture::None => false,
            Gesture::Pan { .. } => (primary_released && !middle_down) || (middle_released && !primary_down),
            Gesture::GuidePending { .. } => primary_released,
            _ => primary_released,
        };
        if stop {
            let doc = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()))
                .map(|pointer| self.screen_to_doc(pointer, rect))
                .unwrap_or((0.0, 0.0));
            self.end_gesture(doc, shift);
        }
        self.update_cursor(ui, response, space, rect);
        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing && ui.input(|i| i.key_pressed(egui::Key::Enter)) && self.tool == Tool::Crop {
            self.apply_crop();
        }
        if !typing && ui.input(|i| i.key_pressed(egui::Key::Enter)) && self.tool == Tool::Poly && self.poly_points.len() >= 3 {
            let points = std::mem::take(&mut self.poly_points);
            let next = edit::polygon_selection(&points);
            self.commit_selection(next, false, false);
        }
        if self.editor.is_some() && !self.stroke_active() && !self.preview_covers(rect) && !self.compositing {
            self.request_composite();
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            let was_drag = self.live_drag_on;
            self.clear_live_drag();
            if was_drag {
                self.request_composite();
            }
            self.crop = None;
            self.gesture = Gesture::None;
            self.poly_points.clear();
            if let Some(editor) = self.editor.as_mut() {
                editor.selection = Selection::None;
            }
        }
    }

    fn update_cursor(&self, ui: &mut egui::Ui, response: &egui::Response, space: bool, rect: Rect) {
        let transforming = self.transform_gesture() || matches!(self.gesture, Gesture::Distort { .. });
        if !response.hovered() && !transforming && !matches!(self.gesture, Gesture::Pan { .. }) {
            return;
        }
        let pointer = ui.input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
        let icon = if matches!(self.gesture, Gesture::Pan { .. }) {
            CursorIcon::Grabbing
        } else if space || self.tool == Tool::Hand {
            CursorIcon::Grab
        } else if self.tool == Tool::Zoom {
            if ui.input(|i| i.modifiers.alt) { CursorIcon::ZoomOut } else { CursorIcon::ZoomIn }
        } else if self.tool == Tool::Move || transforming {
            pointer.map(|pos| self.transform_cursor(pos, rect)).unwrap_or(CursorIcon::Move)
        } else if matches!(self.tool, Tool::Type) {
            CursorIcon::Text
        } else {
            CursorIcon::Crosshair
        };
        ui.ctx().set_cursor_icon(icon);
    }

    fn transform_cursor(&self, pointer: Pos2, rect: Rect) -> CursorIcon {
        let doc = self.screen_to_doc(pointer, rect);
        match &self.gesture {
            Gesture::Scale { handle, start, .. } => return cursor_for_handle(*handle, *start, |p| self.doc_to_screen(p, rect)),
            Gesture::Rotate { .. } => return CursorIcon::Alias,
            Gesture::Distort { .. } => return CursorIcon::Crosshair,
            Gesture::Move { .. } => return CursorIcon::Move,
            _ => {}
        }
        let Some(transform) = self.active_transform() else {
            return CursorIcon::Move;
        };
        let handles = handle_points(transform);
        for (index, point) in handles.into_iter().enumerate() {
            if dist(point, doc) < 10.0 / self.zoom.max(0.05) {
                return cursor_for_handle(index, transform, |p| self.doc_to_screen(p, rect));
            }
        }
        let top = transform.unit_to_doc(0.5, 0.0);
        let stem = (top.0, top.1 - 22.0 / self.zoom.max(0.05));
        if dist(stem, doc) < 12.0 / self.zoom.max(0.05) {
            return CursorIcon::Alias;
        }
        CursorIcon::Move
    }

    fn release_keyboard_focus(&self, ctx: &egui::Context) {
        if let Some(id) = ctx.memory(|mem| mem.focused()) {
            ctx.memory_mut(|mem| mem.surrender_focus(id));
        }
    }

    fn zoom_at(&mut self, pointer: Pos2, rect: Rect, factor: f32) {
        if !factor.is_finite() || (factor - 1.0).abs() < 0.0001 {
            return;
        }
        let before = self.screen_to_doc(pointer, rect);
        self.zoom = (self.zoom * factor).clamp(0.02, 64.0);
        self.pan = pointer.to_vec2() - rect.min.to_vec2() - vec2(before.0, before.1) * self.zoom;
    }

    fn zoom_by(&mut self, factor: f32) {
        let Some(rect) = self.canvas_rect else {
            self.zoom = (self.zoom * factor).clamp(0.02, 64.0);
            return;
        };
        self.zoom_at(rect.center(), rect, factor);
    }

    fn zoom_center(&mut self, zoom: f32) {
        let factor = if self.zoom > 0.0001 { zoom / self.zoom } else { 1.0 };
        self.zoom_by(factor);
    }

    fn set_tool(&mut self, tool: Tool) {
        if self.tool != tool {
            self.poly_points.clear();
        }
        self.tool = tool;
    }

    fn stroke_active(&self) -> bool {
        matches!(
            self.gesture,
            Gesture::Brush { .. } | Gesture::Clone { .. } | Gesture::Heal { .. } | Gesture::BlurPaint { .. } | Gesture::Liquify { .. }
        )
    }

    fn begin_gesture(&mut self, tool: Tool, doc: (f32, f32), pointer: Pos2, rect: Rect, shift: bool, alt: bool) {
        self.gesture_tool = tool;
        match tool {
            Tool::Hand => {
                self.gesture = Gesture::Pan { last: pointer };
            }
            Tool::Move => self.begin_move(doc, alt),
            Tool::Brush | Tool::Eraser => {
                let start = if shift { self.last_dab.unwrap_or(doc) } else { doc };
                self.begin_stroke();
                self.gesture = Gesture::Brush { last: start };
                self.paint_at(start, tool == Tool::Eraser);
                if start != doc {
                    self.stroke_between(start, doc, tool == Tool::Eraser);
                }
                self.last_dab = Some(doc);
            }
            Tool::Marquee | Tool::Ellipse => {
                self.gesture = Gesture::Marquee { start: doc, add: shift, subtract: alt };
            }
            Tool::Lasso => self.gesture = Gesture::Lasso { points: vec![doc] },
            Tool::Crop => self.gesture = Gesture::Crop { start: doc },
            Tool::Shape => self.gesture = Gesture::Shape { start: doc },
            Tool::Gradient => self.gesture = Gesture::Gradient { start: doc },
            Tool::Eyedropper => {
                self.sample_color(doc);
                self.gesture = Gesture::Sample;
            }
            Tool::Wand => self.wand_at(doc),
            Tool::Bucket => self.bucket_at(doc),
            Tool::Type => {
                self.modal = Modal::Text { content: String::new(), at: doc };
                self.gesture = Gesture::None;
            }
            Tool::Zoom => {
                self.zoom_at(pointer, rect, if alt { 1.0 / 1.25 } else { 1.25 });
                self.gesture = Gesture::None;
            }
            Tool::Clone => self.begin_clone(doc, alt),
            Tool::Heal => {
                self.begin_stroke();
                self.gesture = Gesture::Heal { last: doc };
                self.heal_stroke(doc, doc);
            }
            Tool::Blur => {
                self.begin_stroke();
                self.gesture = Gesture::BlurPaint { last: doc };
                self.blur_stroke(doc, doc);
            }
            Tool::Liquify => {
                self.begin_stroke();
                self.gesture = Gesture::Liquify { last: doc };
            }
            Tool::Poly => {
                self.poly_points.push(doc);
                self.gesture = Gesture::None;
            }
        }
    }

    fn child_transforms(&self) -> Vec<(uuid::Uuid, Transform)> {
        let Some(ed) = &self.editor else { return Vec::new() };
        let Some(id) = ed.document.active_layer_id else { return Vec::new() };
        ed.document.descendant_ids(id).into_iter().filter_map(|child| {
            ed.document.layer(child).map(|layer| (child, layer.transform))
        }).collect()
    }

    fn apply_group_transform(&mut self, next: Transform, start: Transform, children: &[(uuid::Uuid, Transform)]) {
        if let Some(ed) = self.editor.as_mut() {
            if let Some(layer) = ed.document.active_mut() {
                layer.transform = next;
                layer.touch();
            }
            for (id, child) in children {
                if let Some(layer) = ed.document.layer_mut(*id) {
                    layer.transform = Document::follow_child(start, next, *child);
                    layer.touch();
                }
            }
        }
    }

    fn begin_guide(&mut self, horizontal: bool, pointer: Pos2, rect: Rect) {
        let (x, y) = self.screen_to_doc(pointer, rect);
        let axis = if horizontal { GuideAxis::Horizontal } else { GuideAxis::Vertical };
        let position = if horizontal { y } else { x };
        let id = uuid::Uuid::new_v4();
        self.begin_stroke();
        if let Some(ed) = self.editor.as_mut() {
            ed.document.guides.push(Guide { id, axis, position });
        }
        self.gesture = Gesture::Guide { id, axis };
    }

    fn begin_clone(&mut self, doc: (f32, f32), alt: bool) {
        if alt {
            self.clone_mark = Some(doc);
            self.clone_offset = None;
            self.note("已设置仿制源", "Clone source set");
            return;
        }
        if self.clone_mark.is_none() {
            self.note("按 Alt 点击设置仿制源", "Alt-click to set the clone source");
            return;
        }
        let mark = self.clone_mark.unwrap();
        if !self.clone_aligned || self.clone_offset.is_none() {
            self.clone_offset = Some((mark.0 - doc.0, mark.1 - doc.1));
        }
        let delta = self.clone_offset.unwrap_or((0.0, 0.0));
        self.begin_stroke();
        self.gesture = Gesture::Clone { last: doc, delta };
        self.clone_stroke(doc, doc, delta);
    }

    fn begin_move(&mut self, doc: (f32, f32), alt: bool) {
        let Some(transform) = self.active_transform() else {
            if let Some(id) = self.editor.as_ref().and_then(|ed| ed.document.hit_test(doc.0, doc.1)) {
                if let Some(ed) = self.editor.as_mut() {
                    ed.document.active_layer_id = Some(id);
                }
            }
            return;
        };
        let handles = handle_points(transform);
        for (i, point) in handles.into_iter().enumerate() {
            if dist(point, doc) < 10.0 / self.zoom.max(0.05) {
                if alt && matches!(i, 0 | 2 | 4 | 6) {
                    let corners = [
                        transform.unit_to_doc(0.0, 0.0),
                        transform.unit_to_doc(1.0, 0.0),
                        transform.unit_to_doc(1.0, 1.0),
                        transform.unit_to_doc(0.0, 1.0),
                    ];
                    let index = [0, 0, 1, 0, 2, 0, 3, 0][i];
                    self.gesture = Gesture::Distort { index, corners };
                    return;
                }
                let children = self.child_transforms();
                self.gesture = Gesture::Scale { handle: i, start: transform, children };
                return;
            }
        }
        let top = transform.unit_to_doc(0.5, 0.0);
        let stem = (top.0, top.1 - 22.0 / self.zoom.max(0.05));
        if dist(stem, doc) < 12.0 / self.zoom.max(0.05) {
            let (cx, cy) = transform.center();
            let children = self.child_transforms();
            self.gesture = Gesture::Rotate {
                start_angle: (doc.1 - cy).atan2(doc.0 - cx),
                start_rotation: transform.rotation,
                start: transform,
                children,
            };
            return;
        }
        let hit = self.editor.as_ref().and_then(|ed| ed.document.hit_test(doc.0, doc.1));
        if let Some(id) = hit {
            if let Some(ed) = self.editor.as_mut() {
                ed.document.active_layer_id = Some(id);
            }
        }
        if transform.contains(doc.0, doc.1) || hit == self.editor.as_ref().and_then(|ed| ed.document.active_layer_id) {
            let followers = self.editor.as_ref().map(|ed| {
                let id = ed.document.active_layer_id;
                id.map(|id| {
                    ed.document.descendant_ids(id).into_iter().filter_map(|child| {
                        ed.document.layer(child).map(|layer| (child, layer.transform.origin_x, layer.transform.origin_y))
                    }).collect()
                }).unwrap_or_default()
            }).unwrap_or_default();
            self.gesture = Gesture::Move { origin: (transform.origin_x, transform.origin_y), start: doc, followers };
        }
    }

    fn drag_gesture(&mut self, doc: (f32, f32), pointer: Pos2, rect: Rect, shift: bool, alt: bool) {
        if matches!(self.gesture, Gesture::Move { .. } | Gesture::Scale { .. } | Gesture::Rotate { .. } | Gesture::Distort { .. }) {
            self.begin_stroke();
        }
        match &mut self.gesture {
            Gesture::Pan { last } => {
                let delta = pointer - *last;
                self.pan += delta;
                *last = pointer;
            }
            Gesture::Move { origin, start, followers } => {
                let (ox, oy) = *origin;
                let mut dx = doc.0 - start.0;
                let mut dy = doc.1 - start.1;
                if self.snap {
                    let mut nx = ox + dx;
                    let mut ny = oy + dy;
                    let threshold = 6.0 / self.zoom.max(0.05);
                    nx = (nx / self.grid).round() * self.grid;
                    ny = (ny / self.grid).round() * self.grid;
                    if let Some(ed) = &self.editor {
                        for guide in &ed.document.guides {
                            match guide.axis {
                                GuideAxis::Vertical if (guide.position - nx).abs() < threshold => nx = guide.position,
                                GuideAxis::Horizontal if (guide.position - ny).abs() < threshold => ny = guide.position,
                                _ => {}
                            }
                        }
                        if nx.abs() < threshold { nx = 0.0; }
                        if ny.abs() < threshold { ny = 0.0; }
                    }
                    dx = nx - ox;
                    dy = ny - oy;
                }
                let followers = followers.clone();
                if let Some(ed) = self.editor.as_mut() {
                    if let Some(layer) = ed.document.active_mut() {
                        layer.transform.origin_x = ox + dx;
                        layer.transform.origin_y = oy + dy;
                        layer.touch();
                    }
                    for (id, fx, fy) in followers {
                        if let Some(layer) = ed.document.layer_mut(id) {
                            layer.transform.origin_x = fx + dx;
                            layer.transform.origin_y = fy + dy;
                            layer.touch();
                        }
                    }
                }
            }
            Gesture::Scale { handle, start, children } => {
                let next = scale_handle(*start, *handle, doc, shift);
                let origin = *start;
                let children = children.clone();
                if let Some(ed) = self.editor.as_mut() {
                    if let Some(layer) = ed.document.active_mut() {
                        layer.transform = next;
                        layer.touch();
                    }
                    for (id, child) in children {
                        if let Some(layer) = ed.document.layer_mut(id) {
                            layer.transform = Document::follow_child(origin, next, child);
                            layer.touch();
                        }
                    }
                }
            }
            Gesture::Rotate { start_angle, start_rotation, start, children } => {
                let origin = *start;
                let mut next = origin;
                let (cx, cy) = origin.center();
                let angle = (doc.1 - cy).atan2(doc.0 - cx);
                next.rotation = *start_rotation + (angle - *start_angle).to_degrees();
                if shift {
                    next.rotation = (next.rotation / 15.0).round() * 15.0;
                }
                let children = children.clone();
                if let Some(ed) = self.editor.as_mut() {
                    if let Some(layer) = ed.document.active_mut() {
                        layer.transform = next;
                        layer.touch();
                    }
                    for (id, child) in children {
                        if let Some(layer) = ed.document.layer_mut(id) {
                            layer.transform = Document::follow_child(origin, next, child);
                            layer.touch();
                        }
                    }
                }
            }
            Gesture::Brush { last } => {
                let from = *last;
                *last = doc;
                let erase = self.gesture_tool == Tool::Eraser;
                self.stroke_between(from, doc, erase);
            }
            Gesture::Marquee { start, .. } => {
                let (x0, y0) = *start;
                let mut x = x0.min(doc.0);
                let mut y = y0.min(doc.1);
                let mut w = (doc.0 - x0).abs();
                let mut h = (doc.1 - y0).abs();
                if shift {
                    let side = w.max(h);
                    w = side;
                    h = side;
                }
                if self.snap {
                    x = (x / self.grid).round() * self.grid;
                    y = (y / self.grid).round() * self.grid;
                }
                if self.gesture_tool == Tool::Ellipse {
                    return;
                }
                if let Some(ed) = self.editor.as_mut() {
                    ed.selection = Selection::Rect { x, y, w, h };
                }
            }
            Gesture::Lasso { points } => points.push(doc),
            Gesture::Crop { start } => {
                let (x0, y0) = *start;
                let mut x = x0.min(doc.0);
                let mut y = y0.min(doc.1);
                let mut w = (doc.0 - x0).abs().max(1.0);
                let mut h = (doc.1 - y0).abs().max(1.0);
                if self.crop_ratio.0 > 0.0 && self.crop_ratio.1 > 0.0 {
                    let ratio = self.crop_ratio.0 / self.crop_ratio.1;
                    if w / h > ratio { w = h * ratio; } else { h = w / ratio; }
                    if doc.0 < x0 { x = x0 - w; }
                    if doc.1 < y0 { y = y0 - h; }
                }
                if alt {
                    x = x0 - w;
                    y = y0 - h;
                    w *= 2.0;
                    h *= 2.0;
                }
                self.crop = Some((x, y, w, h));
            }
            Gesture::Heal { last } => {
                let from = *last;
                *last = doc;
                self.heal_stroke(from, doc);
            }
            Gesture::BlurPaint { last } => {
                let from = *last;
                *last = doc;
                self.blur_stroke(from, doc);
            }
            Gesture::Distort { index, corners } => {
                corners[*index] = doc;
            }
            Gesture::Sample => self.sample_color(doc),
            Gesture::Liquify { last } => {
                let from = *last;
                *last = doc;
                self.liquify_stroke(from, doc);
            }
            Gesture::Clone { last, delta } => {
                let from = *last;
                let delta = *delta;
                *last = doc;
                self.clone_stroke(from, doc, delta);
            }
            Gesture::Guide { id, axis } => {
                let id = *id;
                let axis = *axis;
                let position = match axis {
                    GuideAxis::Horizontal => doc.1,
                    GuideAxis::Vertical => doc.0,
                };
                if let Some(guide) = self.editor.as_mut().and_then(|ed| ed.document.guides.iter_mut().find(|guide| guide.id == id)) {
                    guide.position = position;
                }
            }
            Gesture::Shape { .. } | Gesture::Gradient { .. } | Gesture::GuidePending { .. } | Gesture::None => {
                let _ = (pointer, rect);
            }
        }
    }

    fn end_gesture(&mut self, doc: (f32, f32), shift: bool) {
        if self.live_drag_on {
            self.live_hold = true;
            self.live_drag_on = false;
        }
        let edited = self.editor.as_ref().is_some_and(|ed| ed.stroke);
        let gesture = std::mem::replace(&mut self.gesture, Gesture::None);
        if let Some(ed) = self.editor.as_mut() {
            ed.stroke = false;
        }
        match gesture {
            Gesture::GuidePending { .. } => {}
            Gesture::Marquee { start, add, subtract } => {
                let (x0, y0) = start;
                let x = x0.min(doc.0);
                let y = y0.min(doc.1);
                let mut w = (doc.0 - x0).abs();
                let mut h = (doc.1 - y0).abs();
                if shift {
                    let side = w.max(h);
                    w = side;
                    h = side;
                }
                if w < 2.0 && h < 2.0 && !add && !subtract {
                    if let Some(ed) = self.editor.as_mut() {
                        ed.selection = Selection::None;
                    }
                    return;
                }
                let next = if self.gesture_tool == Tool::Ellipse {
                    edit::ellipse_selection(x, y, w, h)
                } else {
                    Selection::Rect { x, y, w, h }
                };
                self.commit_selection(next, add, subtract);
            }
            Gesture::Lasso { points } => {
                if points.len() >= 3 {
                    let next = edit::polygon_selection(&points);
                    self.commit_selection(next, false, false);
                }
            }
            Gesture::Shape { start } => {
                if (doc.0 - start.0).hypot(doc.1 - start.1) >= 2.0 {
                    self.commit_shape(start, doc, shift);
                }
            }
            Gesture::Gradient { start } => {
                if (doc.0 - start.0).hypot(doc.1 - start.1) >= 2.0 {
                    self.commit_gradient(start, doc);
                }
            }
            Gesture::Guide { id, .. } => {
                let inside = self.editor.as_ref().is_some_and(|ed| {
                    ed.document.guides.iter().find(|guide| guide.id == id).is_some_and(|guide| {
                        match guide.axis {
                            GuideAxis::Vertical => guide.position >= -20.0 && guide.position <= ed.document.width as f32 + 20.0,
                            GuideAxis::Horizontal => guide.position >= -20.0 && guide.position <= ed.document.height as f32 + 20.0,
                        }
                    })
                });
                if !inside {
                    if let Some(ed) = self.editor.as_mut() {
                        ed.document.guides.retain(|guide| guide.id != id);
                    }
                }
                self.request_composite();
            }
            Gesture::Distort { corners, .. } => {
                self.finish_distort(corners);
            }
            Gesture::Pan { .. } | Gesture::Sample => {}
            _ => {
                if edited {
                    self.request_composite();
                }
            }
        }
    }

    fn commit_selection(&mut self, next: Selection, add: bool, subtract: bool) {
        let Some(ed) = self.editor.as_mut() else { return };
        let w = ed.document.width;
        let h = ed.document.height;
        if !add && !subtract {
            ed.selection = next;
            return;
        }
        let current = ed.selection.clone();
        ed.selection = combine_selection(current, next, w, h, subtract);
    }

    fn commit_shape(&mut self, start: (f32, f32), end: (f32, f32), shift: bool) {
        let mut x = start.0.min(end.0);
        let mut y = start.1.min(end.1);
        let mut w = (end.0 - start.0).abs().max(1.0);
        let mut h = (end.1 - start.1).abs().max(1.0);
        if shift && !matches!(self.shape, ShapeKind::Line) {
            let side = w.max(h);
            w = side;
            h = side;
        }
        let color = [self.fg[0], self.fg[1], self.fg[2], 255];
        let kind = self.shape;
        let line_width = self.line_width;
        let image = if matches!(kind, ShapeKind::Line) {
            line_raster(start, end, line_width, color)
        } else {
            edit::rasterize_shape(kind, w.round() as u32, h.round() as u32, color, self.corner_radius, line_width)
        };
        if matches!(kind, ShapeKind::Line) {
            x = start.0.min(end.0) - line_width;
            y = start.1.min(end.1) - line_width;
            w = image.width as f32;
            h = image.height as f32;
        }
        self.mutate(|ed| {
            let mut layer = Layer::with_image(if matches!(kind, ShapeKind::Line) { "Line" } else { "Shape" }, image);
            layer.transform.origin_x = x;
            layer.transform.origin_y = y;
            layer.transform.width = w;
            layer.transform.height = h;
            ed.document.add_layer(layer);
        });
    }

    fn commit_gradient(&mut self, start: (f32, f32), end: (f32, f32)) {
        let fg = self.fg;
        let bg = self.bg;
        self.mutate(|ed| {
            let width = ed.document.width;
            let height = ed.document.height;
            let Some(layer) = ed.document.active_mut() else { return };
            if layer.is_group || layer.adjustment.is_some() {
                return;
            }
            if layer.image.is_none() {
                layer.image = Some(Raster::new(width, height, [0, 0, 0, 0]));
                layer.transform = Transform::identity(width as f32, height as f32);
            }
            let transform = layer.transform;
            let Some(image) = layer.image.as_mut() else { return };
            let to_px = |p: (f32, f32)| {
                let (u, v) = transform.doc_to_unit(p.0, p.1);
                (u * image.width as f32, v * image.height as f32)
            };
            let a = to_px(start);
            let b = to_px(end);
            paint_gradient(image, a, b, fg, bg);
            layer.rasterize_metadata();
        });
    }

    fn begin_stroke(&mut self) {
        let mut started = false;
        if let Some(ed) = self.editor.as_mut() {
            if !ed.stroke {
                ed.push_undo();
                ed.stroke = true;
                started = true;
            }
        }
        if started {
            self.generation = self.generation.wrapping_add(1);
        }
    }

    fn finish_distort(&mut self, corners: [(f32, f32); 4]) {
        self.mutate(|ed| {
            let Some(layer) = ed.document.active_mut() else { return };
            let Some(image) = layer.image.clone() else { return };
            let warped = crate::ops::warp_quad(&image, corners);
            let min_x = corners.iter().map(|p| p.0).fold(f32::MAX, f32::min);
            let min_y = corners.iter().map(|p| p.1).fold(f32::MAX, f32::min);
            layer.transform.origin_x = min_x;
            layer.transform.origin_y = min_y;
            layer.transform.width = warped.width as f32;
            layer.transform.height = warped.height as f32;
            layer.transform.rotation = 0.0;
            layer.image = Some(warped);
            layer.rasterize_metadata();
        });
    }

    fn nudge(&mut self, dx: f32, dy: f32) {
        if matches!(self.tool, Tool::Marquee | Tool::Ellipse | Tool::Lasso | Tool::Poly) {
            if let Some(ed) = self.editor.as_mut() {
                ed.selection = crate::ops::offset_selection(std::mem::replace(&mut ed.selection, Selection::None), dx, dy);
            }
            return;
        }
        self.mutate(|ed| {
            if let Some(layer) = ed.document.active_mut() {
                layer.transform.origin_x += dx;
                layer.transform.origin_y += dy;
                layer.touch();
            }
        });
    }

    fn select_subject(&mut self) {
        if let Some(ed) = &self.editor {
            let selection = crate::ops::select_subject(&ed.document);
            if let Some(ed) = self.editor.as_mut() {
                ed.selection = selection;
            }
            self.note("已按边缘选择主体", "Subject selected from the border");
        }
    }

    fn auto_levels(&mut self) {
        let Some(ed) = &self.editor else { return };
        let range = crate::ops::auto_levels(&ed.document);
        self.add_adjustment(Adjustment::Levels { ranges: [range, LevelRange::default(), LevelRange::default(), LevelRange::default()] });
    }

    fn trim_canvas(&mut self) {
        let Some(bounds) = self.editor.as_ref().and_then(|ed| crate::ops::trim_bounds(&ed.document)) else { return };
        self.mutate(|ed| {
            ed.document.crop(bounds.0, bounds.1, bounds.2, bounds.3);
            ed.selection = Selection::None;
        });
    }

    fn remove_background(&mut self) {
        self.mutate(|ed| {
            let selection = ed.selection.clone();
            let _ = edit::apply_to_active(&mut ed.document, &selection, crate::ops::remove_background);
        });
    }

    fn filter_bloom(&mut self) {
        self.spawn_pixel_filter(|image| crate::ops::bloom(image, 0.8));
    }

    fn filter_tonal(&mut self) {
        self.spawn_pixel_filter(|image| crate::ops::tonal_contrast(image, 0.8));
    }

    fn filter_lens(&mut self) {
        self.spawn_pixel_filter(|image| crate::ops::lens_correct(image, -0.2));
    }

    fn spawn_pixel_filter(&mut self, op: impl FnOnce(&mut Raster) + Send + 'static) {
        if self.filter_rx.is_some() {
            self.note("滤镜还在处理", "A filter is still running");
            return;
        }
        let Some(ed) = self.editor.as_mut() else {
            self.note("还没有打开工程", "No document open");
            return;
        };
        if !ed.stroke {
            ed.push_undo();
        }
        let Some(layer) = ed.document.active() else {
            self.note("没有活动图层", "No active layer");
            return;
        };
        if layer.is_group || layer.adjustment.is_some() {
            self.note("这个图层不能直接应用滤镜", "This layer cannot take a filter directly");
            return;
        }
        let Some(image) = layer.image.clone() else {
            self.note("图层没有像素", "Layer has no pixels");
            return;
        };
        let id = layer.id;
        let selection = ed.selection.clone();
        let transform = layer.transform;
        let (tx, rx) = std::sync::mpsc::channel();
        self.filter_rx = Some(rx);
        self.note("正在应用滤镜…", "Applying filter…");
        std::thread::spawn(move || {
            let mut image = image;
            apply_filter_image(&mut image, &selection, transform, op);
            let _ = tx.send((id, image));
        });
    }

    fn poll_filter(&mut self) {
        let finished = self.filter_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        let Some((id, image)) = finished else {
            return;
        };
        self.filter_rx = None;
        if let Some(layer) = self.editor.as_mut().and_then(|ed| ed.document.layer_mut(id)) {
            layer.image = Some(image);
            layer.rasterize_metadata();
        }
        if let Some(ed) = self.editor.as_mut() {
            ed.dirty = true;
        }
        self.request_composite();
        self.note("滤镜已应用", "Filter applied");
    }

    fn cut_selection_to_layer(&mut self) {
        self.mutate(|ed| {
            let selection = ed.selection.clone();
            if matches!(selection, Selection::None) || selection.is_empty() {
                return;
            }
            let width = ed.document.width;
            let height = ed.document.height;
            let Some(layer) = ed.document.active_mut() else { return };
            let Some(image) = layer.image.as_mut() else { return };
            let iw = image.width;
            let ih = image.height;
            let transform = layer.transform;
            let mut cut = Raster::new(iw, ih, [0, 0, 0, 0]);
            for y in 0..ih {
                for x in 0..iw {
                    let u = (x as f32 + 0.5) / iw as f32;
                    let v = (y as f32 + 0.5) / ih as f32;
                    let (dx, dy) = transform.unit_to_doc(u, v);
                    if selection.coverage_at(dx, dy) > 0.5 {
                        cut.set_pixel(x as i32, y as i32, image.pixel(x as i32, y as i32));
                        image.set_pixel(x as i32, y as i32, [0, 0, 0, 0]);
                    }
                }
            }
            layer.rasterize_metadata();
            let mut next = Layer::with_image("Selection", cut);
            next.transform = transform;
            let _ = (width, height);
            ed.document.add_layer(next);
        });
    }

    fn merge_group(&mut self) {
        let mut err: Option<String> = None;
        self.mutate(|ed| {
            let Some(id) = ed.document.active_layer_id else { return };
            let Some(layer) = ed.document.layer(id) else { return };
            if !layer.is_group {
                err = Some("请先选中一个组".into());
                return;
            }
            let ids = ed.document.descendant_ids(id);
            if ids.is_empty() {
                return;
            }
            let mut ghost = ed.document.clone();
            for layer in &mut ghost.layers {
                layer.visible = ids.contains(&layer.id);
            }
            let flat = crate::render::composite_document(&ghost, 1.0).image;
            let mut merged = Layer::with_image("Group", flat);
            merged.transform = Transform::identity(ed.document.width as f32, ed.document.height as f32);
            let new_id = merged.id;
            ed.document.layers.push(merged);
            ed.document.delete_layer(id);
            ed.document.active_layer_id = Some(new_id);
        });
        if let Some(err) = err {
            self.fail(err);
        }
    }

    fn copy_layer(&mut self) {
        if let Some(layer) = self.editor.as_ref().and_then(|ed| ed.document.active()).cloned() {
            self.layer_clip = Some(layer);
            self.note("已复制图层", "Layer copied");
        }
    }

    fn paste_copied_layer(&mut self) {
        let Some(mut layer) = self.layer_clip.clone() else {
            self.note("剪贴板里没有图层", "No copied layer");
            return;
        };
        layer.id = uuid::Uuid::new_v4();
        layer.name = format!("{} copy", layer.name);
        layer.parent_id = None;
        self.mutate(|ed| {
            ed.document.add_layer(layer);
        });
    }

    fn blur_stroke(&mut self, from: (f32, f32), to: (f32, f32)) {
        self.retouch_stroke(from, to, true);
    }

    fn liquify_stroke(&mut self, from: (f32, f32), to: (f32, f32)) {
        self.retouch_stroke(from, to, false);
    }

    fn retouch_stroke(&mut self, from: (f32, f32), to: (f32, f32), blur: bool) {
        let Some(layer_id) = self.editor.as_ref().and_then(|ed| ed.document.active_layer_id) else { return };
        let Some(source) = self.editor.as_ref().and_then(|ed| {
            ed.undo.last().and_then(|snap| snap.document.layer(layer_id)).and_then(|layer| layer.image.clone())
        }) else { return };
        let size = self.brush_size;
        let Some(ed) = self.editor.as_mut() else { return };
        let Some(layer) = ed.document.active_mut() else { return };
        let Some(image) = layer.image.as_mut() else { return };
        let transform = layer.transform;
        let map = |p: (f32, f32)| {
            let (u, v) = transform.doc_to_unit(p.0, p.1);
            (u * image.width as f32, v * image.height as f32)
        };
        let a = map(from);
        let b = map(to);
        let radius = size * image.width as f32 / transform.width.max(1.0);
        if blur {
            crate::ops::blur_dab(image, &source, b.0, b.1, radius);
        } else {
            crate::ops::liquify(image, &source, a, b, radius);
        }
        layer.rasterize_metadata();
        self.patch_preview(from, to, size);
    }

    fn heal_stroke(&mut self, from: (f32, f32), to: (f32, f32)) {
        let Some(layer_id) = self.editor.as_ref().and_then(|ed| ed.document.active_layer_id) else { return };
        let Some(source) = self.editor.as_ref().and_then(|ed| {
            ed.undo.last().and_then(|snap| snap.document.layer(layer_id)).and_then(|layer| layer.image.clone())
        }) else { return };
        let size = self.brush_size;
        let hardness = self.brush_hardness;
        let Some(ed) = self.editor.as_mut() else { return };
        let width = ed.document.width;
        let height = ed.document.height;
        let Some(layer) = ed.document.active_mut() else { return };
        if layer.is_group || layer.adjustment.is_some() {
            return;
        }
        if layer.image.is_none() {
            layer.image = Some(Raster::new(width, height, [0, 0, 0, 0]));
            layer.transform = Transform::identity(width as f32, height as f32);
        }
        let transform = layer.transform;
        let Some(image) = layer.image.as_mut() else { return };
        let iw = image.width.max(1);
        let ih = image.height.max(1);
        let radius = (size * iw as f32 / transform.width.max(1.0)).max(1.0);
        let map = |p: (f32, f32)| {
            let (u, v) = transform.doc_to_unit(p.0, p.1);
            (u * iw as f32, v * ih as f32)
        };
        let a = map(from);
        let b = map(to);
        let dist = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let step = (radius * 0.35).max(1.0);
        let n = (dist / step).ceil().max(1.0) as i32;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let center = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            let r = radius.ceil() as i32 + 1;
            for y in (center.1 as i32 - r).max(0)..=(center.1 as i32 + r).min(ih as i32 - 1) {
                for x in (center.0 as i32 - r).max(0)..=(center.0 as i32 + r).min(iw as i32 - 1) {
                    let dx = x as f32 + 0.5 - center.0;
                    let dy = y as f32 + 0.5 - center.1;
                    let d = (dx * dx + dy * dy).sqrt() / radius;
                    if d > 1.0 {
                        continue;
                    }
                    let falloff = if d <= hardness { 1.0 } else { 1.0 - (d - hardness) / (1.0 - hardness).max(0.001) };
                    let sample = edit::heal_sample(&source, x, y, radius as i32);
                    let mut ink = crate::raster::byte_to_f(sample);
                    ink[3] *= falloff;
                    let index = (y as usize * iw as usize + x as usize) * 4;
                    crate::blend::blend_pixel(&mut image.pixels_mut()[index..index + 4], ink, crate::blend::BlendMode::Normal);
                }
            }
        }
        layer.rasterize_metadata();
        self.patch_preview(from, to, size);
    }

    fn content_fill_selection(&mut self) {
        self.mutate(|ed| {
            let selection = ed.selection.clone();
            if matches!(selection, Selection::None) || selection.is_empty() {
                return;
            }
            let Some(layer) = ed.document.active_mut() else { return };
            if layer.is_group || layer.adjustment.is_some() {
                return;
            }
            let Some(image) = layer.image.as_mut() else { return };
            let width = image.width;
            let height = image.height;
            let transform = layer.transform;
            let mut hole = vec![false; width as usize * height as usize];
            for y in 0..height {
                for x in 0..width {
                    let u = (x as f32 + 0.5) / width as f32;
                    let v = (y as f32 + 0.5) / height as f32;
                    let (dx, dy) = transform.unit_to_doc(u, v);
                    if selection.coverage_at(dx, dy) > 0.5 {
                        hole[(y * width + x) as usize] = true;
                    }
                }
            }
            edit::content_aware_fill(image, &hole);
            layer.rasterize_metadata();
        });
    }

    fn clone_stroke(&mut self, from: (f32, f32), to: (f32, f32), delta: (f32, f32)) {
        let Some(source) = self.editor.as_ref().and_then(|ed| ed.undo.last().map(|snap| snap.document.clone())) else {
            return;
        };
        let size = self.brush_size;
        let hardness = self.brush_hardness;
        let opacity = self.brush_opacity;
        let Some(ed) = self.editor.as_mut() else { return };
        let width = ed.document.width;
        let height = ed.document.height;
        let selection = ed.selection.clone();
        let Some(layer) = ed.document.active_mut() else { return };
        if layer.is_group || layer.adjustment.is_some() {
            return;
        }
        if layer.image.is_none() {
            layer.image = Some(Raster::new(width, height, [0, 0, 0, 0]));
            layer.transform = Transform::identity(width as f32, height as f32);
        }
        let transform = layer.transform;
        let Some(image) = layer.image.as_mut() else { return };
        let iw = image.width.max(1);
        let ih = image.height.max(1);
        let radius = size * iw as f32 / transform.width.max(1.0);
        let map = |p: (f32, f32)| {
            let (u, v) = transform.doc_to_unit(p.0, p.1);
            (u * iw as f32, v * ih as f32)
        };
        let a = map(from);
        let b = map(to);
        let dist = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let step = (radius * 0.35).max(1.0);
        let n = (dist / step).ceil().max(1.0) as i32;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let center = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            let doc_center = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            let r = radius.ceil() as i32 + 1;
            let x0 = (center.0 as i32 - r).max(0);
            let y0 = (center.1 as i32 - r).max(0);
            let x1 = (center.0 as i32 + r).min(iw as i32 - 1);
            let y1 = (center.1 as i32 + r).min(ih as i32 - 1);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let dx = x as f32 + 0.5 - center.0;
                    let dy = y as f32 + 0.5 - center.1;
                    let d = (dx * dx + dy * dy).sqrt() / radius.max(0.5);
                    if d > 1.0 {
                        continue;
                    }
                    let falloff = if d <= hardness { 1.0 } else { 1.0 - (d - hardness) / (1.0 - hardness).max(0.001) };
                    let u = (x as f32 + 0.5) / iw as f32;
                    let v = (y as f32 + 0.5) / ih as f32;
                    let (lx, ly) = transform.unit_to_doc(u, v);
                    let cover = selection.coverage_at(lx, ly);
                    if cover <= 0.0 || falloff <= 0.0 {
                        continue;
                    }
                    let offset = (lx - doc_center.0, ly - doc_center.1);
                    let sample = render::sample_document(&source, doc_center.0 + delta.0 + offset.0, doc_center.1 + delta.1 + offset.1);
                    let mut ink = crate::raster::byte_to_f(sample);
                    ink[3] *= opacity * falloff * cover;
                    let i = (y as usize * iw as usize + x as usize) * 4;
                    crate::blend::blend_pixel(&mut image.pixels_mut()[i..i + 4], ink, crate::blend::BlendMode::Normal);
                }
            }
        }
        layer.rasterize_metadata();
        self.patch_preview(from, to, size);
    }

    fn take_visual_list(&mut self) -> Vec<Editor> {
        let mut list = std::mem::take(&mut self.tabs);
        if let Some(active) = self.editor.take() {
            let slot = self.tab_slot.min(list.len());
            list.insert(slot, active);
        }
        list
    }

    fn restore_visual_list(&mut self, mut list: Vec<Editor>, active: usize) {
        if list.is_empty() {
            self.editor = None;
            self.tabs.clear();
            self.tab_slot = 0;
            return;
        }
        let active = active.min(list.len() - 1);
        self.tab_slot = active;
        self.editor = Some(list.remove(active));
        self.tabs = list;
    }

    fn activate_visual(&mut self, index: usize) {
        let list = self.take_visual_list();
        if index >= list.len() {
            self.restore_visual_list(list, self.tab_slot);
            return;
        }
        self.restore_visual_list(list, index);
        self.after_tab_switch();
    }

    fn close_visual(&mut self, index: usize) {
        let mut list = self.take_visual_list();
        if index >= list.len() {
            self.restore_visual_list(list, self.tab_slot);
            return;
        }
        let active = if index < self.tab_slot {
            self.tab_slot - 1
        } else if index == self.tab_slot {
            index
        } else {
            self.tab_slot
        };
        list.remove(index);
        self.restore_visual_list(list, active);
        self.after_tab_switch();
    }

    fn close_tab(&mut self) {
        let mut list = self.take_visual_list();
        if list.is_empty() {
            return;
        }
        let slot = self.tab_slot.min(list.len() - 1);
        list.remove(slot);
        let next = slot.min(list.len().saturating_sub(1));
        self.restore_visual_list(list, next);
        self.after_tab_switch();
    }

    fn after_tab_switch(&mut self) {
        self.preview = None;
        self.texture = None;
        self.crop = None;
        self.poly_points.clear();
        self.gesture = Gesture::None;
        self.fit_pending = true;
        self.watch_stamp = None;
        self.request_composite();
    }

    fn open_effects(&mut self) {
        if self.editor.is_none() {
            self.note("还没有打开工程", "No document open");
            return;
        }
        if let Some(ed) = self.editor.as_mut() {
            ed.push_undo();
        }
        self.modal = Modal::Effects;
    }

    fn paint_at(&mut self, doc: (f32, f32), erase: bool) {
        self.stroke_between(doc, doc, erase);
    }

    fn stroke_between(&mut self, from: (f32, f32), to: (f32, f32), erase: bool) {
        if self.paint_mask {
            self.paint_mask_stroke(from, to, erase);
            return;
        }
        let size = self.brush_size;
        let hardness = self.brush_hardness;
        let opacity = self.brush_opacity;
        let color = [self.fg[0], self.fg[1], self.fg[2], 255];
        let Some(ed) = self.editor.as_mut() else { return };
        let width = ed.document.width;
        let height = ed.document.height;
        let selection = ed.selection.clone();
        let Some(layer) = ed.document.active_mut() else { return };
        if layer.is_group || layer.adjustment.is_some() {
            return;
        }
        if layer.image.is_none() {
            layer.image = Some(Raster::new(width, height, [0, 0, 0, 0]));
            layer.transform = Transform::identity(width as f32, height as f32);
        }
        let transform = layer.transform;
        let Some(image) = layer.image.as_mut() else { return };
        let iw = image.width.max(1);
        let ih = image.height.max(1);
        let radius = size * iw as f32 / transform.width.max(1.0);
        let map = |p: (f32, f32)| {
            let (u, v) = transform.doc_to_unit(p.0, p.1);
            (u * iw as f32, v * ih as f32)
        };
        let a = map(from);
        let b = map(to);
        let dist = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let step = (radius * 0.15).max(0.75);
        let n = (dist / step).ceil().max(1.0) as i32;
        let sel = selection;
        let tr = transform;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            raster::paint_dab(image, p.0, p.1, radius, hardness, color, opacity, erase, Some(&|x, y| {
                let u = (x as f32 + 0.5) / iw as f32;
                let v = (y as f32 + 0.5) / ih as f32;
                let (dx, dy) = tr.unit_to_doc(u, v);
                sel.coverage_at(dx, dy)
            }));
        }
        layer.rasterize_metadata();
        self.last_dab = Some(to);
        self.stamp_preview(from, to, size, color, opacity, hardness, erase);
    }

    fn paint_mask_stroke(&mut self, from: (f32, f32), to: (f32, f32), erase: bool) {
        let size = self.brush_size;
        let hardness = self.brush_hardness;
        let opacity = self.brush_opacity;
        let Some(ed) = self.editor.as_mut() else { return };
        let width = ed.document.width;
        let height = ed.document.height;
        let Some(layer) = ed.document.active_mut() else { return };
        if layer.mask.is_none() {
            layer.mask = Some(Raster::new(width.max(1), height.max(1), [255, 255, 255, 255]));
            layer.mask_enabled = true;
        }
        let color = if erase { [0, 0, 0, 255] } else { [255, 255, 255, 255] };
        let Some(mask) = layer.mask.as_mut() else { return };
        let radius = size.max(1.0);
        let dist = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt();
        let step = (radius * 0.2).max(1.0);
        let n = (dist / step).ceil().max(1.0) as i32;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let p = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            crate::raster::paint_dab(mask, p.0, p.1, radius, hardness, color, opacity, false, None);
        }
        layer.touch();
        self.patch_preview(from, to, size);
    }

    fn pointer_trail(&self, ui: &egui::Ui, rect: Rect) -> Vec<Pos2> {
        let painting = self.stroke_active();
        let mut points = Vec::new();
        ui.input(|input| {
            for event in &input.events {
                if let egui::Event::PointerMoved(pos) = event {
                    if painting || rect.expand(80.0).contains(*pos) {
                        points.push(*pos);
                    }
                }
            }
        });
        points
    }

    fn sample_hover(&self, x: f32, y: f32) -> [u8; 4] {
        if let Some(preview) = &self.preview {
            let px = ((x - preview.origin_x) * preview.scale) as i32;
            let py = ((y - preview.origin_y) * preview.scale) as i32;
            if px >= 0 && py >= 0 && (px as u32) < preview.image.width && (py as u32) < preview.image.height {
                return preview.image.pixel(px, py);
            }
        }
        [0, 0, 0, 0]
    }

    fn stamp_preview(&mut self, from: (f32, f32), to: (f32, f32), radius: f32, color: [u8; 4], opacity: f32, hardness: f32, erase: bool) {
        let Some(preview) = &mut self.preview else {
            self.pending_composite = true;
            return;
        };
        let scale = preview.scale.max(0.01);
        let map = |p: (f32, f32)| ((p.0 - preview.origin_x) * scale, (p.1 - preview.origin_y) * scale);
        let a = map(from);
        let b = map(to);
        let pr = (radius * scale).max(0.5);
        let dist = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let step = (pr * 0.15).max(0.75);
        let n = (dist / step).ceil().max(1.0) as i32;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            raster::paint_dab(&mut preview.image, p.0, p.1, pr, hardness, color, opacity, erase, None);
        }
        let pad = pr.ceil() as i32 + 2;
        let x0 = (a.0.min(b.0).floor() as i32 - pad).max(0) as u32;
        let y0 = (a.1.min(b.1).floor() as i32 - pad).max(0) as u32;
        let x1 = ((a.0.max(b.0).ceil() as i32 + pad).max(0) as u32).min(preview.image.width);
        let y1 = ((a.1.max(b.1).ceil() as i32 + pad).max(0) as u32).min(preview.image.height);
        let dirty = (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0));
        if let Some((x, y, w, h)) = dirty {
            self.mark_preview_dirty(x, y, w, h);
        }
    }

    fn mark_preview_dirty(&mut self, x: u32, y: u32, w: u32, h: u32) {
        self.preview_dirty = Some(match self.preview_dirty {
            Some((x0, y0, w0, h0)) => {
                let left = x0.min(x);
                let top = y0.min(y);
                let right = (x0 + w0).max(x + w);
                let bottom = (y0 + h0).max(y + h);
                (left, top, right - left, bottom - top)
            }
            None => (x, y, w, h),
        });
        self.texture_dirty = true;
    }

    fn patch_preview(&mut self, from: (f32, f32), to: (f32, f32), radius: f32) {
        let Some(ed) = &self.editor else { return };
        let Some(preview) = &mut self.preview else {
            self.pending_composite = true;
            return;
        };
        let scale = preview.scale;
        let origin_x = preview.origin_x;
        let origin_y = preview.origin_y;
        let pad = (radius * scale).ceil() as i32 + 6;
        let x0 = (((from.0.min(to.0) - origin_x) * scale).floor() as i32 - pad).max(0) as u32;
        let y0 = (((from.1.min(to.1) - origin_y) * scale).floor() as i32 - pad).max(0) as u32;
        let x1 = (((from.0.max(to.0) - origin_x) * scale).ceil() as i32 + pad).max(0) as u32;
        let y1 = (((from.1.max(to.1) - origin_y) * scale).ceil() as i32 + pad).max(0) as u32;
        let x1 = x1.min(preview.image.width);
        let y1 = y1.min(preview.image.height);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        composite_into(&ed.document, &mut preview.image, origin_x, origin_y, scale, Some((x0, y0, x1 - x0, y1 - y0)));
        let (dx, dy, dw, dh) = (x0, y0, x1 - x0, y1 - y0);
        self.mark_preview_dirty(dx, dy, dw, dh);
    }

    fn transform_gesture(&self) -> bool {
        matches!(self.gesture, Gesture::Move { .. } | Gesture::Scale { .. } | Gesture::Rotate { .. })
    }

    fn ensure_live_drag(&mut self, ctx: &egui::Context) {
        if !self.live_drag_on {
            self.start_live_drag(ctx);
        }
        self.poll_live_drag(ctx);
    }

    fn start_live_drag(&mut self, ctx: &egui::Context) {
        self.live_layers.clear();
        self.live_under = None;
        self.live_under_rx = None;
        self.live_hold = false;
        self.live_drag_on = true;
        let Some(ed) = &self.editor else { return };
        let Some(active) = ed.document.active_layer_id else { return };
        let mut ids = vec![active];
        ids.extend(ed.document.descendant_ids(active));
        for id in &ids {
            let Some(layer) = ed.document.layer(*id) else { continue };
            let Some(image) = &layer.image else { continue };
            let preview = layer_drag_image(image, layer.opacity);
            let texture = ctx.load_texture(format!("drag-{id}"), preview, egui::TextureOptions::LINEAR);
            self.live_layers.push(LiveLayer { id: *id, texture, start: layer.transform });
        }
        if self.live_layers.is_empty() {
            return;
        }
        let (origin_x, origin_y, scale, width, height) = self.preview_job(&ed.document);
        let mut doc = ed.document.clone();
        for id in ids {
            if let Some(layer) = doc.layer_mut(id) {
                layer.visible = false;
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.live_under_rx = Some(rx);
        std::thread::spawn(move || {
            let image = render::composite_region(&doc, origin_x, origin_y, scale, width, height);
            let _ = tx.send(PreviewMsg { generation: 0, image, scale, origin_x, origin_y });
        });
    }

    fn poll_live_drag(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.live_under_rx else { return };
        let Ok(msg) = rx.try_recv() else { return };
        let width = msg.image.width as usize;
        let height = msg.image.height as usize;
        let baked = bake_checker(&Preview {
            image: msg.image,
            scale: msg.scale,
            origin_x: msg.origin_x,
            origin_y: msg.origin_y,
        });
        let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &baked);
        let texture = ctx.load_texture("drag-under", image, egui::TextureOptions::LINEAR);
        self.live_under = Some((texture, msg.origin_x, msg.origin_y, msg.scale));
        self.live_under_rx = None;
    }

    fn cover_live_starts(&self, painter: &egui::Painter, rect: Rect) {
        let cover = Color32::from_rgb(46, 46, 46);
        for live in &self.live_layers {
            let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
            let pts: Vec<Pos2> = corners.iter().map(|(u, v)| self.doc_to_screen(live.start.unit_to_doc(*u, *v), rect)).collect();
            painter.add(egui::Shape::convex_polygon(pts, cover, egui::Stroke::NONE));
        }
    }

    fn paint_live_layers(&self, painter: &egui::Painter, rect: Rect) {
        let Some(ed) = &self.editor else { return };
        for live in &self.live_layers {
            let Some(layer) = ed.document.layer(live.id) else { continue };
            let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
            let pts: Vec<Pos2> = corners.iter().map(|(u, v)| self.doc_to_screen(layer.transform.unit_to_doc(*u, *v), rect)).collect();
            let mut mesh = egui::Mesh::with_texture(live.texture.id());
            let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
            for i in 0..4 {
                mesh.vertices.push(egui::epaint::Vertex { pos: pts[i], uv: uvs[i], color: Color32::WHITE });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(egui::Shape::mesh(mesh));
        }
    }

    fn clear_live_drag(&mut self) {
        self.live_layers.clear();
        self.live_under = None;
        self.live_under_rx = None;
        self.live_drag_on = false;
        self.live_hold = false;
    }

    fn invalidate_preview(&mut self) {
        self.texture_dirty = true;
        self.pending_composite = true;
    }

    fn sample_color(&mut self, doc: (f32, f32)) {
        if let Some(ed) = &self.editor {
            let px = render::sample_document(&ed.document, doc.0, doc.1);
            self.fg = [px[0], px[1], px[2]];
        }
    }

    fn wand_at(&mut self, doc: (f32, f32)) {
        let tolerance = self.tolerance;
        if let Some(ed) = &self.editor {
            let selection = edit::magic_select(&ed.document, doc.0, doc.1, tolerance);
            if let Some(ed) = self.editor.as_mut() {
                ed.selection = selection;
            }
            self.note("已按颜色建立选区", "Selection created from color");
        }
    }

    fn bucket_at(&mut self, doc: (f32, f32)) {
        let color = [self.fg[0], self.fg[1], self.fg[2], 255];
        let tolerance = self.tolerance;
        self.mutate(|ed| {
            let _ = edit::bucket(&mut ed.document, doc.0, doc.1, color, tolerance);
        });
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || !matches!(self.modal, Modal::None) {
            return;
        }
        let Some(combo) = ctx.input(pressed_combo) else { return };
        let action = self.bindings.iter().find(|(_, binding)| *binding == &combo).map(|(id, _)| id.clone());
        let Some(action) = action else { return };
        self.run_binding(&action);
    }

    fn run_binding(&mut self, action: &str) {
        match action {
            "undo" => self.do_undo(),
            "redo" => self.do_redo(),
            "new" => self.modal = Modal::New { width: 1920, height: 1080, ppi: 72.0, white: true },
            "open" => self.pick_image(),
            "save" => self.save(false),
            "select-all" => self.select_all(),
            "deselect" => self.deselect(),
            "invert-sel" => self.invert_selection(),
            "duplicate" => self.duplicate(),
            "merge" => self.merge_down(),
            "fit" => self.fit_pending = true,
            "actual" => self.zoom_center(1.0),
            "rulers" => self.show_rulers = !self.show_rulers,
            "brush-down" => self.brush_size = (self.brush_size - 2.0).max(1.0),
            "brush-up" => self.brush_size = (self.brush_size + 2.0).min(400.0),
            "swap" => std::mem::swap(&mut self.fg, &mut self.bg),
            "move" => self.set_tool(Tool::Move),
            "brush" => self.set_tool(Tool::Brush),
            "eraser" => self.set_tool(Tool::Eraser),
            "marquee" => self.set_tool(Tool::Marquee),
            "eyedropper" => self.set_tool(Tool::Eyedropper),
            "hand" => self.set_tool(Tool::Hand),
            "zoom" => self.set_tool(Tool::Zoom),
            "crop" => self.set_tool(Tool::Crop),
            "gradient" => self.set_tool(Tool::Gradient),
            "type" => self.set_tool(Tool::Type),
            "lasso" => self.set_tool(Tool::Lasso),
            "wand" => self.set_tool(Tool::Wand),
            "clone" => self.set_tool(Tool::Clone),
            "heal" => self.set_tool(Tool::Heal),
            "blur" => self.set_tool(Tool::Blur),
            "liquify" => self.set_tool(Tool::Liquify),
            "ellipse" => self.set_tool(Tool::Ellipse),
            "bucket" => self.set_tool(Tool::Bucket),
            "shape" => self.set_tool(Tool::Shape),
            "poly" => self.set_tool(Tool::Poly),
            "zoom-in" => self.zoom_by(1.25),
            "zoom-out" => self.zoom_by(1.0 / 1.25),
            "clear" => self.delete_or_clear(),
            "nudge-left" => self.nudge(-1.0, 0.0),
            "nudge-right" => self.nudge(1.0, 0.0),
            "nudge-up" => self.nudge(0.0, -1.0),
            "nudge-down" => self.nudge(0.0, 1.0),
            _ => {}
        }
    }

    fn check_updates(&mut self) {
        if self.update_rx.is_some() {
            self.note("正在检查更新…", "Already checking for updates…");
            return;
        }
        self.note("正在检查更新…", "Checking for updates…");
        let (tx, rx) = std::sync::mpsc::channel();
        self.update_rx = Some(rx);
        let local = env!("CARGO_PKG_VERSION").to_string();
        std::thread::spawn(move || {
            let message = match ureq::get("https://api.github.com/repos/robbietilton/Compositor/releases/latest")
                .set("User-Agent", "compositor-cross")
                .call()
            {
                Ok(response) => {
                    let body = response.into_string().unwrap_or_default();
                    let tag = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|value| value.get("tag_name").and_then(|v| v.as_str()).map(|s| s.to_string()))
                        .unwrap_or_else(|| "unknown".into());
                    format!(
                        "上游 Compositor 最新发布是 {tag}。\n这个跨平台版本是本地移植 {local}，用 cargo run 更新源码即可。\nhttps://github.com/robbietilton/Compositor/releases"
                    )
                }
                Err(err) => format!("无法检查更新: {err}"),
            };
            let _ = tx.send(message);
        });
    }

    fn poll_update(&mut self) {
        let message = self.update_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(message) = message {
            self.update_rx = None;
            self.modal = Modal::Error(message);
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let files: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().filter_map(|file| file.path.clone()).collect()
        });
        for path in files {
            self.open_any(&path);
        }
    }

    fn poll_preview(&mut self, ctx: &egui::Context) {
        let mut got = false;
        self.watch_project();
        while let Ok(msg) = self.preview_rx.try_recv() {
            self.compositing = false;
            if msg.generation == self.generation {
                self.preview = Some(Preview {
                    image: msg.image,
                    scale: msg.scale,
                    origin_x: msg.origin_x,
                    origin_y: msg.origin_y,
                });
                self.preview_dirty = None;
                self.texture_dirty = true;
                got = true;
            }
        }
        if !self.compositing && self.pending_composite {
            self.pending_composite = false;
            self.spawn_composite(ctx);
        } else if got {
            ctx.request_repaint();
        }
    }

    fn watch_project(&mut self) {
        let Some(path) = self.editor.as_ref().and_then(|ed| ed.path.clone()) else { return };
        if self.dirty() {
            return;
        }
        let manifest = path.join("manifest.json");
        let Ok(meta) = std::fs::metadata(&manifest) else { return };
        let Ok(modified) = meta.modified() else { return };
        if self.watch_stamp == Some(modified) {
            return;
        }
        if self.watch_stamp.is_none() {
            self.watch_stamp = Some(modified);
            return;
        }
        self.watch_stamp = Some(modified);
        if let Ok(doc) = project::load(&path) {
            if let Some(ed) = self.editor.as_mut() {
                ed.document = doc;
                ed.dirty = false;
            }
            self.preview = None;
            self.texture = None;
            self.request_composite();
            self.note("工程已从磁盘刷新", "Project reloaded from disk");
        }
    }

    fn request_composite(&mut self) {
        self.pending_composite = true;
    }

    fn spawn_composite(&mut self, ctx: &egui::Context) {
        if self.stroke_active() || self.transform_gesture() {
            self.pending_composite = true;
            return;
        }
        let Some(ed) = &self.editor else { return };
        if self.compositing {
            self.pending_composite = true;
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.compositing = true;
        let generation = self.generation;
        let doc = ed.document.clone();
        let (origin_x, origin_y, scale, width, height) = self.preview_job(&doc);
        let tx = self.preview_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let image = render::composite_region(&doc, origin_x, origin_y, scale, width, height);
            let _ = tx.send(PreviewMsg { generation, image, scale, origin_x, origin_y });
            ctx.request_repaint();
        });
    }

    fn preview_job(&self, doc: &Document) -> (f32, f32, f32, u32, u32) {
        let base = render::preview_scale(doc.width, doc.height);
        let full_w = (doc.width as f32 * base).round().max(1.0) as u32;
        let full_h = (doc.height as f32 * base).round().max(1.0) as u32;
        let Some(rect) = self.canvas_rect else {
            return (0.0, 0.0, base, full_w, full_h);
        };
        if self.zoom <= base * 1.15 {
            return (0.0, 0.0, base, full_w, full_h);
        }
        let (x0, y0) = self.screen_to_doc(rect.min, rect);
        let (x1, y1) = self.screen_to_doc(rect.max, rect);
        let margin = 96.0 / self.zoom.max(0.05);
        let origin_x = (x0 - margin).max(0.0);
        let origin_y = (y0 - margin).max(0.0);
        let end_x = (x1 + margin).min(doc.width as f32).max(origin_x + 1.0);
        let end_y = (y1 + margin).min(doc.height as f32).max(origin_y + 1.0);
        let region_w = end_x - origin_x;
        let region_h = end_y - origin_y;
        let scale = self.zoom.min(2560.0 / region_w).min(2560.0 / region_h).max(base);
        let width = (region_w * scale).round().clamp(1.0, 2560.0) as u32;
        let height = (region_h * scale).round().clamp(1.0, 2560.0) as u32;
        (origin_x, origin_y, scale, width, height)
    }

    fn preview_covers(&self, rect: Rect) -> bool {
        let Some(preview) = &self.preview else { return false };
        let Some(ed) = &self.editor else { return true };
        let (x0, y0) = self.screen_to_doc(rect.min, rect);
        let (x1, y1) = self.screen_to_doc(rect.max, rect);
        let vx0 = x0.max(0.0);
        let vy0 = y0.max(0.0);
        let vx1 = x1.min(ed.document.width as f32);
        let vy1 = y1.min(ed.document.height as f32);
        let cover_w = preview.image.width as f32 / preview.scale.max(0.01);
        let cover_h = preview.image.height as f32 / preview.scale.max(0.01);
        if vx1 <= vx0 || vy1 <= vy0 {
            return true;
        }
        let scale_ok = self.zoom <= preview.scale * 1.25;
        let covers = vx0 >= preview.origin_x - 8.0
            && vy0 >= preview.origin_y - 8.0
            && vx1 <= preview.origin_x + cover_w + 8.0
            && vy1 <= preview.origin_y + cover_h + 8.0
            && scale_ok;
        if covers {
            return true;
        }
        let (ox, oy, scale, width, height) = self.preview_job(&ed.document);
        (preview.origin_x - ox).abs() < 1.5
            && (preview.origin_y - oy).abs() < 1.5
            && (preview.scale - scale).abs() < 0.02
            && preview.image.width == width
            && preview.image.height == height
    }

    fn upload_texture(&mut self, ctx: &egui::Context) {
        let Some(preview) = &self.preview else { return };
        if let Some((x, y, w, h)) = self.preview_dirty {
            if w > 0 && h > 0 && self.texture.is_some() && (w as u64) * (h as u64) < (preview.image.width as u64) * (preview.image.height as u64) {
                let image = bake_checker_rect(preview, x, y, w, h);
                if let Some(texture) = &mut self.texture {
                    texture.set_partial([x as usize, y as usize], image, egui::TextureOptions::LINEAR);
                }
                self.preview_dirty = None;
                self.texture_dirty = false;
                self.last_upload = Instant::now();
                return;
            }
        }
        let baked = bake_checker(preview);
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [preview.image.width as usize, preview.image.height as usize],
            &baked,
        );
        if let Some(texture) = &mut self.texture {
            texture.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.texture = Some(ctx.load_texture("canvas", image, egui::TextureOptions::LINEAR));
        }
        self.preview_dirty = None;
        self.texture_dirty = false;
        self.last_upload = Instant::now();
    }

    fn fit(&mut self, rect: Rect) {
        let Some(ed) = &self.editor else { return };
        let margin = 48.0;
        let zx = (rect.width() - margin) / ed.document.width.max(1) as f32;
        let zy = (rect.height() - margin) / ed.document.height.max(1) as f32;
        self.zoom = zx.min(zy).clamp(0.02, 64.0);
        let size = vec2(ed.document.width as f32, ed.document.height as f32) * self.zoom;
        self.pan = (rect.size() - size) * 0.5;
    }

    fn doc_to_screen(&self, doc: (f32, f32), rect: Rect) -> Pos2 {
        rect.min + self.pan + vec2(doc.0, doc.1) * self.zoom
    }

    fn screen_to_doc(&self, screen: Pos2, rect: Rect) -> (f32, f32) {
        let p = (screen - rect.min - self.pan) / self.zoom;
        (p.x, p.y)
    }

    fn active_transform(&self) -> Option<Transform> {
        self.editor.as_ref().and_then(|ed| ed.document.active()).map(|layer| layer.transform)
    }

    fn mutate(&mut self, f: impl FnOnce(&mut Editor)) {
        let Some(ed) = self.editor.as_mut() else {
            self.note("还没有打开工程", "No document open");
            return;
        };
        if !ed.stroke {
            ed.push_undo();
        }
        f(ed);
        self.request_composite();
    }

    fn do_undo(&mut self) {
        if let Some(ed) = self.editor.as_mut() {
            ed.undo();
        }
        self.request_composite();
    }

    fn do_redo(&mut self) {
        if let Some(ed) = self.editor.as_mut() {
            ed.redo();
        }
        self.request_composite();
    }

    fn set_editor(&mut self, editor: Editor, path: Option<&Path>) {
        if let Some(path) = path {
            remember(&mut self.recent, path);
        }
        if let Some(current) = self.editor.take() {
            let slot = self.tab_slot.min(self.tabs.len());
            self.tabs.insert(slot, current);
            self.tab_slot = slot + 1;
        } else {
            self.tab_slot = self.tabs.len();
        }
        self.watch_stamp = None;
        self.editor = Some(editor);
        self.preview = None;
        self.texture = None;
        self.crop = None;
        self.fit_pending = true;
        self.hover = None;
        self.request_composite();
        self.note("已打开", "Opened");
    }

    fn open_any(&mut self, path: &Path) {
        if path.is_dir() || path.extension().and_then(|ext| ext.to_str()) == Some("comp") {
            self.open_project(path);
        } else {
            self.open_image_path(path);
        }
    }

    fn pick_project(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_folder() else { return };
        self.open_project(&path);
    }

    fn pick_image(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Image", &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "ico", "psd"])
            .add_filter("Photoshop", &["psd", "psb"])
            .add_filter("SVG", &["svg"])
            .add_filter("HEIC", &["heic", "heif"])
            .add_filter("Camera RAW", &["dng", "cr2", "cr3", "nef", "arw", "raf", "rw2", "orf", "pef", "raw"])
            .pick_file()
        else {
            return;
        };
        self.open_image_path(&path);
    }

    fn open_project(&mut self, path: &Path) {
        match project::load(path) {
            Ok(doc) => self.set_editor(Editor::from_document(doc, Some(path.to_path_buf())), Some(path)),
            Err(err) => self.fail(err),
        }
    }

    fn open_image_path(&mut self, path: &Path) {
        let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == "svg" {
            let bytes = match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(err) => {
                    self.fail(err.to_string());
                    return;
                }
            };
            match crate::svg::rasterize(&bytes) {
                Ok((name, image, notes)) => {
                    if self.editor.is_none() {
                        self.set_editor(Editor::from_document(Document::from_image(name, image), None), None);
                    } else {
                        self.mutate(|ed| { ed.document.add_layer(Layer::with_image(name, image)); });
                    }
                    if !notes.is_empty() {
                        self.modal = Modal::Error(notes.join("\n"));
                    }
                }
                Err(err) => self.fail(err),
            }
            return;
        }
        if ext == "heic" || ext == "heif" {
            match crate::decode::import_heic(path) {
                Ok(image) => {
                    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("HEIC").to_string();
                    self.set_editor(Editor::from_document(Document::from_image(name, image), None), None);
                    self.note("已从 HEIC 提取 JPEG 预览", "Opened the JPEG preview embedded in the HEIC");
                }
                Err(err) => self.fail(err),
            }
            return;
        }
        if crate::decode::is_raw(&ext) {
            match crate::decode::import_raw(path) {
                Ok(image) => {
                    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("RAW").to_string();
                    self.set_editor(Editor::from_document(Document::from_image(name, image), None), None);
                    self.note("已用本地去马赛克打开 RAW，可用显影继续调整", "Opened RAW with a local demosaic. Use Develop to finish it.");
                }
                Err(err) => self.fail(err),
            }
            return;
        }
        if ext == "psd" || ext == "psb" {
            match crate::psd::import_file(path) {
                Ok(imported) => {
                    let notes = imported.notes.join("\n");
                    self.set_editor(Editor::from_document(imported.document, None), None);
                    if notes.is_empty() {
                        self.note("已导入 PSD", "Imported PSD");
                    } else {
                        self.modal = Modal::Error(notes);
                    }
                }
                Err(err) => self.fail(err),
            }
            return;
        }
        match project::import_image(path) {
            Ok((name, image)) => {
                if self.editor.is_none() {
                    let doc = Document::from_image(name, image);
                    self.set_editor(Editor::from_document(doc, None), None);
                } else {
                    self.mutate(|ed| {
                        let mut layer = Layer::with_image(name, image);
                        layer.transform.origin_x = 24.0;
                        layer.transform.origin_y = 24.0;
                        ed.document.add_layer(layer);
                    });
                }
                remember(&mut self.recent, path);
            }
            Err(err) => self.fail(err),
        }
    }

    fn save(&mut self, save_as: bool) {
        let Some(ed) = &self.editor else { return };
        let existing = ed.path.clone();
        let path = if save_as || existing.is_none() {
            let Some(path) = rfd::FileDialog::new().add_filter("Compositor", &["comp"]).save_file() else { return };
            path
        } else {
            existing.unwrap()
        };
        if let Some(ed) = &self.editor {
            if let Err(err) = project::save(&ed.document, &path) {
                self.fail(err);
                return;
            }
        }
        if let Some(ed) = self.editor.as_mut() {
            ed.path = Some(if path.extension().and_then(|e| e.to_str()) == Some("comp") { path.clone() } else { path.with_extension("comp") });
            ed.dirty = false;
        }
        if let Ok(meta) = std::fs::metadata(path.join("manifest.json")) {
            self.watch_stamp = meta.modified().ok();
        }
        remember(&mut self.recent, &path);
        self.note("已保存", "Saved");
    }

    fn export_png(&mut self) {
        let Some(ed) = &self.editor else { return };
        let image = render::composite_document(&ed.document, 1.0).image;
        let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).save_file() else { return };
        let path: PathBuf = if path.extension().is_some() { path } else { path.with_extension("png") };
        if let Err(err) = project::export_png(&image, &path) {
            self.fail(err);
        } else {
            self.note("已导出 PNG", "Exported PNG");
        }
    }

    fn refresh_jpeg_preview(&mut self, ctx: &egui::Context, quality: u8) {
        if self.jpeg_preview.is_some() && self.jpeg_preview_quality == quality {
            return;
        }
        let Some(ed) = &self.editor else { return };
        let scale = (420.0 / ed.document.width.max(1) as f32).min(1.0);
        let image = render::composite_document(&ed.document, scale).image;
        let mut rgb = Vec::with_capacity(image.width as usize * image.height as usize * 3);
        for px in image.pixels().chunks_exact(4) {
            let a = px[3] as f32 / 255.0;
            for c in 0..3 {
                rgb.push((px[c] as f32 * a + 255.0 * (1.0 - a)).round() as u8);
            }
        }
        let mut encoded = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, quality.clamp(1, 100));
        if encoder.encode(&rgb, image.width, image.height, image::ExtendedColorType::Rgb8).is_err() {
            return;
        }
        let Ok(decoded) = image::load_from_memory(&encoded) else { return };
        let rgba = decoded.to_rgba8();
        let (w, h) = rgba.dimensions();
        let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
        self.jpeg_preview = Some(ctx.load_texture("jpeg-preview", color, egui::TextureOptions::LINEAR));
        self.jpeg_preview_quality = quality;
    }

    fn export_jpeg_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("JPEG", &["jpg", "jpeg"]).save_file() else { return };
        let path: PathBuf = if path.extension().is_some() { path } else { path.with_extension("jpg") };
        self.jpeg_preview = None;
        self.jpeg_preview_quality = 0;
        self.modal = Modal::Jpeg { quality: 90, path };
    }

    fn new_layer(&mut self) {
        self.mutate(|ed| {
            let layer = Layer::blank(format!("Layer {}", ed.document.layers.len() + 1), ed.document.width as f32, ed.document.height as f32);
            ed.document.add_layer(layer);
        });
    }

    fn new_group(&mut self) {
        self.mutate(|ed| {
            let group = Layer::group("Group", ed.document.width as f32, ed.document.height as f32);
            ed.document.add_layer(group);
        });
    }

    fn duplicate(&mut self) {
        self.mutate(|ed| {
            if let Some(id) = ed.document.active_layer_id {
                ed.document.duplicate_layer(id);
            }
        });
    }

    fn delete_active(&mut self) {
        self.mutate(|ed| {
            if let Some(id) = ed.document.active_layer_id {
                ed.document.delete_layer(id);
            }
        });
    }

    fn reorder(&mut self, up: bool) {
        let Some(id) = self.editor.as_ref().and_then(|ed| ed.document.active_layer_id) else { return };
        self.reorder_id(id, up);
    }

    fn reorder_id(&mut self, id: uuid::Uuid, up: bool) {
        self.note_appearance_edit();
        if let Some(ed) = self.editor.as_mut() {
            ed.document.active_layer_id = Some(id);
            ed.document.reorder_sibling(id, up);
            ed.dirty = true;
        }
        self.defer_composite = true;
    }

    fn duplicate_id(&mut self, id: uuid::Uuid) {
        self.mutate(|ed| {
            ed.document.active_layer_id = Some(id);
            ed.document.duplicate_layer(id);
        });
    }

    fn delete_id(&mut self, id: uuid::Uuid) {
        self.mutate(|ed| {
            ed.document.active_layer_id = Some(id);
            ed.document.delete_layer(id);
        });
    }

    fn merge_id(&mut self, id: uuid::Uuid) {
        let mut err = None;
        self.mutate(|ed| {
            ed.document.active_layer_id = Some(id);
            if let Err(e) = edit::merge_down(&mut ed.document, id) {
                err = Some(e);
            }
        });
        if let Some(err) = err {
            self.fail(err);
        }
    }

    fn delete_or_clear(&mut self) {
        let has_sel = self.editor.as_ref().is_some_and(|ed| !ed.selection.is_empty());
        if has_sel {
            self.clear_selection_pixels();
        } else {
            self.delete_active();
        }
    }

    fn note_appearance_edit(&mut self) {
        if self.appearance_captured {
            return;
        }
        if let Some(ed) = self.editor.as_mut() {
            if !ed.stroke {
                ed.push_undo();
            }
            ed.dirty = true;
        }
        self.appearance_captured = true;
    }

    fn bake_on_layer(&mut self, adjustment: Adjustment) {
        self.mutate(|ed| {
            let selection = ed.selection.clone();
            let _ = edit::apply_to_active(&mut ed.document, &selection, |image| {
                render::bake_adjustment(image, &adjustment);
            });
        });
    }

    fn merge_down(&mut self) {
        let mut err = None;
        self.mutate(|ed| {
            if let Some(id) = ed.document.active_layer_id {
                if let Err(e) = edit::merge_down(&mut ed.document, id) {
                    err = Some(e);
                }
            }
        });
        if let Some(err) = err {
            self.fail(err);
        }
    }

    fn merge_visible(&mut self) {
        let mut err = None;
        self.mutate(|ed| {
            if let Err(e) = edit::merge_visible(&mut ed.document) {
                err = Some(e);
            }
        });
        if let Some(err) = err {
            self.fail(err);
        }
    }

    fn flatten(&mut self) {
        let mut err = None;
        self.mutate(|ed| {
            if let Err(e) = edit::flatten(&mut ed.document) {
                err = Some(e);
            }
        });
        if let Some(err) = err {
            self.fail(err);
        }
    }

    fn flip_layer(&mut self, horizontal: bool) {
        self.mutate(|ed| {
            if let Some(layer) = ed.document.active_mut() {
                if horizontal {
                    layer.transform.flip_x = !layer.transform.flip_x;
                } else {
                    layer.transform.flip_y = !layer.transform.flip_y;
                }
                layer.touch();
            }
        });
    }

    fn add_mask(&mut self) {
        self.mutate(|ed| {
            let w = ed.document.width;
            let h = ed.document.height;
            if let Some(layer) = ed.document.active_mut() {
                if layer.mask.is_none() {
                    layer.mask = Some(Raster::new(w.max(1), h.max(1), [255, 255, 255, 255]));
                    layer.mask_enabled = true;
                    layer.touch();
                }
            }
        });
    }

    fn invert_mask(&mut self) {
        self.mutate(|ed| {
            if let Some(layer) = ed.document.active_mut() {
                if let Some(mask) = layer.mask.as_mut() {
                    for px in mask.pixels_mut().chunks_exact_mut(4) {
                        px[0] = 255 - px[0];
                        px[1] = px[0];
                        px[2] = px[0];
                    }
                    layer.touch();
                }
            }
        });
    }

    fn fill_fg(&mut self) {
        let color = [self.fg[0], self.fg[1], self.fg[2], 255];
        self.mutate(|ed| {
            let selection = ed.selection.clone();
            let _ = edit::fill_active(&mut ed.document, &selection, color);
        });
    }

    fn fill_bg(&mut self) {
        let color = [self.bg[0], self.bg[1], self.bg[2], 255];
        self.mutate(|ed| {
            let selection = ed.selection.clone();
            let _ = edit::fill_active(&mut ed.document, &selection, color);
        });
    }

    fn clear_selection_pixels(&mut self) {
        self.mutate(|ed| {
            let selection = ed.selection.clone();
            let _ = edit::clear_selection(&mut ed.document, &selection);
        });
    }

    fn desaturate(&mut self) {
        self.mutate(|ed| {
            let selection = ed.selection.clone();
            let _ = edit::desaturate_active(&mut ed.document, &selection);
        });
    }

    fn select_all(&mut self) {
        if let Some(ed) = self.editor.as_mut() {
            ed.selection = Selection::select_all(ed.document.width, ed.document.height);
        }
    }

    fn deselect(&mut self) {
        if let Some(ed) = self.editor.as_mut() {
            ed.selection = Selection::None;
        }
    }

    fn invert_selection(&mut self) {
        if let Some(ed) = self.editor.as_mut() {
            let w = ed.document.width;
            let h = ed.document.height;
            ed.selection = std::mem::replace(&mut ed.selection, Selection::None).invert(w, h);
        }
    }

    fn load_alpha_selection(&mut self) {
        let Some(ed) = self.editor.as_mut() else { return };
        let Some(layer) = ed.document.active() else { return };
        let Some(image) = layer.image.as_ref() else { return };
        let transform = layer.transform;
        let w = ed.document.width;
        let h = ed.document.height;
        if w as u64 * h as u64 > 8_000_000 {
            self.note("画布太大，无法载入像素选区", "Canvas is too large for an alpha selection");
            return;
        }
        let mut coverage = vec![0u8; w as usize * h as usize];
        for y in 0..h {
            for x in 0..w {
                let (u, v) = transform.doc_to_unit(x as f32 + 0.5, y as f32 + 0.5);
                if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
                    let sample = image.sample(u * image.width as f32, v * image.height as f32);
                    coverage[(y * w + x) as usize] = (sample[3] * 255.0).round() as u8;
                }
            }
        }
        ed.selection = Selection::Mask { x: 0, y: 0, width: w, height: h, coverage };
    }

    fn crop_to_selection(&mut self) {
        let bounds = self.editor.as_ref().and_then(|ed| ed.selection.bounds());
        if let Some((x0, y0, x1, y1)) = bounds {
            self.mutate(|ed| {
                ed.document.crop(x0, y0, x1 - x0, y1 - y0);
                ed.selection = Selection::None;
            });
        }
    }

    fn apply_crop(&mut self) {
        let Some((x, y, w, h)) = self.crop else { return };
        self.crop = None;
        self.mutate(|ed| ed.document.crop(x, y, w, h));
    }

    fn add_adjustment(&mut self, adjustment: Adjustment) {
        self.mutate(|ed| {
            let mut layer = Layer::blank(adjustment.kind_name(), ed.document.width as f32, ed.document.height as f32);
            layer.adjustment = Some(adjustment);
            layer.transform = Transform::identity(ed.document.width as f32, ed.document.height as f32);
            ed.document.add_layer(layer);
        });
    }

    fn open_image_size(&mut self) {
        if let Some(ed) = &self.editor {
            self.modal = Modal::ImageSize { width: ed.document.width, height: ed.document.height };
        }
    }

    fn open_canvas_size(&mut self) {
        if let Some(ed) = &self.editor {
            self.modal = Modal::Canvas { width: ed.document.width, height: ed.document.height, anchor: 4 };
        }
    }

    fn copy_merged(&mut self) {
        let Some(ed) = &self.editor else { return };
        let image = edit::copy_merged_rgba(&ed.document);
        match clipboard_set(&image) {
            Ok(()) => self.note("已复制合并图像", "Copied merged image"),
            Err(err) => self.fail(err),
        }
    }

    fn paste_layer(&mut self) {
        match clipboard_get() {
            Ok(image) => self.mutate(|ed| {
                ed.document.add_layer(Layer::with_image("Pasted", image));
            }),
            Err(err) => self.fail(err),
        }
    }

    fn confirm(&mut self, then: Confirm) {
        if let Some(ed) = self.editor.as_mut() {
            ed.dirty = false;
        }
        match then {
            Confirm::Quit => {
                // The close was cancelled; ask the viewport to close again now that we are clean.
                self.editor = None;
            }
            Confirm::OpenProject(path) => self.open_project(&path),
            Confirm::OpenImage(path) => self.open_image_path(&path),
            Confirm::NewDoc { width, height, white, ppi } => {
                let bg = white.then_some([255, 255, 255, 255]);
                let mut doc = Document::new(width, height, bg);
                doc.resolution = ppi;
                self.set_editor(Editor::from_document(doc, None), None);
            }
        }
    }

    fn modals(&mut self, ctx: &egui::Context) {
        let zh = self.zh;
        if matches!(self.modal, Modal::Effects) {
            self.effects_window(ctx);
        }
        if matches!(self.modal, Modal::Shortcuts) {
            self.shortcuts_window(ctx);
        }
        let mut apply_new = None;
        let mut apply_canvas = None;
        let mut apply_image = None;
        let mut apply_jpeg = None;
        let mut apply_blur = None;
        let mut apply_noise = None;
        let mut apply_hue = None;
        let mut apply_exposure = None;
        let mut apply_levels = None;
        let mut apply_curves = None;
        let mut apply_gradient = None;
        let mut apply_motion = None;
        let mut apply_feather = None;
        let mut apply_grain = None;
        let mut apply_balance = None;
        let mut apply_vignette = None;
        let mut apply_expand = None;
        let mut apply_bw = None;
        let mut apply_develop = None;
        let mut apply_text = None;
        let mut confirm = None;
        let mut close = false;
        match &mut self.modal {
            Modal::New { width, height, ppi, white } => {
                egui::Window::new(tr(zh, "新建", "New")).id(egui::Id::new("new-doc")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("1920×1080").clicked() { *width = 1920; *height = 1080; }
                        if ui.button("1280×720").clicked() { *width = 1280; *height = 720; }
                        if ui.button("1080×1920").clicked() { *width = 1080; *height = 1920; }
                        if ui.button("2048²").clicked() { *width = 2048; *height = 2048; }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(zh, "宽", "W"));
                        ui.add(egui::DragValue::new(width).range(1..=MAX_SIDE));
                        ui.label(tr(zh, "高", "H"));
                        ui.add(egui::DragValue::new(height).range(1..=MAX_SIDE));
                        ui.label("ppi");
                        ui.add(egui::DragValue::new(ppi).range(1.0..=9600.0));
                    });
                    ui.checkbox(white, tr(zh, "白色背景", "White background"));
                    ui.horizontal(|ui| {
                        if ui.button(tr(zh, "创建", "Create")).clicked() {
                            apply_new = Some((*width, *height, *ppi, *white));
                        }
                        if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                    });
                });
            }
            Modal::Canvas { width, height, anchor } => {
                egui::Window::new(tr(zh, "画布大小", "Canvas Size")).id(egui::Id::new("canvas-size")).collapsible(false).show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(zh, "宽", "W"));
                        ui.add(egui::DragValue::new(width).range(1..=MAX_SIDE));
                        ui.label(tr(zh, "高", "H"));
                        ui.add(egui::DragValue::new(height).range(1..=MAX_SIDE));
                    });
                    ui.label(tr(zh, "锚点", "Anchor"));
                    ui.horizontal(|ui| {
                        for i in 0..9 {
                            if ui.selectable_label(*anchor == i, format!("{}", i + 1)).clicked() { *anchor = i; }
                        }
                    });
                    if ui.button(tr(zh, "应用", "Apply")).clicked() { apply_canvas = Some((*width, *height, *anchor)); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::ImageSize { width, height } => {
                egui::Window::new(tr(zh, "图像大小", "Image Size")).id(egui::Id::new("image-size")).collapsible(false).show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(width).range(1..=MAX_SIDE));
                        ui.label("×");
                        ui.add(egui::DragValue::new(height).range(1..=MAX_SIDE));
                    });
                    if ui.button(tr(zh, "重采样", "Resample")).clicked() { apply_image = Some((*width, *height)); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Jpeg { quality, path } => {
                let label = path.display().to_string();
                let preview = self.jpeg_preview.clone();
                egui::Window::new("JPEG").id(egui::Id::new("jpeg")).collapsible(false).show(ctx, |ui| {
                    ui.label(label);
                    if let Some(tex) = preview {
                        ui.add(egui::Image::from_texture(&tex).max_width(280.0));
                    }
                    ui.add(egui::Slider::new(quality, 1..=100).text(tr(zh, "质量", "Quality")));
                    ui.label(tr(zh, "预览会按当前质量重新压缩", "Preview is recompressed at this quality"));
                    if ui.button(tr(zh, "导出", "Export")).clicked() { apply_jpeg = Some((*quality, path.clone())); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Blur { radius } => {
                egui::Window::new(tr(zh, "高斯模糊", "Gaussian Blur")).id(egui::Id::new("blur")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(radius, 0.5..=40.0));
                    if ui.button(tr(zh, "应用", "Apply")).clicked() { apply_blur = Some(*radius); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Noise { amount } => {
                egui::Window::new(tr(zh, "杂色", "Noise")).id(egui::Id::new("noise")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(amount, 1.0..=80.0));
                    if ui.button(tr(zh, "应用", "Apply")).clicked() { apply_noise = Some(*amount); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Hue { hue, sat, light, as_layer } => {
                egui::Window::new(tr(zh, "色相/饱和度", "Hue/Saturation")).id(egui::Id::new("hue")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(hue, -180.0..=180.0).text(tr(zh, "色相", "Hue")));
                    ui.add(egui::Slider::new(sat, -100.0..=100.0).text(tr(zh, "饱和度", "Saturation")));
                    ui.add(egui::Slider::new(light, -100.0..=100.0).text(tr(zh, "明度", "Lightness")));
                    ui.checkbox(as_layer, tr(zh, "作为调整图层", "As adjustment layer"));
                    if ui.button(tr(zh, "应用", "Apply")).clicked() { apply_hue = Some((*hue, *sat, *light, *as_layer)); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Exposure { exposure, offset, gamma, as_layer } => {
                egui::Window::new(tr(zh, "曝光", "Exposure")).id(egui::Id::new("exposure")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(exposure, -3.0..=3.0).text(tr(zh, "曝光", "Exposure")));
                    ui.add(egui::Slider::new(offset, -0.5..=0.5).text(tr(zh, "位移", "Offset")));
                    ui.add(egui::Slider::new(gamma, 0.2..=3.0).text("Gamma"));
                    ui.checkbox(as_layer, tr(zh, "作为调整图层", "As adjustment layer"));
                    if ui.button(tr(zh, "应用", "Apply")).clicked() { apply_exposure = Some((*exposure, *offset, *gamma, *as_layer)); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Levels { black, gamma, white, as_layer } => {
                egui::Window::new(tr(zh, "色阶", "Levels")).id(egui::Id::new("levels")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(black, 0.0..=254.0).text(tr(zh, "黑场", "Black")));
                    ui.add(egui::Slider::new(gamma, 0.1..=4.0).text("Gamma"));
                    ui.add(egui::Slider::new(white, 1.0..=255.0).text(tr(zh, "白场", "White")));
                    ui.checkbox(as_layer, tr(zh, "作为调整图层", "As adjustment layer"));
                    if ui.button(tr(zh, "应用", "Apply")).clicked() { apply_levels = Some((*black, *gamma, *white, *as_layer)); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Curves { points } => {
                egui::Window::new(tr(zh, "曲线", "Curves")).id(egui::Id::new("curves")).collapsible(false).show(ctx, |ui| {
                    curve_editor(ui, points);
                    if ui.button(tr(zh, "作为调整图层", "As adjustment layer")).clicked() {
                        apply_curves = Some(points.clone());
                    }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::GradientMap { shadow, highlight } => {
                egui::Window::new(tr(zh, "渐变映射", "Gradient Map")).id(egui::Id::new("gradient-map")).collapsible(false).show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr(zh, "阴影", "Shadows"));
                        ui.color_edit_button_srgb(shadow);
                        ui.label(tr(zh, "高光", "Highlights"));
                        ui.color_edit_button_srgb(highlight);
                    });
                    if ui.button(tr(zh, "作为调整图层", "As adjustment layer")).clicked() {
                        apply_gradient = Some((*shadow, *highlight));
                    }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Motion { angle, distance } => {
                egui::Window::new(tr(zh, "动感模糊", "Motion Blur")).id(egui::Id::new("motion")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(angle, -90.0..=90.0).text(tr(zh, "角度", "Angle")));
                    ui.add(egui::Slider::new(distance, 1.0..=80.0).text(tr(zh, "距离", "Distance")));
                    if ui.button(tr(zh, "作为调整图层", "As adjustment layer")).clicked() {
                        apply_motion = Some((*angle, *distance));
                    }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Feather { radius } => {
                egui::Window::new(tr(zh, "羽化", "Feather")).id(egui::Id::new("feather")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(radius, 1.0..=64.0));
                    if ui.button(tr(zh, "应用", "Apply")).clicked() { apply_feather = Some(*radius); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Effects => {}
            Modal::Grain { amount } => {
                egui::Window::new(tr(zh, "颗粒", "Grain")).id(egui::Id::new("grain")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(amount, 1.0..=100.0).text(tr(zh, "数量", "Amount")));
                    if ui.button(tr(zh, "作为调整图层", "As adjustment layer")).clicked() {
                        apply_grain = Some(*amount);
                    }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Balance { cyan_red, magenta_green, yellow_blue } => {
                egui::Window::new(tr(zh, "色彩平衡", "Color Balance")).id(egui::Id::new("balance")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(cyan_red, -100.0..=100.0).text(tr(zh, "青 / 红", "Cyan / Red")));
                    ui.add(egui::Slider::new(magenta_green, -100.0..=100.0).text(tr(zh, "品红 / 绿", "Magenta / Green")));
                    ui.add(egui::Slider::new(yellow_blue, -100.0..=100.0).text(tr(zh, "黄 / 蓝", "Yellow / Blue")));
                    if ui.button(tr(zh, "作为调整图层", "As adjustment layer")).clicked() {
                        apply_balance = Some((*cyan_red, *magenta_green, *yellow_blue));
                    }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Expand { radius, grow } => {
                let grow_flag = *grow;
                egui::Window::new(if grow_flag { tr(zh, "扩展", "Expand") } else { tr(zh, "收缩", "Contract") }).id(egui::Id::new("expand")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(radius, 1.0..=64.0));
                    if ui.button(tr(zh, "应用", "Apply")).clicked() { apply_expand = Some((*radius, grow_flag)); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::BlackWhite { weights } => {
                egui::Window::new(tr(zh, "黑白", "Black & White")).id(egui::Id::new("bw")).collapsible(false).vscroll(true).max_height(480.0).show(ctx, |ui| {
                    ui.add(egui::Slider::new(&mut weights[0], 0.0..=200.0).text(tr(zh, "红", "Reds")));
                    ui.add(egui::Slider::new(&mut weights[1], 0.0..=200.0).text(tr(zh, "黄", "Yellows")));
                    ui.add(egui::Slider::new(&mut weights[2], 0.0..=200.0).text(tr(zh, "绿", "Greens")));
                    ui.add(egui::Slider::new(&mut weights[3], 0.0..=200.0).text(tr(zh, "青", "Cyans")));
                    ui.add(egui::Slider::new(&mut weights[4], 0.0..=200.0).text(tr(zh, "蓝", "Blues")));
                    ui.add(egui::Slider::new(&mut weights[5], 0.0..=200.0).text(tr(zh, "品红", "Magentas")));
                    if ui.button(tr(zh, "作为调整图层", "As adjustment layer")).clicked() {
                        apply_bw = Some(*weights);
                    }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Develop(settings) => {
                egui::Window::new(tr(zh, "显影", "Develop")).id(egui::Id::new("develop")).collapsible(false).vscroll(true).max_height(520.0).show(ctx, |ui| {
                    ui.label(tr(zh, "这是 Camera Raw 的本地替代：色调、颜色、清晰度和暗角。", "A local stand-in for Camera Raw: tone, color, clarity and vignette."));
                    ui.add(egui::Slider::new(&mut settings.exposure, -3.0..=3.0).text(tr(zh, "曝光", "Exposure")));
                    ui.add(egui::Slider::new(&mut settings.contrast, -1.0..=1.0).text(tr(zh, "对比度", "Contrast")));
                    ui.add(egui::Slider::new(&mut settings.highlights, -1.0..=1.0).text(tr(zh, "高光", "Highlights")));
                    ui.add(egui::Slider::new(&mut settings.shadows, -1.0..=1.0).text(tr(zh, "阴影", "Shadows")));
                    ui.add(egui::Slider::new(&mut settings.temperature, -1.0..=1.0).text(tr(zh, "色温", "Temperature")));
                    ui.add(egui::Slider::new(&mut settings.tint, -1.0..=1.0).text(tr(zh, "色调", "Tint")));
                    ui.add(egui::Slider::new(&mut settings.vibrance, -1.0..=1.0).text(tr(zh, "自然饱和度", "Vibrance")));
                    ui.add(egui::Slider::new(&mut settings.saturation, -1.0..=1.0).text(tr(zh, "饱和度", "Saturation")));
                    ui.add(egui::Slider::new(&mut settings.clarity, 0.0..=1.5).text(tr(zh, "清晰度", "Clarity")));
                    ui.add(egui::Slider::new(&mut settings.sharpen, 0.0..=1.5).text(tr(zh, "锐化", "Sharpen")));
                    ui.add(egui::Slider::new(&mut settings.denoise, 0.0..=4.0).text(tr(zh, "降噪", "Denoise")));
                    ui.add(egui::Slider::new(&mut settings.vignette, 0.0..=1.0).text(tr(zh, "暗角", "Vignette")));
                    if ui.button(tr(zh, "应用到图层", "Apply")).clicked() {
                        apply_develop = Some(*settings);
                    }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Vignette { amount } => {
                egui::Window::new(tr(zh, "晕影", "Vignette")).id(egui::Id::new("vignette")).collapsible(false).show(ctx, |ui| {
                    ui.add(egui::Slider::new(amount, 0.05..=1.0));
                    if ui.button(tr(zh, "应用到图层", "Apply to layer")).clicked() {
                        apply_vignette = Some(*amount);
                    }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Text { content, at } => {
                let at = *at;
                egui::Window::new(tr(zh, "文字", "Type")).id(egui::Id::new("type")).collapsible(false).show(ctx, |ui| {
                    ui.text_edit_multiline(content);
                    if ui.button(tr(zh, "放置", "Place")).clicked() { apply_text = Some((content.clone(), at)); }
                    if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                });
            }
            Modal::Error(message) => {
                let message = message.clone();
                egui::Window::new(tr(zh, "提示", "Notice")).id(egui::Id::new("error")).collapsible(false).show(ctx, |ui| {
                    ui.label(message);
                    if ui.button(tr(zh, "关闭", "Close")).clicked() { close = true; }
                });
            }
            Modal::About => {
                egui::Window::new(tr(zh, "关于", "About")).id(egui::Id::new("about")).collapsible(false).show(ctx, |ui| {
                    ui.label("Compositor");
                    ui.label(tr(zh, 
                        "用 Rust 与 egui 重写的跨平台图层合成器。可读写 .comp 工程。",
                        "A cross-platform layer compositor written in Rust and egui. Reads and writes .comp projects.",
                    ));
                    ui.label(tr(zh, 
                        "可导入 8 位 RGB 的 PSD，并包含污点修复、内容识别填充、颗粒和色彩平衡。",
                        "Imports 8-bit RGB PSD files, and includes spot healing, content-aware fill, grain and color balance.",
                    ));
                    if ui.button(tr(zh, "关闭", "Close")).clicked() { close = true; }
                });
            }
            Modal::Shortcuts => {}
            Modal::Confirm { message, then } => {
                let message = message.clone();
                let then = then.clone();
                egui::Window::new(tr(zh, "确认", "Confirm")).id(egui::Id::new("confirm")).collapsible(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(message);
                    let quit = matches!(then, Confirm::Quit);
                    ui.horizontal(|ui| {
                        let proceed = if quit { tr(zh, "不保存并退出", "Quit without saving") } else { tr(zh, "继续", "Continue") };
                        if ui.button(proceed).clicked() { confirm = Some(then.clone()); }
                        if ui.button(tr(zh, "取消", "Cancel")).clicked() { close = true; }
                    });
                });
            }
            Modal::None => {}
        }
        if close {
            self.modal = Modal::None;
        }
        if let Some((w, h, ppi, white)) = apply_new {
            self.modal = Modal::None;
            let mut doc = Document::new(w, h, white.then_some([255, 255, 255, 255]));
            doc.resolution = ppi;
            self.set_editor(Editor::from_document(doc, None), None);
        }
        if let Some((w, h, anchor)) = apply_canvas {
            self.modal = Modal::None;
            let ax = (anchor % 3) as f32 / 2.0;
            let ay = (anchor / 3) as f32 / 2.0;
            self.mutate(|ed| ed.document.resize_canvas(w, h, 1.0 - ax, 1.0 - ay));
        }
        if let Some((w, h)) = apply_image {
            self.modal = Modal::None;
            self.mutate(|ed| ed.document.resize_image(w, h));
        }
        if let Modal::Jpeg { quality, .. } = &self.modal {
            let quality = *quality;
            self.refresh_jpeg_preview(ctx, quality);
        }
        if let Some((quality, path)) = apply_jpeg {
            self.modal = Modal::None;
            if let Some(ed) = &self.editor {
                let image = render::composite_document(&ed.document, 1.0).image;
                if let Err(err) = project::export_jpeg(&image, &path, quality) {
                    self.fail(err);
                } else {
                    self.note("已导出 JPEG", "Exported JPEG");
                }
            }
        }
        if let Some(radius) = apply_blur {
            self.modal = Modal::None;
            let radius = radius.round() as u32;
            self.spawn_pixel_filter(move |image| crate::raster::box_blur(image, radius));
        }
        if let Some(amount) = apply_noise {
            self.modal = Modal::None;
            self.spawn_pixel_filter(move |image| crate::raster::add_noise(image, amount, 1, false));
        }
        if let Some((hue, sat, light, as_layer)) = apply_hue {
            self.modal = Modal::None;
            let adjustment = Adjustment::HueSat { hue, saturation: sat, lightness: light };
            if as_layer {
                self.add_adjustment(adjustment);
            } else {
                self.bake_on_layer(adjustment);
            }
        }
        if let Some((exposure, offset, gamma, as_layer)) = apply_exposure {
            self.modal = Modal::None;
            let adjustment = Adjustment::Exposure { exposure, offset, gamma };
            if as_layer {
                self.add_adjustment(adjustment);
            } else {
                self.bake_on_layer(adjustment);
            }
        }
        if let Some(points) = apply_curves {
            self.modal = Modal::None;
            let mut channels = crate::document::identity_curves();
            channels[0] = points;
            self.add_adjustment(Adjustment::Curves { channels });
        }
        if let Some((shadow, highlight)) = apply_gradient {
            self.modal = Modal::None;
            self.add_adjustment(Adjustment::GradientMap {
                shadow: [shadow[0] as f32 / 255.0, shadow[1] as f32 / 255.0, shadow[2] as f32 / 255.0],
                highlight: [highlight[0] as f32 / 255.0, highlight[1] as f32 / 255.0, highlight[2] as f32 / 255.0],
                reversed: false,
            });
        }
        if let Some((angle, distance)) = apply_motion {
            self.modal = Modal::None;
            self.add_adjustment(Adjustment::MotionBlur { angle, distance });
        }
        if let Some(amount) = apply_grain {
            self.modal = Modal::None;
            self.add_adjustment(Adjustment::Grain { amount, size: 1.5, roughness: 50.0, seed: 1 });
        }
        if let Some((cyan_red, magenta_green, yellow_blue)) = apply_balance {
            self.modal = Modal::None;
            let midtone = [cyan_red, magenta_green, yellow_blue];
            self.add_adjustment(Adjustment::ColorBalance {
                shadow: [0.0; 3],
                midtone,
                highlight: [0.0; 3],
                preserve_luminosity: true,
            });
        }
        if let Some((radius, grow)) = apply_expand {
            self.modal = Modal::None;
            if let Some(ed) = self.editor.as_mut() {
                let w = ed.document.width;
                let h = ed.document.height;
                ed.selection = if grow {
                    crate::ops::expand_selection(&ed.selection, radius.round() as i32, w, h)
                } else {
                    crate::ops::contract_selection(&ed.selection, radius.round() as i32, w, h)
                };
            }
        }
        if let Some(weights) = apply_bw {
            self.modal = Modal::None;
            self.add_adjustment(Adjustment::BlackWhite { weights });
        }
        if let Some(settings) = apply_develop {
            self.modal = Modal::None;
            self.mutate(|ed| {
                let selection = ed.selection.clone();
                let _ = edit::apply_to_active(&mut ed.document, &selection, |image| crate::ops::develop(image, settings));
            });
        }
        if let Some(amount) = apply_vignette {
            self.modal = Modal::None;
            self.mutate(|ed| {
                let selection = ed.selection.clone();
                let _ = edit::apply_to_active(&mut ed.document, &selection, |image| edit::vignette(image, amount));
            });
        }
        if let Some(radius) = apply_feather {
            self.modal = Modal::None;
            if let Some(ed) = self.editor.as_mut() {
                let w = ed.document.width;
                let h = ed.document.height;
                ed.selection = edit::feather_selection(&ed.selection, radius.round() as i32, w, h);
            }
        }
        if let Some((black, gamma, white, as_layer)) = apply_levels {
            self.modal = Modal::None;
            let mut ranges = [LevelRange::default(); 4];
            ranges[0] = LevelRange { black, gamma, white, ..LevelRange::default() };
            let adjustment = Adjustment::Levels { ranges };
            if as_layer {
                self.add_adjustment(adjustment);
            } else {
                self.bake_on_layer(adjustment);
            }
        }
        if let Some((content, at)) = apply_text {
            self.modal = Modal::None;
            let size = self.font_size;
            let color = [self.fg[0], self.fg[1], self.fg[2], 255];
            match edit::rasterize_text(&content, size, color) {
                Ok(image) => self.mutate(|ed| {
                    let mut layer = Layer::with_image("Text", image);
                    layer.transform.origin_x = at.0;
                    layer.transform.origin_y = at.1;
                    layer.extras.insert("text".into(), serde_json::json!({
                        "content": content,
                        "fontName": "Arial",
                        "fontSize": size,
                        "red": color[0] as f32 / 255.0,
                        "green": color[1] as f32 / 255.0,
                        "blue": color[2] as f32 / 255.0,
                        "alignment": "left",
                        "tracking": 0,
                        "leading": 0
                    }));
                    ed.document.add_layer(layer);
                }),
                Err(err) => self.fail(err),
            }
        }
        if let Some(then) = confirm {
            self.modal = Modal::None;
            let quit = matches!(then, Confirm::Quit);
            self.confirm(then);
            if quit {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl App {
    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let zh = self.zh;
        let mut close = false;
        let waiting = self.rebinding.clone();
        if let Some(id) = waiting {
            if let Some(combo) = ctx.input(pressed_combo) {
                self.bindings.insert(id, combo);
                save_shortcuts(&self.bindings);
                self.rebinding = None;
            }
        }
        let rows: Vec<(String, String, String)> = default_shortcuts().into_iter().map(|(id, combo)| {
            let current = self.bindings.get(id).cloned().unwrap_or_else(|| combo.to_string());
            (id.to_string(), shortcut_label(id, zh).to_string(), current)
        }).collect();
        egui::Window::new(tr(zh, "快捷键", "Shortcuts")).id(egui::Id::new("shortcuts")).collapsible(false).show(ctx, |ui| {
            ui.label(tr(zh, "点一项，再按新的按键。设置保存在本机。", "Click an action, then press the new keys. Saved on this computer."));
            if self.rebinding.is_some() {
                ui.label(tr(zh, "正在等待按键…", "Waiting for a key…"));
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (id, label, combo) in rows {
                    if ui.button(format!("{label}    {combo}")).clicked() {
                        self.rebinding = Some(id);
                    }
                }
            });
            if ui.button(tr(zh, "恢复默认", "Reset")).clicked() {
                self.bindings = default_shortcuts().into_iter().map(|(id, combo)| (id.to_string(), combo.to_string())).collect();
                save_shortcuts(&self.bindings);
                self.rebinding = None;
            }
            if ui.button(tr(zh, "关闭", "Close")).clicked() {
                close = true;
            }
        });
        if close {
            self.modal = Modal::None;
            self.rebinding = None;
        }
    }

    fn effects_window(&mut self, ctx: &egui::Context) {
        let zh = self.zh;
        let mut close = false;
        let mut changed = false;
        egui::Window::new(tr(zh, "图层样式", "Layer Effects"))
            .id(egui::Id::new("effects"))
            .collapsible(false)
            .vscroll(true)
            .max_height(520.0)
            .show(ctx, |ui| {
                let Some(effects) = self.editor.as_mut().and_then(|ed| ed.document.active_mut()).map(|layer| &mut layer.effects) else {
                    ui.label(tr(zh, "没有活动图层", "No active layer"));
                    if ui.button(tr(zh, "关闭", "Close")).clicked() {
                        close = true;
                    }
                    return;
                };
                ui.label(tr(zh, "投影", "Drop Shadow"));
                changed |= shadow_editor(ui, &mut effects.shadow, zh, [0.0, 0.0, 0.0], 0.5, 90.0, 16.0, 8.0);
                ui.label(tr(zh, "外发光", "Outer Glow"));
                changed |= glow_editor(ui, &mut effects.outer_glow, zh, [1.0, 0.85, 0.3], 0.75, 18.0);
                ui.label(tr(zh, "描边", "Stroke"));
                changed |= stroke_editor(ui, &mut effects.stroke, zh);
                ui.label(tr(zh, "颜色叠加", "Color Overlay"));
                changed |= overlay_editor(ui, &mut effects.color_overlay, zh);
                ui.label(tr(zh, "内阴影", "Inner Shadow"));
                changed |= shadow_editor(ui, &mut effects.inner_shadow, zh, [0.0, 0.0, 0.0], 0.45, 90.0, 8.0, 6.0);
                ui.label(tr(zh, "内发光", "Inner Glow"));
                changed |= glow_editor(ui, &mut effects.inner_glow, zh, [1.0, 1.0, 1.0], 0.6, 10.0);
                if ui.button(tr(zh, "清除样式", "Clear")).clicked() {
                    *effects = crate::effects::LayerEffects::default();
                    changed = true;
                }
                if ui.button(tr(zh, "关闭", "Close")).clicked() {
                    close = true;
                }
            });
        if changed {
            self.request_composite();
        }
        if close {
            self.modal = Modal::None;
        }
    }
}

fn curve_editor(ui: &mut egui::Ui, points: &mut Vec<(f32, f32)>) {
    let (response, painter) = ui.allocate_painter(egui::vec2(220.0, 160.0), Sense::click_and_drag());
    let rect = response.rect;
    painter.rect_filled(rect, 4.0, Color32::from_rgb(24, 24, 24));
    painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, Color32::from_rgb(80, 80, 80)), egui::StrokeKind::Inside);
    let to_screen = |p: (f32, f32)| {
        pos2(
            rect.left() + p.0 / 255.0 * rect.width(),
            rect.bottom() - p.1 / 255.0 * rect.height(),
        )
    };
    let to_curve = |pos: Pos2| {
        (
            ((pos.x - rect.left()) / rect.width() * 255.0).clamp(0.0, 255.0),
            ((rect.bottom() - pos.y) / rect.height() * 255.0).clamp(0.0, 255.0),
        )
    };
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let point = to_curve(pos);
            points.push(point);
            points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let point = to_curve(pos);
            if let Some(nearest) = points.iter_mut().min_by(|a, b| {
                (a.0 - point.0).abs().partial_cmp(&(b.0 - point.0).abs()).unwrap_or(std::cmp::Ordering::Equal)
            }) {
                if nearest.0 > 1.0 && nearest.0 < 254.0 {
                    *nearest = point;
                } else {
                    nearest.1 = point.1;
                }
            }
            points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        }
    }
    let screen: Vec<Pos2> = points.iter().copied().map(to_screen).collect();
    if screen.len() >= 2 {
        painter.add(egui::Shape::line(screen.clone(), egui::Stroke::new(1.5, Color32::from_rgb(120, 190, 255))));
    }
    for p in screen {
        painter.circle_filled(p, 4.0, Color32::WHITE);
    }
    ui.label(format!("{} points — click to add", points.len()));
}

fn shadow_editor(ui: &mut egui::Ui, slot: &mut Option<crate::effects::ShadowEffect>, zh: bool, color: [f32; 3], opacity: f32, angle: f32, distance: f32, blur: f32) -> bool {
    let mut on = slot.is_some();
    let mut changed = ui.checkbox(&mut on, tr(zh, "启用", "On")).changed();
    if on && slot.is_none() {
        *slot = Some(crate::effects::ShadowEffect { enabled: true, angle, distance, blur, color, opacity });
        changed = true;
    }
    if !on {
        if slot.take().is_some() {
            changed = true;
        }
        return changed;
    }
    if let Some(effect) = slot.as_mut() {
        changed |= ui.add(egui::Slider::new(&mut effect.angle, -180.0..=180.0).text(tr(zh, "角度", "Angle"))).changed();
        changed |= ui.add(egui::Slider::new(&mut effect.distance, 0.0..=200.0).text(tr(zh, "距离", "Distance"))).changed();
        changed |= ui.add(egui::Slider::new(&mut effect.blur, 0.0..=80.0).text(tr(zh, "模糊", "Blur"))).changed();
        changed |= ui.add(egui::Slider::new(&mut effect.opacity, 0.0..=1.0).text(tr(zh, "不透明度", "Opacity"))).changed();
    }
    changed
}

fn glow_editor(ui: &mut egui::Ui, slot: &mut Option<crate::effects::GlowEffect>, zh: bool, color: [f32; 3], opacity: f32, size: f32) -> bool {
    let mut on = slot.is_some();
    let mut changed = ui.checkbox(&mut on, tr(zh, "启用", "On")).changed();
    if on && slot.is_none() {
        *slot = Some(crate::effects::GlowEffect { enabled: true, size, color, opacity });
        changed = true;
    }
    if !on {
        if slot.take().is_some() {
            changed = true;
        }
        return changed;
    }
    if let Some(effect) = slot.as_mut() {
        changed |= ui.add(egui::Slider::new(&mut effect.size, 0.0..=80.0).text(tr(zh, "大小", "Size"))).changed();
        changed |= ui.add(egui::Slider::new(&mut effect.opacity, 0.0..=1.0).text(tr(zh, "不透明度", "Opacity"))).changed();
    }
    changed
}

fn stroke_editor(ui: &mut egui::Ui, slot: &mut Option<crate::effects::StrokeEffect>, zh: bool) -> bool {
    let mut on = slot.is_some();
    let mut changed = ui.checkbox(&mut on, tr(zh, "启用", "On")).changed();
    if on && slot.is_none() {
        *slot = Some(crate::effects::StrokeEffect {
            enabled: true,
            size: 4.0,
            color: [0.1, 0.4, 1.0],
            opacity: 1.0,
            inside: false,
        });
        changed = true;
    }
    if !on {
        if slot.take().is_some() {
            changed = true;
        }
        return changed;
    }
    if let Some(effect) = slot.as_mut() {
        changed |= ui.add(egui::Slider::new(&mut effect.size, 1.0..=40.0).text(tr(zh, "大小", "Size"))).changed();
        changed |= ui.checkbox(&mut effect.inside, tr(zh, "内部", "Inside")).changed();
    }
    changed
}

fn overlay_editor(ui: &mut egui::Ui, slot: &mut Option<crate::effects::OverlayEffect>, zh: bool) -> bool {
    let mut on = slot.is_some();
    let mut changed = ui.checkbox(&mut on, tr(zh, "启用", "On")).changed();
    if on && slot.is_none() {
        *slot = Some(crate::effects::OverlayEffect { enabled: true, color: [1.0, 0.2, 0.2], opacity: 0.5 });
        changed = true;
    }
    if !on {
        if slot.take().is_some() {
            changed = true;
        }
        return changed;
    }
    if let Some(effect) = slot.as_mut() {
        changed |= ui.add(egui::Slider::new(&mut effect.opacity, 0.0..=1.0).text(tr(zh, "不透明度", "Opacity"))).changed();
    }
    changed
}

fn nice_step(target: f32) -> f32 {
    let raw = target.max(1.0);
    let pow = 10f32.powf(raw.log10().floor());
    let n = raw / pow;
    let nice = if n < 1.5 { 1.0 } else if n < 3.5 { 2.0 } else if n < 7.5 { 5.0 } else { 10.0 };
    nice * pow
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn handle_points(transform: Transform) -> [(f32, f32); 8] {
    [
        (0.0, 0.0),
        (0.5, 0.0),
        (1.0, 0.0),
        (1.0, 0.5),
        (1.0, 1.0),
        (0.5, 1.0),
        (0.0, 1.0),
        (0.0, 0.5),
    ]
    .map(|(u, v)| transform.unit_to_doc(u, v))
}

fn cursor_for_handle(handle: usize, transform: Transform, to_screen: impl Fn((f32, f32)) -> Pos2) -> CursorIcon {
    let (u, v) = handle_uv(handle);
    let center = to_screen(transform.center());
    let tip = to_screen(transform.unit_to_doc(u, v));
    let dx = tip.x - center.x;
    let dy = tip.y - center.y;
    let deg = dy.atan2(dx).to_degrees().rem_euclid(180.0);
    if deg < 22.5 || deg >= 157.5 {
        CursorIcon::ResizeHorizontal
    } else if deg < 67.5 {
        CursorIcon::ResizeNwSe
    } else if deg < 112.5 {
        CursorIcon::ResizeVertical
    } else {
        CursorIcon::ResizeNeSw
    }
}

fn handle_uv(handle: usize) -> (f32, f32) {
    match handle {
        0 => (0.0, 0.0),
        1 => (0.5, 0.0),
        2 => (1.0, 0.0),
        3 => (1.0, 0.5),
        4 => (1.0, 1.0),
        5 => (0.5, 1.0),
        6 => (0.0, 1.0),
        _ => (0.0, 0.5),
    }
}

fn scale_handle(original: Transform, handle: usize, pointer: (f32, f32), lock: bool) -> Transform {
    let (hu, hv) = handle_uv(handle);
    let anchor_u = if (hu - 0.5).abs() < 0.01 { 0.5 } else { 1.0 - hu };
    let anchor_v = if (hv - 0.5).abs() < 0.01 { 0.5 } else { 1.0 - hv };
    let anchor = original.unit_to_doc(anchor_u, anchor_v);
    let (pu, pv) = original.doc_to_unit(pointer.0, pointer.1);
    let edge_u = (hu - 0.5).abs() < 0.01;
    let edge_v = (hv - 0.5).abs() < 0.01;
    let mut across_u = if edge_u { 1.0 } else { (pu - anchor_u).abs() };
    let mut across_v = if edge_v { 1.0 } else { (pv - anchor_v).abs() };
    across_u = across_u.clamp(0.05, 20.0);
    across_v = across_v.clamp(0.05, 20.0);
    if lock || (!edge_u && !edge_v) {
        let factor = if edge_u {
            across_v
        } else if edge_v {
            across_u
        } else {
            across_u.max(across_v)
        };
        across_u = factor;
        across_v = factor;
    } else if edge_u {
        across_u = 1.0;
    } else if edge_v {
        across_v = 1.0;
    }
    let width = (original.width * across_u).max(1.0);
    let height = (original.height * across_v).max(1.0);
    let local_x = (anchor_u - 0.5) * width;
    let local_y = (anchor_v - 0.5) * height;
    let rad = original.rotation.to_radians();
    let (cos, sin) = (rad.cos(), rad.sin());
    let cx = anchor.0 - (local_x * cos - local_y * sin);
    let cy = anchor.1 - (local_x * sin + local_y * cos);
    let mut next = original;
    next.width = width;
    next.height = height;
    next.origin_x = cx - width / 2.0;
    next.origin_y = cy - height / 2.0;
    next
}

fn combine_selection(current: Selection, next: Selection, w: u32, h: u32, subtract: bool) -> Selection {
    if matches!(current, Selection::None) && !subtract {
        return next;
    }
    let mut coverage = vec![0u8; w as usize * h as usize];
    for y in 0..h {
        for x in 0..w {
            let a = current.coverage_at(x as f32 + 0.5, y as f32 + 0.5);
            let b = next.coverage_at(x as f32 + 0.5, y as f32 + 0.5);
            let v = if subtract { (a - b).max(0.0) } else { a.max(b) };
            coverage[(y * w + x) as usize] = (v * 255.0).round() as u8;
        }
    }
    Selection::Mask { x: 0, y: 0, width: w, height: h, coverage }
}

fn line_raster(start: (f32, f32), end: (f32, f32), width: f32, color: [u8; 4]) -> Raster {
    let pad = width + 2.0;
    let min_x = start.0.min(end.0) - pad;
    let min_y = start.1.min(end.1) - pad;
    let max_x = start.0.max(end.0) + pad;
    let max_y = start.1.max(end.1) + pad;
    let w = (max_x - min_x).ceil().max(1.0) as u32;
    let h = (max_y - min_y).ceil().max(1.0) as u32;
    let mut image = Raster::new(w, h, [0, 0, 0, 0]);
    let x0 = start.0 - min_x;
    let y0 = start.1 - min_y;
    let x1 = end.0 - min_x;
    let y1 = end.1 - min_y;
    for y in 0..h {
        for x in 0..w {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dx = x1 - x0;
            let dy = y1 - y0;
            let len2 = dx * dx + dy * dy;
            let t = if len2 < 1e-4 { 0.0 } else { (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0) };
            let cx = x0 + t * dx;
            let cy = y0 + t * dy;
            if ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() <= width / 2.0 {
                image.set_pixel(x as i32, y as i32, color);
            }
        }
    }
    image
}

fn paint_gradient(image: &mut Raster, a: (f32, f32), b: (f32, f32), fg: [u8; 3], bg: [u8; 3]) {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len2 = dx * dx + dy * dy;
    for y in 0..image.height {
        for x in 0..image.width {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let t = if len2 < 1e-4 { 0.0 } else { (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0) };
            let color = [
                (bg[0] as f32 + (fg[0] as f32 - bg[0] as f32) * t).round() as u8,
                (bg[1] as f32 + (fg[1] as f32 - bg[1] as f32) * t).round() as u8,
                (bg[2] as f32 + (fg[2] as f32 - bg[2] as f32) * t).round() as u8,
                255,
            ];
            image.set_pixel(x as i32, y as i32, color);
        }
    }
}

fn apply_filter_image(image: &mut Raster, selection: &Selection, transform: Transform, op: impl FnOnce(&mut Raster)) {
    if selection.is_empty() || matches!(selection, Selection::None) {
        op(image);
        return;
    }
    let before = image.clone();
    op(image);
    let w = image.width;
    let h = image.height;
    for y in 0..h {
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let v = (y as f32 + 0.5) / h as f32;
            let (dx, dy) = transform.unit_to_doc(u, v);
            if selection.coverage_at(dx, dy) <= 0.5 {
                image.set_pixel(x as i32, y as i32, before.pixel(x as i32, y as i32));
            }
        }
    }
}

fn layer_drag_image(image: &Raster, opacity: f32) -> egui::ColorImage {
    let longest = image.width.max(image.height).max(1) as f32;
    let scale = (768.0 / longest).min(1.0);
    let w = (image.width as f32 * scale).round().max(1.0) as u32;
    let h = (image.height as f32 * scale).round().max(1.0) as u32;
    let src = image.pixels();
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    for y in 0..h {
        let sy = ((y as f32 + 0.5) / scale).min(image.height as f32 - 1.0) as u32;
        for x in 0..w {
            let sx = ((x as f32 + 0.5) / scale).min(image.width as f32 - 1.0) as u32;
            let si = (sy * image.width + sx) as usize * 4;
            let di = (y * w + x) as usize * 4;
            let a = (src[si + 3] as f32 * opacity.clamp(0.0, 1.0)).round() as u8;
            pixels[di] = src[si];
            pixels[di + 1] = src[si + 1];
            pixels[di + 2] = src[si + 2];
            pixels[di + 3] = a;
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels)
}

fn selection_key(selection: &Selection) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    match selection {
        Selection::None => 0u8.hash(&mut hasher),
        Selection::Rect { x, y, w, h } => {
            1u8.hash(&mut hasher);
            x.to_bits().hash(&mut hasher);
            y.to_bits().hash(&mut hasher);
            w.to_bits().hash(&mut hasher);
            h.to_bits().hash(&mut hasher);
        }
        Selection::Mask { x, y, width, height, coverage } => {
            2u8.hash(&mut hasher);
            x.hash(&mut hasher);
            y.hash(&mut hasher);
            width.hash(&mut hasher);
            height.hash(&mut hasher);
            coverage.len().hash(&mut hasher);
            for (index, value) in coverage.iter().enumerate().step_by(128) {
                index.hash(&mut hasher);
                value.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

fn bake_checker_rect(preview: &Preview, x: u32, y: u32, w: u32, h: u32) -> egui::ColorImage {
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    let src = preview.image.pixels();
    let sw = preview.image.width;
    let scale = preview.scale.max(0.01);
    for row in 0..h {
        for col in 0..w {
            let sx = x + col;
            let sy = y + row;
            if sx >= sw || sy >= preview.image.height {
                continue;
            }
            let si = (sy * sw + sx) as usize * 4;
            let di = (row * w + col) as usize * 4;
            let a = src[si + 3] as f32 / 255.0;
            let doc_x = preview.origin_x + sx as f32 / scale;
            let doc_y = preview.origin_y + sy as f32 / scale;
            let checker = if ((doc_x / 16.0) as i32 + (doc_y / 16.0) as i32).rem_euclid(2) == 0 { 46.0 } else { 68.0 };
            for c in 0..3 {
                pixels[di + c] = (src[si + c] as f32 * a + checker * (1.0 - a)).round() as u8;
            }
            pixels[di + 3] = 255;
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels)
}

fn bake_checker(preview: &Preview) -> Vec<u8> {
    let mut out = preview.image.pixels().to_vec();
    let w = preview.image.width;
    let scale = preview.scale.max(0.01);
    for y in 0..preview.image.height {
        for x in 0..w {
            let i = (y * w + x) as usize * 4;
            let a = out[i + 3] as f32 / 255.0;
            let doc_x = preview.origin_x + x as f32 / scale;
            let doc_y = preview.origin_y + y as f32 / scale;
            let checker = if ((doc_x / 16.0) as i32 + (doc_y / 16.0) as i32).rem_euclid(2) == 0 { 46.0 } else { 68.0 };
            for c in 0..3 {
                out[i + c] = (out[i + c] as f32 * a + checker * (1.0 - a)).round() as u8;
            }
            out[i + 3] = 255;
        }
    }
    out
}

fn clipboard_set(image: &Raster) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| err.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width as usize,
            height: image.height as usize,
            bytes: std::borrow::Cow::Owned(image.pixels().to_vec()),
        })
        .map_err(|err| err.to_string())
}

fn clipboard_get() -> Result<Raster, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| err.to_string())?;
    let image = clipboard.get_image().map_err(|err| err.to_string())?;
    Raster::from_rgba(image.width as u32, image.height as u32, image.bytes.into_owned()).ok_or_else(|| "剪贴板图像无效".into())
}
