//! egui shell. The same window runs on Windows, macOS and Linux.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Vec2, pos2, vec2};

use crate::blend::BlendMode;
use crate::document::{Adjustment, Document, Guide, GuideAxis, Layer, LevelRange, Selection, Transform, MAX_SIDE};
use crate::edit::{self, ShapeKind};
use crate::project;
use crate::raster::{self, Raster};
use crate::render::{self, composite_into};

const ACCENT: Color32 = Color32::from_rgb(46, 130, 230);
const CANVAS_BG: Color32 = Color32::from_rgb(22, 22, 22);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Move,
    Marquee,
    Ellipse,
    Lasso,
    Wand,
    Crop,
    Eyedropper,
    Brush,
    Eraser,
    Bucket,
    Gradient,
    Shape,
    Type,
    Hand,
    Zoom,
    Clone,
    Heal,
    Blur,
    Liquify,
    Poly,
}

impl Tool {
    fn letter(self) -> &'static str {
        match self {
            Self::Move => "V",
            Self::Marquee => "M",
            Self::Ellipse => "O",
            Self::Lasso => "L",
            Self::Wand => "W",
            Self::Crop => "C",
            Self::Eyedropper => "I",
            Self::Brush => "B",
            Self::Eraser => "E",
            Self::Bucket => "K",
            Self::Gradient => "G",
            Self::Shape => "U",
            Self::Type => "T",
            Self::Hand => "H",
            Self::Zoom => "Z",
            Self::Clone => "S",
            Self::Heal => "J",
            Self::Blur => "R",
            Self::Liquify => "P",
            Self::Poly => "Y",
        }
    }

    fn name(self, zh: bool) -> &'static str {
        match (self, zh) {
            (Self::Move, true) => "移动",
            (Self::Move, false) => "Move",
            (Self::Marquee, true) => "矩形选框",
            (Self::Marquee, false) => "Marquee",
            (Self::Ellipse, true) => "椭圆选框",
            (Self::Ellipse, false) => "Ellipse",
            (Self::Lasso, true) => "套索",
            (Self::Lasso, false) => "Lasso",
            (Self::Wand, true) => "魔棒",
            (Self::Wand, false) => "Wand",
            (Self::Crop, true) => "裁剪",
            (Self::Crop, false) => "Crop",
            (Self::Eyedropper, true) => "吸管",
            (Self::Eyedropper, false) => "Eyedropper",
            (Self::Brush, true) => "画笔",
            (Self::Brush, false) => "Brush",
            (Self::Eraser, true) => "橡皮擦",
            (Self::Eraser, false) => "Eraser",
            (Self::Bucket, true) => "油漆桶",
            (Self::Bucket, false) => "Bucket",
            (Self::Gradient, true) => "渐变",
            (Self::Gradient, false) => "Gradient",
            (Self::Shape, true) => "形状",
            (Self::Shape, false) => "Shape",
            (Self::Type, true) => "文字",
            (Self::Type, false) => "Type",
            (Self::Hand, true) => "抓手",
            (Self::Hand, false) => "Hand",
            (Self::Zoom, true) => "缩放",
            (Self::Zoom, false) => "Zoom",
            (Self::Clone, true) => "仿制图章",
            (Self::Clone, false) => "Clone",
            (Self::Heal, true) => "污点修复",
            (Self::Heal, false) => "Heal",
            (Self::Blur, true) => "模糊",
            (Self::Blur, false) => "Blur",
            (Self::Liquify, true) => "液化",
            (Self::Liquify, false) => "Liquify",
            (Self::Poly, true) => "多边形套索",
            (Self::Poly, false) => "Polygonal Lasso",
        }
    }
}

const TOOLS: [Tool; 20] = [
    Tool::Move,
    Tool::Marquee,
    Tool::Ellipse,
    Tool::Lasso,
    Tool::Wand,
    Tool::Crop,
    Tool::Eyedropper,
    Tool::Brush,
    Tool::Eraser,
    Tool::Bucket,
    Tool::Gradient,
    Tool::Shape,
    Tool::Type,
    Tool::Hand,
    Tool::Zoom,
    Tool::Clone,
    Tool::Heal,
    Tool::Blur,
    Tool::Liquify,
    Tool::Poly,
];

enum Gesture {
    None,
    Pan { last: Pos2 },
    Move { origin: (f32, f32), start: (f32, f32), followers: Vec<(uuid::Uuid, f32, f32)> },
    Scale { handle: usize, start: Transform, children: Vec<(uuid::Uuid, Transform)> },
    Rotate { start_angle: f32, start_rotation: f32, start: Transform, children: Vec<(uuid::Uuid, Transform)> },
    Brush { last: (f32, f32) },
    Marquee { start: (f32, f32), add: bool, subtract: bool },
    Lasso { points: Vec<(f32, f32)> },
    Crop { start: (f32, f32) },
    Shape { start: (f32, f32) },
    Gradient { start: (f32, f32) },
    Clone { last: (f32, f32), delta: (f32, f32) },
    Heal { last: (f32, f32) },
    Distort { index: usize, corners: [(f32, f32); 4] },
    Liquify { last: (f32, f32) },
    BlurPaint { last: (f32, f32) },
    Guide { id: uuid::Uuid, axis: GuideAxis },
    /// Drag out of a ruler before a guide is created, so a click does not leave one behind.
    GuidePending { horizontal: bool, start: Pos2 },
    Sample,
}

enum Modal {
    None,
    New { width: u32, height: u32, ppi: f64, white: bool },
    Canvas { width: u32, height: u32, anchor: usize },
    ImageSize { width: u32, height: u32 },
    Jpeg { quality: u8, path: PathBuf },
    Blur { radius: f32 },
    Noise { amount: f32 },
    Hue { hue: f32, sat: f32, light: f32, as_layer: bool },
    Exposure { exposure: f32, offset: f32, gamma: f32, as_layer: bool },
    Levels { black: f32, gamma: f32, white: f32, as_layer: bool },
    Curves { points: Vec<(f32, f32)> },
    GradientMap { shadow: [u8; 3], highlight: [u8; 3] },
    Motion { angle: f32, distance: f32 },
    Feather { radius: f32 },
    Effects,
    Grain { amount: f32 },
    Balance { cyan_red: f32, magenta_green: f32, yellow_blue: f32 },
    Vignette { amount: f32 },
    Expand { radius: f32, grow: bool },
    BlackWhite { weights: [f32; 6] },
    Develop(crate::ops::DevelopSettings),
    Text { content: String, at: (f32, f32) },
    Error(String),
    About,
    Shortcuts,
    Confirm { message: String, then: Confirm },
}

#[derive(Clone)]
enum Confirm {
    Quit,
    OpenProject(PathBuf),
    OpenImage(PathBuf),
    NewDoc { width: u32, height: u32, white: bool, ppi: f64 },
}

struct Snapshot {
    document: Document,
    selection: Selection,
}

struct Editor {
    path: Option<PathBuf>,
    document: Document,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    dirty: bool,
    selection: Selection,
    stroke: bool,
}

impl Editor {
    fn from_document(document: Document, path: Option<PathBuf>) -> Self {
        let dirty = path.is_none();
        Self {
            path,
            document,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty,
            selection: Selection::None,
            stroke: false,
        }
    }

    fn push_undo(&mut self) {
        self.undo.push(Snapshot {
            document: self.document.clone(),
            selection: self.selection.clone(),
        });
        if self.undo.len() > 40 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    fn undo(&mut self) {
        let Some(prev) = self.undo.pop() else { return };
        self.redo.push(Snapshot {
            document: std::mem::replace(&mut self.document, prev.document),
            selection: std::mem::replace(&mut self.selection, prev.selection),
        });
        self.dirty = true;
    }

    fn redo(&mut self) {
        let Some(next) = self.redo.pop() else { return };
        self.undo.push(Snapshot {
            document: std::mem::replace(&mut self.document, next.document),
            selection: std::mem::replace(&mut self.selection, next.selection),
        });
        self.dirty = true;
    }

    fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|path| path.file_stem())
            .and_then(|stem| stem.to_str())
            .unwrap_or("Untitled");
        format!("{}{name}", if self.dirty { "*" } else { "" })
    }
}

struct Preview {
    image: Raster,
    scale: f32,
    origin_x: f32,
    origin_y: f32,
}

struct PreviewMsg {
    generation: u64,
    image: Raster,
    scale: f32,
    origin_x: f32,
    origin_y: f32,
}

struct LiveLayer {
    id: uuid::Uuid,
    texture: egui::TextureHandle,
    start: Transform,
}

pub struct App {
    zh: bool,
    editor: Option<Editor>,
    tabs: Vec<Editor>,
    tool: Tool,
    fg: [u8; 3],
    bg: [u8; 3],
    brush_size: f32,
    brush_hardness: f32,
    brush_opacity: f32,
    tolerance: u8,
    shape: ShapeKind,
    corner_radius: f32,
    line_width: f32,
    font_size: f32,
    zoom: f32,
    pan: Vec2,
    fit_pending: bool,
    show_rulers: bool,
    show_grid: bool,
    snap: bool,
    grid: f32,
    status: String,
    hover: Option<(f32, f32, [u8; 4])>,
    modal: Modal,
    gesture: Gesture,
    crop: Option<(f32, f32, f32, f32)>,
    preview: Option<Preview>,
    preview_tx: Sender<PreviewMsg>,
    preview_rx: Receiver<PreviewMsg>,
    compositing: bool,
    pending_composite: bool,
    generation: u64,
    texture: Option<egui::TextureHandle>,
    texture_dirty: bool,
    last_upload: Instant,
    recent: Vec<PathBuf>,
    renaming: Option<(uuid::Uuid, String)>,
    clone_mark: Option<(f32, f32)>,
    clone_offset: Option<(f32, f32)>,
    clone_aligned: bool,
    clone_sample_all: bool,
    paint_mask: bool,
    crop_ratio: (f32, f32),
    layer_clip: Option<Layer>,
    last_dab: Option<(f32, f32)>,
    poly_points: Vec<(f32, f32)>,
    watch_stamp: Option<std::time::SystemTime>,
    bindings: std::collections::HashMap<String, String>,
    rebinding: Option<String>,
    develop_base: Option<Raster>,
    develop_preview: Option<egui::TextureHandle>,
    canvas_rect: Option<egui::Rect>,
    jpeg_preview: Option<egui::TextureHandle>,
    jpeg_preview_quality: u8,
    layer_drag: f32,
    layer_drag_id: Option<uuid::Uuid>,
    tab_slot: usize,
    gesture_tool: Tool,
    appearance_captured: bool,
    update_rx: Option<Receiver<String>>,
    close_prompted: bool,
    preview_dirty: Option<(u32, u32, u32, u32)>,
    contour_cache: Vec<Vec<(f32, f32)>>,
    contour_key: u64,
    live_layers: Vec<LiveLayer>,
    live_under: Option<(egui::TextureHandle, f32, f32, f32)>,
    live_under_rx: Option<Receiver<PreviewMsg>>,
    live_drag_on: bool,
    live_hold: bool,
    defer_composite: bool,
    filter_rx: Option<Receiver<(uuid::Uuid, Raster)>>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);
        let zh = install_cjk_font(&cc.egui_ctx);
        let (preview_tx, preview_rx) = mpsc::channel();
        let mut app = Self {
            zh,
            editor: None,
            tabs: Vec::new(),
            tool: Tool::Move,
            fg: [0, 0, 0],
            bg: [255, 255, 255],
            brush_size: 24.0,
            brush_hardness: 0.8,
            brush_opacity: 1.0,
            tolerance: 24,
            shape: ShapeKind::Rectangle,
            corner_radius: 0.0,
            line_width: 4.0,
            font_size: 48.0,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit_pending: false,
            show_rulers: true,
            show_grid: false,
            snap: true,
            grid: 32.0,
            status: if zh { "就绪".into() } else { "Ready".into() },
            hover: None,
            modal: Modal::None,
            gesture: Gesture::None,
            crop: None,
            preview: None,
            preview_tx,
            preview_rx,
            compositing: false,
            pending_composite: false,
            generation: 0,
            texture: None,
            texture_dirty: false,
            last_upload: Instant::now(),
            recent: load_recent(),
            renaming: None,
            clone_mark: None,
            clone_offset: None,
            clone_aligned: true,
            clone_sample_all: true,
            paint_mask: false,
            crop_ratio: (0.0, 0.0),
            layer_clip: None,
            last_dab: None,
            poly_points: Vec::new(),
            watch_stamp: None,
            bindings: load_shortcuts(),
            rebinding: None,
            develop_base: None,
            develop_preview: None,
            canvas_rect: None,
            jpeg_preview: None,
            jpeg_preview_quality: 0,
            layer_drag: 0.0,
            layer_drag_id: None,
            tab_slot: 0,
            gesture_tool: Tool::Move,
            appearance_captured: false,
            update_rx: None,
            close_prompted: false,
            preview_dirty: None,
            contour_cache: Vec::new(),
            contour_key: 0,
            live_layers: Vec::new(),
            live_under: None,
            live_under_rx: None,
            live_drag_on: false,
            live_hold: false,
            defer_composite: false,
            filter_rx: None,
        };
        if let Some(path) = std::env::args().nth(1) {
            let path = PathBuf::from(path);
            if path.is_dir() {
                app.open_project(&path);
            } else {
                app.open_image_path(&path);
            }
        }
        app
    }

    fn t<'a>(&self, zh: &'a str, en: &'a str) -> &'a str {
        if self.zh { zh } else { en }
    }

    fn note(&mut self, zh: &str, en: &str) {
        self.status = if self.zh { zh } else { en }.to_string();
    }

    fn fail(&mut self, err: impl Into<String>) {
        let err = err.into();
        self.status = err.clone();
        self.modal = Modal::Error(err);
    }

    fn dirty(&self) -> bool {
        self.editor.as_ref().is_some_and(|editor| editor.dirty)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_preview(ui.ctx());
        self.poll_filter();
        if self.filter_rx.is_some() || self.live_hold || self.live_under_rx.is_some() {
            ui.ctx().request_repaint();
        }
        self.poll_update();
        self.handle_drops(ui.ctx());
        self.handle_shortcuts(ui.ctx());
        let close_requested = ui.ctx().input(|i| i.viewport().close_requested());
        if close_requested && self.dirty() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !self.close_prompted {
                self.close_prompted = true;
                self.modal = Modal::Confirm {
                    message: self.t("当前工程有未保存的修改。", "This project has unsaved changes.").into(),
                    then: Confirm::Quit,
                };
            }
        }
        if !close_requested {
            self.close_prompted = false;
        }
        let typing = ui.ctx().egui_wants_keyboard_input();
        let pointer_down = ui.input(|i| i.pointer.any_down());
        if !typing && !pointer_down {
            self.appearance_captured = false;
        }
        let title = match &self.editor {
            Some(editor) => format!("{} — Compositor", editor.title()),
            None => "Compositor".into(),
        };
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(title));

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        if self.editor.is_some() || !self.tabs.is_empty() {
            let tab_frame = egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(8, 6))
                .fill(Color32::from_rgb(70, 70, 70));
            egui::Panel::top("tabs").exact_size(40.0).frame(tab_frame).show(ui, |ui| self.tab_bar(ui));
        }
        egui::Panel::top("options").exact_size(34.0).show(ui, |ui| self.options(ui));
        egui::Panel::bottom("status").exact_size(26.0).show(ui, |ui| self.status_bar(ui));
        let tool_frame = egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(4, 6))
            .fill(Color32::from_rgb(46, 46, 46));
        egui::Panel::left("tools").exact_size(48.0).resizable(false).frame(tool_frame).show(ui, |ui| self.tool_rail(ui));
        egui::Panel::right("layers").default_size(280.0).min_size(220.0).show(ui, |ui| self.side_panel(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(CANVAS_BG)).show(ui, |ui| self.canvas(ui));
        self.modals(ui.ctx());
        if self.texture_dirty && (self.preview_dirty.is_some() || self.stroke_active() || self.last_upload.elapsed().as_millis() > 32) {
            self.upload_texture(ui.ctx());
        }
        if self.defer_composite && !ui.input(|i| i.pointer.any_down()) && !self.transform_gesture() {
            self.defer_composite = false;
            self.request_composite();
        }
    }
}

impl App {
    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(self.t("文件", "File"), |ui| {
                if ui.button(self.t("新建…    Ctrl+N", "New…    Ctrl+N")).clicked() {
                    self.modal = Modal::New { width: 1920, height: 1080, ppi: 72.0, white: true };
                    ui.close();
                }
                if ui.button(self.t("打开工程…", "Open Project…")).clicked() {
                    ui.close();
                    self.pick_project();
                }
                if ui.button(self.t("打开图像…    Ctrl+O", "Open Image…    Ctrl+O")).clicked() {
                    ui.close();
                    self.pick_image();
                }
                if !self.recent.is_empty() {
                    ui.menu_button(self.t("最近打开", "Recent"), |ui| {
                        let recent = self.recent.clone();
                        for path in recent.into_iter().take(8) {
                            let label = path.display().to_string();
                            if ui.button(label).clicked() {
                                ui.close();
                                self.open_any(&path);
                            }
                        }
                    });
                }
                ui.separator();
                if ui.button(self.t("保存    Ctrl+S", "Save    Ctrl+S")).clicked() {
                    ui.close();
                    self.save(false);
                }
                if ui.button(self.t("另存为…", "Save As…")).clicked() {
                    ui.close();
                    self.save(true);
                }
                if ui.button(self.t("导出 PNG…", "Export PNG…")).clicked() {
                    ui.close();
                    self.export_png();
                }
                if ui.button(self.t("导出 JPEG…", "Export JPEG…")).clicked() {
                    ui.close();
                    self.export_jpeg_dialog();
                }
                ui.separator();
                if ui.button(self.t("退出", "Quit")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(self.t("编辑", "Edit"), |ui| {
                if ui.button(self.t("撤销    Ctrl+Z", "Undo    Ctrl+Z")).clicked() {
                    self.do_undo();
                    ui.close();
                }
                if ui.button(self.t("重做    Ctrl+Shift+Z", "Redo    Ctrl+Shift+Z")).clicked() {
                    self.do_redo();
                    ui.close();
                }
                ui.separator();
                if ui.button(self.t("填充前景色    Alt+Backspace", "Fill Foreground")).clicked() {
                    self.fill_fg();
                    ui.close();
                }
                if ui.button(self.t("填充背景色", "Fill Background")).clicked() {
                    self.fill_bg();
                    ui.close();
                }
                if ui.button(self.t("内容识别填充", "Content-Aware Fill")).clicked() {
                    self.content_fill_selection();
                    ui.close();
                }
                if ui.button(self.t("清除选区像素", "Clear Selection")).clicked() {
                    self.clear_selection_pixels();
                    ui.close();
                }
                if ui.button(self.t("复制合并图像", "Copy Merged")).clicked() {
                    self.copy_merged();
                    ui.close();
                }
                if ui.button(self.t("粘贴为图层", "Paste as Layer")).clicked() {
                    self.paste_layer();
                    ui.close();
                }
            });
            ui.menu_button(self.t("图像", "Image"), |ui| {
                if ui.button(self.t("图像大小…", "Image Size…")).clicked() {
                    self.open_image_size();
                    ui.close();
                }
                if ui.button(self.t("画布大小…", "Canvas Size…")).clicked() {
                    self.open_canvas_size();
                    ui.close();
                }
                if ui.button(self.t("裁剪到选区", "Crop to Selection")).clicked() {
                    self.crop_to_selection();
                    ui.close();
                }
                ui.separator();
                if ui.button(self.t("顺时针旋转 90°", "Rotate 90° CW")).clicked() {
                    self.mutate(|ed| ed.document.rotate_canvas(true));
                    ui.close();
                }
                if ui.button(self.t("逆时针旋转 90°", "Rotate 90° CCW")).clicked() {
                    self.mutate(|ed| ed.document.rotate_canvas(false));
                    ui.close();
                }
                if ui.button(self.t("水平翻转画布", "Flip Canvas Horizontal")).clicked() {
                    self.mutate(|ed| ed.document.flip_canvas(true));
                    ui.close();
                }
                if ui.button(self.t("垂直翻转画布", "Flip Canvas Vertical")).clicked() {
                    self.mutate(|ed| ed.document.flip_canvas(false));
                    ui.close();
                }
                ui.separator();
                if ui.button(self.t("色阶…", "Levels…")).clicked() {
                    self.modal = Modal::Levels { black: 0.0, gamma: 1.0, white: 255.0, as_layer: true };
                    ui.close();
                }
                if ui.button(self.t("色相/饱和度…", "Hue/Saturation…")).clicked() {
                    self.modal = Modal::Hue { hue: 0.0, sat: 0.0, light: 0.0, as_layer: true };
                    ui.close();
                }
                if ui.button(self.t("曝光…", "Exposure…")).clicked() {
                    self.modal = Modal::Exposure { exposure: 0.0, offset: 0.0, gamma: 1.0, as_layer: true };
                    ui.close();
                }
                if ui.button(self.t("反相", "Invert")).clicked() {
                    self.add_adjustment(Adjustment::Invert);
                    ui.close();
                }
                if ui.button(self.t("去色", "Desaturate")).clicked() {
                    self.desaturate();
                    ui.close();
                }
                if ui.button(self.t("曲线…", "Curves…")).clicked() {
                    self.modal = Modal::Curves { points: vec![(0.0, 0.0), (128.0, 128.0), (255.0, 255.0)] };
                    ui.close();
                }
                if ui.button(self.t("渐变映射…", "Gradient Map…")).clicked() {
                    self.modal = Modal::GradientMap { shadow: [0, 0, 0], highlight: [255, 255, 255] };
                    ui.close();
                }
                if ui.button(self.t("动感模糊…", "Motion Blur…")).clicked() {
                    self.modal = Modal::Motion { angle: 0.0, distance: 16.0 };
                    ui.close();
                }
                if ui.button(self.t("颗粒…", "Grain…")).clicked() {
                    self.modal = Modal::Grain { amount: 25.0 };
                    ui.close();
                }
                if ui.button(self.t("色彩平衡…", "Color Balance…")).clicked() {
                    self.modal = Modal::Balance { cyan_red: 0.0, magenta_green: 0.0, yellow_blue: 0.0 };
                    ui.close();
                }
                if ui.button(self.t("晕影…", "Vignette…")).clicked() {
                    self.modal = Modal::Vignette { amount: 0.45 };
                    ui.close();
                }
                if ui.button(self.t("黑白…", "Black & White…")).clicked() {
                    self.modal = Modal::BlackWhite { weights: [40.0, 60.0, 40.0, 60.0, 20.0, 80.0] };
                    ui.close();
                }
                if ui.button(self.t("自动色阶", "Auto Levels")).clicked() {
                    self.auto_levels();
                    ui.close();
                }
                if ui.button(self.t("修剪", "Trim")).clicked() {
                    self.trim_canvas();
                    ui.close();
                }
                if ui.button(self.t("去除背景", "Remove Background")).clicked() {
                    self.remove_background();
                    ui.close();
                }
                if ui.button(self.t("显影…", "Develop…")).clicked() {
                    self.develop_base = None;
                    self.modal = Modal::Develop(crate::ops::DevelopSettings::default());
                    ui.close();
                }
            });
            ui.menu_button(self.t("图层", "Layer"), |ui| {
                if ui.button(self.t("新建图层    Shift+Ctrl+N", "New Layer")).clicked() {
                    self.new_layer();
                    ui.close();
                }
                if ui.button(self.t("复制图层    Ctrl+J", "Duplicate    Ctrl+J")).clicked() {
                    self.duplicate();
                    ui.close();
                }
                if ui.button(self.t("删除图层", "Delete Layer")).clicked() {
                    self.delete_active();
                    ui.close();
                }
                if ui.button(self.t("新建组", "New Group")).clicked() {
                    self.new_group();
                    ui.close();
                }
                ui.separator();
                if ui.button(self.t("上移", "Bring Forward")).clicked() {
                    self.reorder(true);
                    ui.close();
                }
                if ui.button(self.t("下移", "Send Backward")).clicked() {
                    self.reorder(false);
                    ui.close();
                }
                if ui.button(self.t("向下合并    Ctrl+E", "Merge Down    Ctrl+E")).clicked() {
                    self.merge_down();
                    ui.close();
                }
                if ui.button(self.t("合并可见", "Merge Visible")).clicked() {
                    self.merge_visible();
                    ui.close();
                }
                if ui.button(self.t("拼合图像", "Flatten")).clicked() {
                    self.flatten();
                    ui.close();
                }
                ui.separator();
                if ui.button(self.t("水平翻转图层", "Flip Layer Horizontal")).clicked() {
                    self.flip_layer(true);
                    ui.close();
                }
                if ui.button(self.t("垂直翻转图层", "Flip Layer Vertical")).clicked() {
                    self.flip_layer(false);
                    ui.close();
                }
                if ui.button(self.t("合并组", "Merge Group")).clicked() {
                    self.merge_group();
                    ui.close();
                }
                if ui.button(self.t("复制图层", "Copy Layer")).clicked() {
                    self.copy_layer();
                    ui.close();
                }
                if ui.button(self.t("粘贴图层", "Paste Layer")).clicked() {
                    self.paste_copied_layer();
                    ui.close();
                }
                if ui.button(self.t("添加蒙版", "Add Mask")).clicked() {
                    self.add_mask();
                    ui.close();
                }
                if ui.button(self.t("反相蒙版", "Invert Mask")).clicked() {
                    self.invert_mask();
                    ui.close();
                }
                if ui.button(self.t("图层样式…", "Layer Effects…")).clicked() {
                    self.open_effects();
                    ui.close();
                }
            });
            ui.menu_button(self.t("选择", "Select"), |ui| {
                if ui.button(self.t("全选    Ctrl+A", "All    Ctrl+A")).clicked() {
                    self.select_all();
                    ui.close();
                }
                if ui.button(self.t("取消选择    Ctrl+D", "Deselect    Ctrl+D")).clicked() {
                    self.deselect();
                    ui.close();
                }
                if ui.button(self.t("反选    Ctrl+Shift+I", "Inverse")).clicked() {
                    self.invert_selection();
                    ui.close();
                }
                if ui.button(self.t("载入图层选区", "Load Layer Alpha")).clicked() {
                    self.load_alpha_selection();
                    ui.close();
                }
                if ui.button(self.t("羽化…", "Feather…")).clicked() {
                    self.modal = Modal::Feather { radius: 8.0 };
                    ui.close();
                }
                if ui.button(self.t("扩展…", "Expand…")).clicked() {
                    self.modal = Modal::Expand { radius: 4.0, grow: true };
                    ui.close();
                }
                if ui.button(self.t("收缩…", "Contract…")).clicked() {
                    self.modal = Modal::Expand { radius: 4.0, grow: false };
                    ui.close();
                }
                if ui.button(self.t("选择主体", "Select Subject")).clicked() {
                    self.select_subject();
                    ui.close();
                }
                if ui.button(self.t("剪切选区为图层", "Cut Selection to Layer")).clicked() {
                    self.cut_selection_to_layer();
                    ui.close();
                }
            });
            ui.menu_button(self.t("滤镜", "Filter"), |ui| {
                if ui.button(self.t("高斯模糊…", "Gaussian Blur…")).clicked() {
                    self.modal = Modal::Blur { radius: 4.0 };
                    ui.close();
                }
                if ui.button(self.t("光晕", "Bloom")).clicked() {
                    self.filter_bloom();
                    ui.close();
                }
                if ui.button(self.t("色调对比", "Tonal Contrast")).clicked() {
                    self.filter_tonal();
                    ui.close();
                }
                if ui.button(self.t("镜头校正", "Lens Correction")).clicked() {
                    self.filter_lens();
                    ui.close();
                }
                if ui.button(self.t("添加杂色…", "Add Noise…")).clicked() {
                    self.modal = Modal::Noise { amount: 20.0 };
                    ui.close();
                }
                if ui.button(self.t("动感模糊…", "Motion Blur…")).clicked() {
                    self.modal = Modal::Motion { angle: 0.0, distance: 16.0 };
                    ui.close();
                }
            });
            ui.menu_button(self.t("视图", "View"), |ui| {
                if ui.button(self.t("放大    Ctrl++", "Zoom In")).clicked() {
                    self.zoom_by(1.25);
                    ui.close();
                }
                if ui.button(self.t("缩小    Ctrl+-", "Zoom Out")).clicked() {
                    self.zoom_by(1.0 / 1.25);
                    ui.close();
                }
                if ui.button(self.t("适合窗口    Ctrl+0", "Fit    Ctrl+0")).clicked() {
                    self.fit_pending = true;
                    ui.close();
                }
                if ui.button(self.t("实际像素    Ctrl+1", "100%    Ctrl+1")).clicked() {
                    self.zoom_center(1.0);
                    ui.close();
                }
                ui.separator();
                let rulers = self.t("标尺    Ctrl+R", "Rulers");
                let grid = self.t("网格", "Grid");
                let snap = self.t("对齐", "Snap");
                if ui.checkbox(&mut self.show_rulers, rulers).changed() {}
                ui.checkbox(&mut self.show_grid, grid);
                ui.checkbox(&mut self.snap, snap);
            });
            ui.menu_button(self.t("帮助", "Help"), |ui| {
                if ui.button(self.t("快捷键", "Shortcuts")).clicked() {
                    self.modal = Modal::Shortcuts;
                    ui.close();
                }
                if ui.button(self.t("检查更新", "Check for Updates")).clicked() {
                    self.check_updates();
                    ui.close();
                }
                if ui.button(self.t("关于", "About")).clicked() {
                    self.modal = Modal::About;
                    ui.close();
                }
            });
        });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let mut visual: Vec<(String, bool, usize)> = Vec::new();
        let slot = self.tab_slot.min(self.tabs.len());
        let mut visual_index = 0usize;
        for (index, editor) in self.tabs.iter().enumerate() {
            if index == slot {
                if let Some(active) = &self.editor {
                    visual.push((short_title(&active.title()), true, visual_index));
                    visual_index += 1;
                }
            }
            visual.push((short_title(&editor.title()), false, visual_index));
            visual_index += 1;
        }
        if slot >= self.tabs.len() {
            if let Some(active) = &self.editor {
                visual.push((short_title(&active.title()), true, visual_index));
            }
        }
        let mut activate = None;
        let mut close_at = None;
        let bar = Color32::from_rgb(70, 70, 70);
        ui.painter().rect_filled(ui.max_rect(), 0.0, bar);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let add_w = 36.0;
            egui::ScrollArea::horizontal()
                .id_salt("tabs")
                .max_width((ui.available_width() - add_w).max(40.0))
                .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for (title, active, index) in &visual {
                            let fill = if *active { ACCENT } else { Color32::from_rgb(96, 96, 96) };
                            let frame = egui::Frame::new()
                                .fill(fill)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(10, 3));
                            frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let text = egui::RichText::new(title.as_str()).color(Color32::WHITE);
                                    let label = ui.add(egui::Label::new(text).sense(Sense::click()));
                                    if label.clicked() && !*active {
                                        activate = Some(*index);
                                    }
                                    if label.middle_clicked() {
                                        close_at = Some(*index);
                                    }
                                    let close = egui::Button::new(egui::RichText::new("×").color(Color32::WHITE))
                                        .frame(false)
                                        .min_size(vec2(16.0, 16.0));
                                    if ui.add(close).clicked() {
                                        close_at = Some(*index);
                                    }
                                });
                            });
                        }
                    });
                });
            if ui.button("+").on_hover_text(self.t("新建标签", "New tab")).clicked() {
                self.modal = Modal::New { width: 1920, height: 1080, ppi: 72.0, white: true };
            }
        });
        if let Some(index) = close_at {
            self.close_visual(index);
        } else if let Some(index) = activate {
            self.activate_visual(index);
        }
    }

    fn options(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().slider_width = 88.0;
        egui::ScrollArea::horizontal().id_salt("options").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(self.tool.name(self.zh));
            ui.separator();
            match self.tool {
                Tool::Brush | Tool::Eraser | Tool::Clone | Tool::Heal | Tool::Blur | Tool::Liquify => {
                    ui.label(self.t("大小", "Size"));
                    ui.add(egui::Slider::new(&mut self.brush_size, 1.0..=400.0).logarithmic(true));
                    if matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::Clone) {
                        ui.label(self.t("硬度", "Hardness"));
                        ui.add(egui::Slider::new(&mut self.brush_hardness, 0.0..=1.0));
                        ui.label(self.t("不透明度", "Opacity"));
                        ui.add(egui::Slider::new(&mut self.brush_opacity, 0.02..=1.0));
                    }
                    if matches!(self.tool, Tool::Brush | Tool::Eraser) {
                        let mask = self.t("绘制蒙版", "Paint mask");
                        ui.checkbox(&mut self.paint_mask, mask);
                        ui.label(self.t("Shift 直线", "Shift line"));
                    }
                    if self.tool == Tool::Clone {
                        let aligned = self.t("对齐", "Aligned");
                        ui.checkbox(&mut self.clone_aligned, aligned);
                        let all = self.t("全部图层", "All layers");
                        ui.checkbox(&mut self.clone_sample_all, all);
                        ui.label(self.t("Alt 点击设源", "Alt-click sets source"));
                    }
                    if self.tool == Tool::Heal {
                        ui.label(self.t("在瑕疵上拖动", "Drag over a spot"));
                    }
                }
                Tool::Bucket | Tool::Wand => {
                    ui.label(self.t("容差", "Tolerance"));
                    ui.add(egui::Slider::new(&mut self.tolerance, 0..=255));
                    ui.label(self.t("点击填充或选色", "Click to fill or select"));
                }
                Tool::Shape => {
                    let rect = self.t("矩形", "Rect");
                    let ellipse = self.t("椭圆", "Ellipse");
                    let line = self.t("直线", "Line");
                    ui.selectable_value(&mut self.shape, ShapeKind::Rectangle, rect);
                    ui.selectable_value(&mut self.shape, ShapeKind::Ellipse, ellipse);
                    ui.selectable_value(&mut self.shape, ShapeKind::Line, line);
                    ui.label(self.t("圆角", "Radius"));
                    ui.add(egui::Slider::new(&mut self.corner_radius, 0.0..=200.0));
                    ui.label(self.t("线宽", "Width"));
                    ui.add(egui::Slider::new(&mut self.line_width, 1.0..=64.0));
                }
                Tool::Type => {
                    ui.label(self.t("字号", "Size"));
                    ui.add(egui::Slider::new(&mut self.font_size, 8.0..=400.0));
                    ui.label(self.t("点击画布输入文字", "Click the canvas to type"));
                }
                Tool::Poly => {
                    ui.label(self.t("逐点点击，Enter 闭合，Esc 取消", "Click points, Enter closes, Esc cancels"));
                }
                Tool::Crop => {
                    ui.label(self.t("比例", "Ratio"));
                    if ui.button(self.t("自由", "Free")).clicked() { self.crop_ratio = (0.0, 0.0); }
                    if ui.button("1:1").clicked() { self.crop_ratio = (1.0, 1.0); }
                    if ui.button("3:4").clicked() { self.crop_ratio = (3.0, 4.0); }
                    if ui.button("9:16").clicked() { self.crop_ratio = (9.0, 16.0); }
                    ui.label(self.t("Alt 从中心，Enter 应用", "Alt from center, Enter applies"));
                }
                Tool::Move => {
                    ui.label(self.t("拖动图层。手柄缩放，圆点旋转。Alt+角点扭曲。", "Drag a layer. Handles scale, the stem rotates. Alt+corner warps."));
                }
                Tool::Hand => {
                    ui.label(self.t("拖动平移。滚轮平移，Ctrl+滚轮缩放。", "Drag to pan. Scroll pans, Ctrl+scroll zooms."));
                }
                Tool::Zoom => {
                    ui.label(self.t("点击放大，Alt 点击缩小。滚轮平移，Ctrl+滚轮缩放。", "Click zooms in, Alt zooms out. Scroll pans, Ctrl+scroll zooms."));
                }
                Tool::Marquee | Tool::Ellipse | Tool::Lasso => {
                    ui.label(self.t("拖动建立选区。Shift 加选，Alt 减选。", "Drag a selection. Shift adds, Alt subtracts."));
                }
                _ => {}
            }
        });
        });
    }

    fn tool_rail(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("tools")
            .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.vertical_centered(|ui| {
                    for tool in TOOLS {
                        let selected = self.tool == tool;
                        let (rect, response) = ui.allocate_exact_size(vec2(36.0, 32.0), Sense::click());
                        let fill = if selected {
                            ACCENT
                        } else if response.hovered() {
                            Color32::from_rgb(96, 96, 96)
                        } else {
                            Color32::from_rgb(68, 68, 68)
                        };
                        ui.painter().rect_filled(rect, 5.0, fill);
                        paint_tool_icon(ui.painter(), rect, tool, Color32::WHITE);
                        if response.clicked() {
                            self.set_tool(tool);
                        }
                        response.on_hover_text(format!("{} ({})", tool.name(self.zh), tool.letter()));
                    }
                });
            });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        let inspector_h = (ui.available_height() * 0.34).clamp(96.0, 200.0);
        egui::ScrollArea::vertical().id_salt("inspector").max_height(inspector_h).show(ui, |ui| {
            ui.label(self.t("颜色", "Color"));
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size = vec2(28.0, 22.0);
                ui.color_edit_button_srgb(&mut self.fg);
                ui.label(self.t("前景", "FG"));
                ui.color_edit_button_srgb(&mut self.bg);
                ui.label(self.t("背景", "BG"));
                if ui.button("X").on_hover_text(self.t("交换", "Swap")).clicked() {
                    std::mem::swap(&mut self.fg, &mut self.bg);
                }
                if ui.button("D").on_hover_text(self.t("恢复黑白", "Default colors")).clicked() {
                    self.fg = [0, 0, 0];
                    self.bg = [255, 255, 255];
                }
            });
            ui.separator();
            self.inspector_controls(ui);
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("+").on_hover_text(self.t("新建图层", "New layer")).clicked() {
                self.new_layer();
            }
            if ui.button(self.t("组", "Grp")).on_hover_text(self.t("新建组", "New group")).clicked() {
                self.new_group();
            }
            if ui.button(self.t("复制", "Dup")).clicked() {
                self.duplicate();
            }
            if ui.button(self.t("删除", "Del")).clicked() {
                self.delete_active();
            }
            if ui.button("↑").on_hover_text(self.t("上移", "Bring forward")).clicked() {
                self.reorder(true);
            }
            if ui.button("↓").on_hover_text(self.t("下移", "Send backward")).clicked() {
                self.reorder(false);
            }
        });
        ui.label(self.t("图层", "Layers"));
        let rows = self.editor.as_ref().map(|ed| ed.document.panel_rows()).unwrap_or_default();
        let active = self.editor.as_ref().and_then(|ed| ed.document.active_layer_id);
        egui::ScrollArea::vertical().id_salt("layers").auto_shrink([false, false]).show(ui, |ui| {
            if rows.is_empty() {
                ui.weak(self.t("没有图层", "No layers"));
            }
            for (id, depth) in rows {
                self.layer_row(ui, id, depth, active == Some(id));
            }
        });
    }

    fn inspector_controls(&mut self, ui: &mut egui::Ui) {
        let zh = self.zh;
        let appearance = self.editor.as_ref().and_then(|editor| {
            let layer = editor.document.active()?;
            Some((layer.id, layer.opacity, layer.blend_mode, layer.is_group))
        });
        let Some((id, mut opacity, mut mode, is_group)) = appearance else {
            ui.weak(tr(zh, "打开或新建工程后可编辑图层", "Open or create a project to edit layers"));
            return;
        };
        ui.label(tr(zh, "不透明度", "Opacity"));
        let opacity_response = ui.add(egui::Slider::new(&mut opacity, 0.0..=1.0));
        let mut blend_changed = false;
        egui::ComboBox::from_id_salt("blend")
            .selected_text(mode.label(zh))
            .width(ui.available_width().max(80.0))
            .show_ui(ui, |ui| {
                for item in BlendMode::ALL {
                    if ui.selectable_value(&mut mode, item, item.label(zh)).clicked() {
                        blend_changed = true;
                    }
                }
            });
        if opacity_response.changed() || (blend_changed && !is_group) {
            self.note_appearance_edit();
            if let Some(layer) = self.editor.as_mut().and_then(|ed| ed.document.layer_mut(id)) {
                if opacity_response.changed() {
                    layer.opacity = opacity;
                }
                if blend_changed && !layer.is_group {
                    layer.blend_mode = mode;
                }
                layer.touch();
            }
            self.request_composite();
        }
        let xform = self.editor.as_ref().and_then(|editor| {
            editor.document.layer(id).map(|layer| {
                (
                    layer.transform.origin_x,
                    layer.transform.origin_y,
                    layer.transform.width,
                    layer.transform.height,
                    layer.transform.rotation,
                )
            })
        });
        if let Some((mut ox, mut oy, mut width, mut height, mut rotation)) = xform {
            ui.label(tr(zh, "变换", "Transform"));
            let mut changed = false;
            ui.horizontal(|ui| {
                changed |= ui.add(egui::DragValue::new(&mut ox).speed(1.0).prefix("X ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut oy).speed(1.0).prefix("Y ")).changed();
            });
            ui.horizontal(|ui| {
                changed |= ui.add(egui::DragValue::new(&mut width).speed(1.0).range(1.0..=16000.0).prefix("W ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut height).speed(1.0).range(1.0..=16000.0).prefix("H ")).changed();
            });
            changed |= ui.add(egui::DragValue::new(&mut rotation).speed(0.5).suffix("°")).changed();
            if changed {
                self.note_appearance_edit();
                if let Some(layer) = self.editor.as_mut().and_then(|ed| ed.document.layer_mut(id)) {
                    layer.transform.origin_x = ox;
                    layer.transform.origin_y = oy;
                    layer.transform.width = width.max(1.0);
                    layer.transform.height = height.max(1.0);
                    layer.transform.rotation = rotation;
                    layer.touch();
                }
                self.request_composite();
            }
        }
    }

    fn layer_row(&mut self, ui: &mut egui::Ui, id: uuid::Uuid, depth: usize, selected: bool) {
        let zh = self.zh;
        let Some((name, visible, kind)) = self.editor.as_ref().and_then(|editor| {
            let layer = editor.document.layer(id)?;
            let kind = if layer.is_group {
                tr(zh, "组", "Group").to_string()
            } else if let Some(adjustment) = layer.adjustment.as_ref() {
                adjustment.label(zh)
            } else {
                String::new()
            };
            Some((layer.name.clone(), layer.visible, kind))
        }) else {
            return;
        };
        let mut toggle_vis = false;
        let mut select = false;
        let mut rename = false;
        let mut committed = None;
        let mut drag_y = 0.0;
        let mut menu_dup = false;
        let mut menu_del = false;
        let mut menu_merge = false;
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 12.0);
            let eye = if visible { "●" } else { "○" };
            if ui.add(egui::Button::new(eye).frame(false).min_size(vec2(18.0, 18.0))).on_hover_text(tr(zh, "显示/隐藏", "Show or hide")).clicked() {
                toggle_vis = true;
            }
            let label = if kind.is_empty() { name.clone() } else { format!("{name}  {kind}") };
            if self.renaming.as_ref().is_some_and(|(rid, _)| *rid == id) {
                if let Some((_, text)) = self.renaming.as_mut() {
                    let response = ui.text_edit_singleline(text);
                    if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        committed = Some(text.clone());
                    }
                }
            } else {
                let text = egui::RichText::new(label).color(if visible {
                    Color32::from_rgb(230, 230, 230)
                } else {
                    Color32::from_rgb(120, 120, 120)
                });
                let button = egui::Button::new(text)
                    .sense(Sense::click_and_drag())
                    .min_size(vec2(ui.available_width().max(48.0), 22.0))
                    .fill(if selected { ACCENT } else { Color32::TRANSPARENT });
                let response = ui.add(button);
                if response.clicked() || response.secondary_clicked() {
                    select = true;
                }
                if response.double_clicked() {
                    rename = true;
                }
                if response.dragged_by(egui::PointerButton::Primary) {
                    drag_y = response.drag_delta().y;
                    select = true;
                }
                response.context_menu(|ui| {
                    select = true;
                    if ui.button(tr(zh, "复制", "Duplicate")).clicked() {
                        menu_dup = true;
                        ui.close();
                    }
                    if ui.button(tr(zh, "向下合并", "Merge Down")).clicked() {
                        menu_merge = true;
                        ui.close();
                    }
                    if ui.button(tr(zh, "删除", "Delete")).clicked() {
                        menu_del = true;
                        ui.close();
                    }
                });
            }
        });
        if toggle_vis {
            self.note_appearance_edit();
            if let Some(layer) = self.editor.as_mut().and_then(|ed| ed.document.layer_mut(id)) {
                layer.visible = !visible;
                layer.touch();
            }
            self.request_composite();
        }
        if select {
            if let Some(editor) = self.editor.as_mut() {
                editor.document.active_layer_id = Some(id);
            }
        }
        if rename {
            self.renaming = Some((id, name));
        }
        if drag_y.abs() > 0.0 {
            if self.layer_drag_id != Some(id) {
                self.layer_drag_id = Some(id);
                self.layer_drag = 0.0;
            }
            self.layer_drag += drag_y;
            if self.layer_drag < -22.0 {
                self.reorder_id(id, true);
                self.layer_drag = 0.0;
            } else if self.layer_drag > 22.0 {
                self.reorder_id(id, false);
                self.layer_drag = 0.0;
            }
        } else if ui.input(|i| i.pointer.primary_released()) && self.layer_drag_id == Some(id) {
            self.layer_drag = 0.0;
            self.layer_drag_id = None;
        }
        if menu_dup {
            self.duplicate_id(id);
        }
        if menu_merge {
            self.merge_id(id);
        }
        if menu_del {
            self.delete_id(id);
        }
        if let Some(new_name) = committed {
            self.renaming = None;
            if !new_name.trim().is_empty() {
                if let Some(layer) = self.editor.as_mut().and_then(|ed| ed.document.layer_mut(id)) {
                    layer.name = new_name.trim().to_string();
                }
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(editor) = &self.editor {
                ui.label(format!("{} × {}", editor.document.width, editor.document.height));
                ui.separator();
            }
            let zoom = format!("{:.0}%", self.zoom * 100.0);
            if ui.button(zoom).on_hover_text(self.t("点击在 100% 和适合窗口之间切换", "Click to toggle 100% and fit")).clicked() {
                if (self.zoom - 1.0).abs() < 0.03 {
                    self.fit_pending = true;
                } else {
                    self.zoom_center(1.0);
                }
            }
            ui.separator();
            if let Some((x, y, px)) = self.hover {
                ui.label(format!("{x:.0}, {y:.0}"));
                ui.separator();
                ui.label(format!("#{:02X}{:02X}{:02X}", px[0], px[1], px[2]));
                ui.separator();
            }
            if self.compositing {
                ui.label(self.t("正在合成…", "Compositing…"));
                ui.separator();
            }
            ui.label(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(self.t("滚轮平移 · Ctrl+滚轮缩放", "Scroll pans · Ctrl+scroll zooms"));
            });
        });
    }
}

fn paint_tool_icon(painter: &egui::Painter, rect: Rect, tool: Tool, color: Color32) {
    let stroke = egui::Stroke::new(1.6, color);
    let thin = egui::Stroke::new(1.15, color);
    let icon = Rect::from_center_size(rect.center(), vec2(16.0, 16.0));
    let p = |x: f32, y: f32| icon.min + vec2(x, y);
    let line = |a: Pos2, b: Pos2, s: egui::Stroke| painter.line_segment([a, b], s);
    match tool {
        Tool::Move => {
            let c = icon.center();
            line(c + vec2(-6.0, 0.0), c + vec2(6.0, 0.0), stroke);
            line(c + vec2(0.0, -6.0), c + vec2(0.0, 6.0), stroke);
            for (dx, dy) in [(6.0, 0.0), (-6.0, 0.0), (0.0, 6.0), (0.0, -6.0)] {
                let tip = c + vec2(dx, dy);
                let side = if dx.abs() > 0.1 { vec2(0.0, 2.4) } else { vec2(2.4, 0.0) };
                let back = if dx.abs() > 0.1 { vec2(-dx.signum() * 2.6, 0.0) } else { vec2(0.0, -dy.signum() * 2.6) };
                line(tip, tip + back + side, stroke);
                line(tip, tip + back - side, stroke);
            }
        }
        Tool::Marquee => {
            for (x, y, sx, sy) in [(1.0, 1.0, 1.0, 1.0), (15.0, 1.0, -1.0, 1.0), (1.0, 15.0, 1.0, -1.0), (15.0, 15.0, -1.0, -1.0)] {
                line(p(x, y), p(x + sx * 4.5, y), stroke);
                line(p(x, y), p(x, y + sy * 4.5), stroke);
            }
        }
        Tool::Ellipse => {
            painter.add(egui::Shape::ellipse_stroke(icon.center(), vec2(6.5, 5.2), stroke));
        }
        Tool::Lasso => {
            let pts = vec![p(2.0, 9.0), p(4.0, 4.0), p(9.0, 2.0), p(14.0, 5.0), p(13.0, 11.0), p(8.0, 14.0), p(3.0, 12.0)];
            painter.add(egui::Shape::line(pts, stroke));
        }
        Tool::Poly => {
            let pts = vec![p(8.0, 1.5), p(15.0, 6.5), p(12.5, 14.5), p(3.5, 14.5), p(1.0, 6.5)];
            painter.add(egui::Shape::closed_line(pts, stroke));
        }
        Tool::Wand => {
            line(p(3.0, 13.0), p(11.0, 5.0), stroke);
            painter.circle_stroke(p(12.2, 3.6), 2.1, stroke);
            line(p(12.2, 1.0), p(12.2, 2.2), thin);
            line(p(14.6, 3.6), p(15.6, 3.6), thin);
        }
        Tool::Crop => {
            for (x, y, sx, sy) in [(1.0, 1.0, 1.0, 1.0), (15.0, 1.0, -1.0, 1.0), (1.0, 15.0, 1.0, -1.0), (15.0, 15.0, -1.0, -1.0)] {
                line(p(x, y), p(x + sx * 5.0, y), stroke);
                line(p(x, y), p(x, y + sy * 5.0), stroke);
            }
            line(p(5.0, 5.0), p(11.0, 11.0), thin);
        }
        Tool::Eyedropper => {
            line(p(3.5, 12.5), p(10.5, 5.5), stroke);
            painter.circle_stroke(p(12.2, 3.8), 2.3, stroke);
            line(p(2.2, 13.8), p(5.2, 11.2), stroke);
        }
        Tool::Brush => {
            line(p(2.5, 13.5), p(11.5, 4.5), egui::Stroke::new(2.2, color));
            painter.circle_filled(p(12.4, 3.4), 1.7, color);
        }
        Tool::Eraser => {
            let pts = vec![p(3.0, 11.0), p(9.0, 14.5), p(14.0, 7.0), p(8.0, 3.5)];
            painter.add(egui::Shape::closed_line(pts, stroke));
            line(p(6.0, 8.5), p(11.0, 5.5), thin);
        }
        Tool::Bucket => {
            let pts = vec![p(4.0, 7.5), p(11.0, 7.5), p(13.5, 14.5), p(2.0, 14.5)];
            painter.add(egui::Shape::closed_line(pts, stroke));
            line(p(8.0, 7.5), p(12.5, 2.5), stroke);
            painter.circle_filled(p(13.4, 2.2), 1.3, color);
        }
        Tool::Gradient => {
            painter.rect_stroke(Rect::from_min_max(p(1.5, 2.0), p(14.5, 14.0)), 1.0, stroke, egui::StrokeKind::Inside);
            line(p(3.5, 12.0), p(12.5, 4.0), thin);
            painter.circle_filled(p(3.5, 12.0), 1.5, color);
            painter.circle_stroke(p(12.5, 4.0), 1.5, thin);
        }
        Tool::Shape => {
            painter.rect_stroke(Rect::from_min_max(p(2.0, 3.0), p(14.0, 13.0)), 3.0, stroke, egui::StrokeKind::Inside);
        }
        Tool::Type => {
            line(p(3.0, 3.0), p(13.0, 3.0), egui::Stroke::new(2.0, color));
            line(p(8.0, 3.0), p(8.0, 13.5), stroke);
            line(p(5.5, 13.5), p(10.5, 13.5), thin);
        }
        Tool::Hand => {
            painter.circle_stroke(p(8.0, 10.5), 3.4, stroke);
            line(p(5.6, 8.5), p(5.2, 3.5), stroke);
            line(p(8.0, 7.6), p(8.0, 2.0), stroke);
            line(p(10.4, 8.2), p(11.2, 3.8), stroke);
        }
        Tool::Zoom => {
            painter.circle_stroke(p(7.0, 7.0), 4.4, stroke);
            line(p(10.2, 10.2), p(14.5, 14.5), egui::Stroke::new(2.0, color));
            line(p(5.2, 7.0), p(8.8, 7.0), thin);
            line(p(7.0, 5.2), p(7.0, 8.8), thin);
        }
        Tool::Clone => {
            painter.circle_stroke(p(6.0, 9.5), 4.0, stroke);
            painter.circle_stroke(p(10.5, 6.0), 3.2, thin);
        }
        Tool::Heal => {
            painter.circle_stroke(p(8.0, 8.0), 5.5, stroke);
            line(p(8.0, 5.0), p(8.0, 11.0), stroke);
            line(p(5.0, 8.0), p(11.0, 8.0), stroke);
        }
        Tool::Blur => {
            painter.circle_stroke(p(8.0, 8.0), 2.2, stroke);
            painter.circle_stroke(p(8.0, 8.0), 4.4, thin);
            painter.circle_stroke(p(8.0, 8.0), 6.4, egui::Stroke::new(1.0, Color32::from_white_alpha(150)));
        }
        Tool::Liquify => {
            let mut pts = Vec::new();
            for i in 0..=8 {
                let x = 1.0 + i as f32 * 1.75;
                let y = 8.0 + (i as f32 * 0.9).sin() * 4.5;
                pts.push(p(x, y));
            }
            painter.add(egui::Shape::line(pts, stroke));
        }
    }
}

fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = Color32::from_rgb(43, 43, 43);
    visuals.panel_fill = Color32::from_rgb(36, 36, 36);
    visuals.extreme_bg_color = Color32::from_rgb(24, 24, 24);
    visuals.faint_bg_color = Color32::from_rgb(48, 48, 48);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(55, 55, 55);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(70, 70, 70);
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.selection.bg_fill = ACCENT;
    visuals.hyperlink_color = Color32::from_rgb(120, 180, 255);
    ctx.set_visuals(visuals);
}

fn install_cjk_font(ctx: &egui::Context) -> bool {
    let Some((bytes, index)) = load_cjk_font() else {
        return false;
    };
    let mut data = egui::FontData::from_owned(bytes);
    data.index = index;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("cjk".to_owned(), std::sync::Arc::new(data));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.insert(0, "cjk".to_owned());
        }
    }
    ctx.set_fonts(fonts);
    true
}

fn load_cjk_font() -> Option<(Vec<u8>, u32)> {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\msjh.ttc",
        r"C:\Windows\Fonts\Deng.ttf",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    ];
    for path in CANDIDATES {
        if let Some(font) = read_cjk_file(path) {
            return Some(font);
        }
    }
    None
}

fn read_cjk_file(path: &str) -> Option<(Vec<u8>, u32)> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 1024 {
        return None;
    }
    let index = face_with_char(&bytes, '中')?;
    Some((bytes, index))
}

/// Face index whose cmap contains `ch`. A `.ttc` is kept whole; egui selects the face by index.
fn face_with_char(bytes: &[u8], ch: char) -> Option<u32> {
    let code = ch as u32;
    let count = collection_len(bytes);
    for index in 0..count {
        if face_has_char(bytes, index, code) {
            return Some(index);
        }
    }
    None
}

fn collection_len(bytes: &[u8]) -> u32 {
    if bytes.len() >= 12 && &bytes[0..4] == b"ttcf" {
        u32::from_be_bytes(bytes[8..12].try_into().unwrap_or([0, 0, 0, 0])).max(1)
    } else if is_sfnt(bytes) {
        1
    } else {
        0
    }
}

fn is_sfnt(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && (&bytes[0..4] == b"OTTO" || &bytes[0..4] == b"true" || bytes[0..4] == [0, 1, 0, 0])
}

fn face_offset(bytes: &[u8], index: u32) -> Option<usize> {
    if bytes.len() >= 12 && &bytes[0..4] == b"ttcf" {
        let count = u32::from_be_bytes(bytes.get(8..12)?.try_into().ok()?);
        if index >= count {
            return None;
        }
        let p = 12 + index as usize * 4;
        return Some(u32::from_be_bytes(bytes.get(p..p + 4)?.try_into().ok()?) as usize);
    }
    if index == 0 && is_sfnt(bytes) {
        Some(0)
    } else {
        None
    }
}

fn face_has_char(bytes: &[u8], index: u32, code: u32) -> bool {
    let Some(origin) = face_offset(bytes, index) else {
        return false;
    };
    let Some(cmap) = table(bytes, origin, *b"cmap") else {
        return false;
    };
    if cmap.len() < 4 {
        return false;
    }
    let records = u16::from_be_bytes(cmap[2..4].try_into().unwrap_or([0, 0])) as usize;
    for i in 0..records {
        let p = 4 + i * 8;
        if p + 8 > cmap.len() {
            break;
        }
        let platform = u16::from_be_bytes(cmap[p..p + 2].try_into().unwrap());
        let encoding = u16::from_be_bytes(cmap[p + 2..p + 4].try_into().unwrap());
        let unicode = platform == 0 || (platform == 3 && matches!(encoding, 1 | 10));
        if !unicode {
            continue;
        }
        let sub = u32::from_be_bytes(cmap[p + 4..p + 8].try_into().unwrap()) as usize;
        if sub >= cmap.len() {
            continue;
        }
        if cmap_has(&cmap[sub..], code) {
            return true;
        }
    }
    false
}

fn table<'a>(bytes: &'a [u8], origin: usize, tag: [u8; 4]) -> Option<&'a [u8]> {
    if origin + 12 > bytes.len() {
        return None;
    }
    let num = u16::from_be_bytes(bytes[origin + 4..origin + 6].try_into().ok()?) as usize;
    for i in 0..num {
        let p = origin + 12 + i * 16;
        if p + 16 > bytes.len() {
            return None;
        }
        if bytes[p..p + 4] != tag {
            continue;
        }
        let offset = u32::from_be_bytes(bytes[p + 8..p + 12].try_into().ok()?) as usize;
        let length = u32::from_be_bytes(bytes[p + 12..p + 16].try_into().ok()?) as usize;
        return bytes.get(offset..offset + length);
    }
    None
}

fn cmap_has(sub: &[u8], code: u32) -> bool {
    if sub.len() < 2 {
        return false;
    }
    match u16::from_be_bytes(sub[0..2].try_into().unwrap_or([0, 0])) {
        4 => cmap4_has(sub, code),
        12 => cmap12_has(sub, code),
        _ => false,
    }
}

fn cmap4_has(sub: &[u8], code: u32) -> bool {
    if code > 0xFFFF || sub.len() < 14 {
        return false;
    }
    let seg_count = u16::from_be_bytes(sub[6..8].try_into().unwrap_or([0, 0])) as usize / 2;
    if seg_count == 0 {
        return false;
    }
    let end_at = 14;
    let start_at = end_at + seg_count * 2 + 2;
    let delta_at = start_at + seg_count * 2;
    let range_at = delta_at + seg_count * 2;
    if range_at + seg_count * 2 > sub.len() {
        return false;
    }
    let code = code as u16;
    for i in 0..seg_count {
        let end = u16::from_be_bytes(sub[end_at + i * 2..end_at + i * 2 + 2].try_into().unwrap());
        if code > end {
            continue;
        }
        let start = u16::from_be_bytes(sub[start_at + i * 2..start_at + i * 2 + 2].try_into().unwrap());
        if code < start {
            return false;
        }
        let delta = i16::from_be_bytes(sub[delta_at + i * 2..delta_at + i * 2 + 2].try_into().unwrap());
        let range = u16::from_be_bytes(sub[range_at + i * 2..range_at + i * 2 + 2].try_into().unwrap()) as usize;
        let glyph = if range == 0 {
            code.wrapping_add(delta as u16)
        } else {
            let at = range_at + i * 2 + range + (code - start) as usize * 2;
            let Some(raw) = sub.get(at..at + 2) else {
                return false;
            };
            let id = u16::from_be_bytes(raw.try_into().unwrap());
            if id == 0 {
                0
            } else {
                id.wrapping_add(delta as u16)
            }
        };
        return glyph != 0;
    }
    false
}

fn cmap12_has(sub: &[u8], code: u32) -> bool {
    if sub.len() < 16 {
        return false;
    }
    let groups = u32::from_be_bytes(sub[12..16].try_into().unwrap_or([0, 0, 0, 0])) as usize;
    let mut p = 16;
    for _ in 0..groups {
        let Some(group) = sub.get(p..p + 12) else {
            return false;
        };
        let start = u32::from_be_bytes(group[0..4].try_into().unwrap());
        let end = u32::from_be_bytes(group[4..8].try_into().unwrap());
        if code < start {
            return false;
        }
        if code <= end {
            return u32::from_be_bytes(group[8..12].try_into().unwrap()) != 0;
        }
        p += 12;
    }
    false
}

pub fn run() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([960.0, 640.0])
        .with_title("Compositor");
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "Compositor",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

fn load_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(include_bytes!("../assets/icon.png")).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

fn config_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("CompositorCross");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config").join("compositor-cross");
    }
    PathBuf::from(".compositor-cross")
}

fn load_recent() -> Vec<PathBuf> {
    let path = config_dir().join("recent.json");
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    serde_json::from_str::<Vec<String>>(&text)
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect()
}

fn default_shortcuts() -> Vec<(&'static str, &'static str)> {
    vec![
        ("undo", "Ctrl+Z"),
        ("redo", "Ctrl+Shift+Z"),
        ("new", "Ctrl+N"),
        ("open", "Ctrl+O"),
        ("save", "Ctrl+S"),
        ("select-all", "Ctrl+A"),
        ("deselect", "Ctrl+D"),
        ("invert-sel", "Ctrl+Shift+I"),
        ("duplicate", "Ctrl+J"),
        ("merge", "Ctrl+E"),
        ("fit", "Ctrl+0"),
        ("actual", "Ctrl+1"),
        ("rulers", "Ctrl+R"),
        ("brush-down", "["),
        ("brush-up", "]"),
        ("swap", "X"),
        ("move", "V"),
        ("brush", "B"),
        ("eraser", "E"),
        ("marquee", "M"),
        ("eyedropper", "I"),
        ("hand", "H"),
        ("zoom", "Z"),
        ("crop", "C"),
        ("gradient", "G"),
        ("type", "T"),
        ("lasso", "L"),
        ("wand", "W"),
        ("clone", "S"),
        ("heal", "J"),
        ("blur", "R"),
        ("liquify", "P"),
        ("ellipse", "O"),
        ("bucket", "K"),
        ("shape", "U"),
        ("poly", "Y"),
        ("zoom-in", "Ctrl+="),
        ("zoom-out", "Ctrl+-"),
        ("clear", "Delete"),
        ("nudge-left", "Left"),
        ("nudge-right", "Right"),
        ("nudge-up", "Up"),
        ("nudge-down", "Down"),
    ]
}

fn load_shortcuts() -> std::collections::HashMap<String, String> {
    let mut map: std::collections::HashMap<String, String> = default_shortcuts().into_iter().map(|(id, combo)| (id.to_string(), combo.to_string())).collect();
    if let Ok(text) = std::fs::read_to_string(config_dir().join("shortcuts.json")) {
        if let Ok(saved) = serde_json::from_str::<std::collections::HashMap<String, String>>(&text) {
            map.extend(saved);
        }
    }
    map
}

fn save_shortcuts(bindings: &std::collections::HashMap<String, String>) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(text) = serde_json::to_string_pretty(bindings) {
        let _ = std::fs::write(dir.join("shortcuts.json"), text);
    }
}

fn shortcut_label(id: &str, zh: bool) -> &'static str {
    match (id, zh) {
        ("undo", true) => "撤销",
        ("undo", false) => "Undo",
        ("redo", true) => "重做",
        ("redo", false) => "Redo",
        ("new", true) => "新建",
        ("new", false) => "New",
        ("open", true) => "打开图像",
        ("open", false) => "Open Image",
        ("save", true) => "保存",
        ("save", false) => "Save",
        ("select-all", true) => "全选",
        ("select-all", false) => "Select All",
        ("deselect", true) => "取消选择",
        ("deselect", false) => "Deselect",
        ("invert-sel", true) => "反选",
        ("invert-sel", false) => "Inverse",
        ("duplicate", true) => "复制图层",
        ("duplicate", false) => "Duplicate",
        ("merge", true) => "向下合并",
        ("merge", false) => "Merge Down",
        ("fit", true) => "适合窗口",
        ("fit", false) => "Fit",
        ("actual", true) => "实际像素",
        ("actual", false) => "100%",
        ("rulers", true) => "标尺",
        ("rulers", false) => "Rulers",
        ("brush-down", true) => "减小画笔",
        ("brush-down", false) => "Smaller Brush",
        ("brush-up", true) => "增大画笔",
        ("brush-up", false) => "Larger Brush",
        ("swap", true) => "交换颜色",
        ("swap", false) => "Swap Colors",
        ("move", true) => "移动",
        ("move", false) => "Move",
        ("brush", true) => "画笔",
        ("brush", false) => "Brush",
        ("eraser", true) => "橡皮",
        ("eraser", false) => "Eraser",
        ("marquee", true) => "选框",
        ("marquee", false) => "Marquee",
        ("eyedropper", true) => "吸管",
        ("eyedropper", false) => "Eyedropper",
        ("hand", true) => "抓手",
        ("hand", false) => "Hand",
        ("zoom", true) => "缩放",
        ("zoom", false) => "Zoom",
        ("crop", true) => "裁剪",
        ("crop", false) => "Crop",
        ("gradient", true) => "渐变",
        ("gradient", false) => "Gradient",
        ("type", true) => "文字",
        ("type", false) => "Type",
        ("lasso", true) => "套索",
        ("lasso", false) => "Lasso",
        ("wand", true) => "魔棒",
        ("wand", false) => "Wand",
        ("clone", true) => "仿制",
        ("clone", false) => "Clone",
        ("heal", true) => "修复",
        ("heal", false) => "Heal",
        ("blur", true) => "模糊",
        ("blur", false) => "Blur",
        ("liquify", true) => "液化",
        ("liquify", false) => "Liquify",
        ("nudge-left", true) => "左移",
        ("nudge-left", false) => "Nudge Left",
        ("nudge-right", true) => "右移",
        ("nudge-right", false) => "Nudge Right",
        ("nudge-up", true) => "上移",
        ("nudge-up", false) => "Nudge Up",
        ("nudge-down", true) => "下移",
        ("nudge-down", false) => "Nudge Down",
        ("ellipse", true) => "椭圆选框",
        ("ellipse", false) => "Ellipse",
        ("bucket", true) => "油漆桶",
        ("bucket", false) => "Bucket",
        ("shape", true) => "形状",
        ("shape", false) => "Shape",
        ("poly", true) => "多边形套索",
        ("poly", false) => "Polygonal Lasso",
        ("zoom-in", true) => "放大",
        ("zoom-in", false) => "Zoom In",
        ("zoom-out", true) => "缩小",
        ("zoom-out", false) => "Zoom Out",
        ("clear", true) => "清除选区或删除图层",
        ("clear", false) => "Clear or Delete Layer",
        _ => "Action",
    }
}

fn short_title(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    if chars.len() > 24 {
        format!("{}…", chars[..23].iter().collect::<String>())
    } else {
        title.to_string()
    }
}

fn pressed_combo(i: &egui::InputState) -> Option<String> {
    const KEYS: &[(egui::Key, &str)] = &[
        (egui::Key::A, "A"), (egui::Key::B, "B"), (egui::Key::C, "C"), (egui::Key::D, "D"),
        (egui::Key::E, "E"), (egui::Key::G, "G"), (egui::Key::H, "H"), (egui::Key::I, "I"),
        (egui::Key::J, "J"), (egui::Key::L, "L"), (egui::Key::M, "M"), (egui::Key::N, "N"),
        (egui::Key::O, "O"), (egui::Key::P, "P"), (egui::Key::R, "R"), (egui::Key::S, "S"),
        (egui::Key::T, "T"), (egui::Key::V, "V"), (egui::Key::W, "W"), (egui::Key::X, "X"),
        (egui::Key::Y, "Y"), (egui::Key::Z, "Z"),
        (egui::Key::OpenBracket, "["), (egui::Key::CloseBracket, "]"),
        (egui::Key::Equals, "="), (egui::Key::Plus, "+"), (egui::Key::Minus, "-"),
        (egui::Key::Num0, "0"), (egui::Key::Num1, "1"),
        (egui::Key::Delete, "Delete"), (egui::Key::Backspace, "Backspace"),
        (egui::Key::ArrowLeft, "Left"), (egui::Key::ArrowRight, "Right"),
        (egui::Key::ArrowUp, "Up"), (egui::Key::ArrowDown, "Down"),
    ];
    let (_, name) = KEYS.iter().find(|(key, _)| i.key_pressed(*key))?;
    let name = match *name {
        "+" => "=",
        "Backspace" => "Delete",
        other => other,
    };
    let mut parts = Vec::new();
    if i.modifiers.command { parts.push("Ctrl"); }
    if i.modifiers.shift { parts.push("Shift"); }
    if i.modifiers.alt { parts.push("Alt"); }
    parts.push(name);
    Some(parts.join("+"))
}

fn save_recent(recent: &[PathBuf]) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let values: Vec<String> = recent.iter().map(|path| path.display().to_string()).collect();
    if let Ok(text) = serde_json::to_string_pretty(&values) {
        let _ = std::fs::write(dir.join("recent.json"), text);
    }
}

fn tr<'a>(zh: bool, zh_text: &'a str, en: &'a str) -> &'a str {
    if zh { zh_text } else { en }
}

fn remember(recent: &mut Vec<PathBuf>, path: &Path) {
    recent.retain(|item| item != path);
    recent.insert(0, path.to_path_buf());
    recent.truncate(10);
    save_recent(recent);
}

include!("interact.rs");

#[cfg(test)]
mod font_tests {
    use super::face_with_char;

    #[test]
    fn yahei_collection_keeps_chinese() {
        let path = r"C:\Windows\Fonts\msyh.ttc";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        assert_eq!(face_with_char(&bytes, '中'), Some(0));
        // Cutting the collection at the next face header drops the glyph tables.
        let next = u32::from_be_bytes(bytes[16..20].try_into().unwrap()) as usize;
        let first = u32::from_be_bytes(bytes[12..16].try_into().unwrap()) as usize;
        if next > first && next < bytes.len() {
            assert!(face_with_char(&bytes[first..next], '中').is_none());
        }
    }
}
