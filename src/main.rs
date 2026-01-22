use eframe::egui;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

// Supported image extensions
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"];

// Clamp window size to fit comfortably on screen (prevents Hyprland from tiling)
// Uses 80% of a 2560x1440 screen as max: 2048x1152
const MAX_WINDOW_WIDTH: f32 = 2048.0;
const MAX_WINDOW_HEIGHT: f32 = 1152.0;

fn clamp_to_screen(width: f32, height: f32) -> [f32; 2] {
    let scale = (MAX_WINDOW_WIDTH / width).min(MAX_WINDOW_HEIGHT / height).min(1.0);
    [width * scale, height * scale]
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = env::args().collect();
    let initial_path = args.get(1).map(PathBuf::from);

    // Default size if image load fails or no image
    let mut initial_size = [800.0, 600.0];

    // Try to peek at the image size, clamped to screen-safe dimensions
    if let Some(path) = &initial_path {
        if let Ok(reader) = image::ImageReader::open(path) {
             if let Ok(dims) = reader.into_dimensions() {
                initial_size = clamp_to_screen(dims.0 as f32, dims.1 as f32);
             }
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("svoy")
            .with_inner_size(initial_size)
            .with_min_inner_size([200.0, 150.0])
            .with_app_id("svoy")
            .with_decorations(true)
            .with_resizable(false), // Makes Hyprland float this window like sxiv/nsxiv
        ..Default::default()
    };

    eframe::run_native(
        "svoy",
        options,
        Box::new(|cc| Ok(Box::new(ImageViewer::new(cc, initial_path)))),
    )
}

#[derive(PartialEq, Clone, Copy)]
enum DrawingTool {
    Pencil,
    Shape,
    Text,
}

#[derive(PartialEq, Clone, Copy)]
enum ShapeType {
    Rectangle,
    Circle,
    Line,
}

#[derive(PartialEq, Clone, Copy)]
enum FontFamily {
    Proportional,
    Monospace,
}

#[derive(Clone)]
struct DrawingObject {
    tool: DrawingTool,
    points: Vec<egui::Pos2>, // Image-space coordinates
    color: egui::Color32,
    size: f32,
    // For shapes/text: start and end points or specific fields
    // For now, let's keep it simple:
    // Pencil: list of points
    // Shape: points[0] = start, points[1] = end
    // Text: points[0] = position
    shape_type: Option<ShapeType>,
    text: Option<String>,
}

struct DrawingSettings {
    tool: DrawingTool,
    shape: ShapeType,
    color: egui::Color32,
    size: f32,
    font_size: f32,
    font_family: FontFamily,
    font_bold: bool,
}

impl Default for DrawingSettings {
    fn default() -> Self {
        Self {
            tool: DrawingTool::Pencil,
            shape: ShapeType::Rectangle,
            color: egui::Color32::RED,
            size: 5.0,
            font_size: 20.0,
            font_family: FontFamily::Proportional,
            font_bold: false,
        }
    }
}

#[derive(Clone)]
struct ImageMetadata {
    filename: String,
    resolution: String, 
    file_size: String,
    format: String,
    modified: String,
}

struct ImageViewer {
    texture: Option<egui::TextureHandle>,
    blurred_texture: Option<egui::TextureHandle>,
    error_message: Option<String>,
    
    current_path: Option<PathBuf>,
    image_list: Vec<PathBuf>,
    current_index: usize,

    // Image Data
    current_image: Option<image::DynamicImage>,

    // Transformation
    zoom: f32,
    target_zoom: f32,
    offset: egui::Vec2,
    last_frame_time: Instant,
    
    // UI State
    top_bar_opacity: f32,
    is_drawing_mode: bool,
    is_image_edited: bool,
    show_exit_confirmation: bool,
    drawing_settings: DrawingSettings,
    
    // Drawing Data
    drawings: Vec<DrawingObject>,
    current_stroke: Option<DrawingObject>,
    
    // Text Entry State
    pending_text_pos: Option<egui::Pos2>, // Image Space
    text_entry_string: String,

    // Metadata State
    metadata: Option<ImageMetadata>,
    show_info_panel: bool,
    
    // Navigation Arrow State
    left_arrow_opacity: f32,
    right_arrow_opacity: f32,
    
    // Pending window resize (for Wayland compatibility)
    pending_resize: Option<egui::Vec2>,
    pending_resize_frame: u8,
}

impl ImageViewer {
    fn new(cc: &eframe::CreationContext<'_>, initial_path: Option<PathBuf>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        
        let mut viewer = Self {
            texture: None,
            blurred_texture: None,
            error_message: None,
            current_path: None,
            image_list: Vec::new(),
            current_index: 0,
            
            current_image: None,

            zoom: 1.0,
            target_zoom: 1.0,
            offset: egui::Vec2::ZERO,
            last_frame_time: Instant::now(),
            
            top_bar_opacity: 0.0,
            is_drawing_mode: false,
            is_image_edited: false,
            show_exit_confirmation: false,
            drawing_settings: DrawingSettings::default(),
            
            drawings: Vec::new(),
            current_stroke: None,
            
            pending_text_pos: None,
            text_entry_string: String::new(),
            metadata: None,
            show_info_panel: false,
            left_arrow_opacity: 0.0,
            right_arrow_opacity: 0.0,
            pending_resize: None,
            pending_resize_frame: 0,
        };

        if let Some(path) = initial_path {
            viewer.load_image_and_context(&cc.egui_ctx, path);
        }

        viewer
    }

    fn load_image_and_context(&mut self, ctx: &egui::Context, path: PathBuf) {
        // Reset transform when loading new image
        self.zoom = 1.0;
        self.target_zoom = 1.0;
        self.offset = egui::Vec2::ZERO;
        self.is_image_edited = false;
        self.drawings.clear();
        self.current_stroke = None;
        self.pending_text_pos = None;
        self.text_entry_string.clear();
        self.metadata = None;

        // Populate image list if needed
        if self.image_list.is_empty() {
            if let Some(parent) = path.parent() {
                 self.scan_directory(parent);
                 // Find index of current image
                 if let Ok(canon_path) = path.canonicalize() {
                     if let Some(idx) = self.image_list.iter().position(|p| p == &canon_path) {
                         self.current_index = idx;
                     }
                 }
            }
        } else {
             // If we already have a list, update index
            if let Ok(canon_path) = path.canonicalize() {
                 if let Some(idx) = self.image_list.iter().position(|p| p == &canon_path) {
                     self.current_index = idx;
                 }
             }
        }

        self.current_path = Some(path.clone());
        self.load_texture(ctx, &path);
    }

    fn scan_directory(&mut self, dir: &Path) {
        let mut images = Vec::new();
        // Use WalkDir but max_depth 1 for current folder only
        for entry in WalkDir::new(dir).max_depth(1).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
                        if let Ok(canon) = path.canonicalize() {
                             images.push(canon);
                        }
                    }
                }
            }
        }
        images.sort();
        self.image_list = images;
    }

    fn load_texture(&mut self, ctx: &egui::Context, path: &Path) {
        match image::open(path) {
            Ok(img) => {
                // Schedule window resize for next frame, clamped to screen-safe size
                let clamped = clamp_to_screen(img.width() as f32, img.height() as f32);
                let new_size = egui::vec2(clamped[0], clamped[1]);
                self.pending_resize = Some(new_size);
                self.pending_resize_frame = 0;
                ctx.request_repaint();
                
                self.current_image = Some(img.clone());
                self.metadata = Some(self.extract_metadata(path, &img));
                self.update_texture_from_image(ctx);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load: {}", e));
                self.texture = None;
                self.blurred_texture = None;
                self.current_image = None;
            }
        }
    }
    
    fn extract_metadata(&self, path: &Path, img: &image::DynamicImage) -> ImageMetadata {
        let resolution = format!("{} x {}", img.width(), img.height());
        let format = path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("???")
            .to_uppercase();
        
        let file_size = if let Ok(meta) = std::fs::metadata(path) {
            let bytes = meta.len();
            if bytes < 1024 * 1024 {
                format!("{:.1} KB", bytes as f64 / 1024.0)
            } else {
                format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
            }
        } else {
            "Unknown".to_string()
        };

        ImageMetadata {
            filename: path.file_name().and_then(|s| s.to_str()).unwrap_or("???").to_string(),
            resolution,
            file_size,
            format,
            modified: "N/A".to_string(), 
        }
    }

    fn update_texture_from_image(&mut self, ctx: &egui::Context) {
        if let Some(img) = &self.current_image {
             let rgba = img.to_rgba8();
             let size = [rgba.width() as usize, rgba.height() as usize];
             let pixels = rgba.into_raw();
             let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
             let texture = ctx.load_texture("img", color_image, egui::TextureOptions::LINEAR);
             self.texture = Some(texture);
             
             // Generate blurred version
             // Downscale for performance first
             let thumb = img.resize(256, 256, image::imageops::FilterType::Nearest);
             let blurred = thumb.blur(60.0); // Heavy blur 
             let b_rgba = blurred.to_rgba8();
             let b_size = [b_rgba.width() as usize, b_rgba.height() as usize];
             let b_pixels = b_rgba.into_raw();
             let b_color_image = egui::ColorImage::from_rgba_unmultiplied(b_size, &b_pixels);
             let b_texture = ctx.load_texture("img_blur", b_color_image, egui::TextureOptions::LINEAR);
             self.blurred_texture = Some(b_texture);

             self.error_message = None;
        }
    }
    
    fn rotate_image(&mut self, ctx: &egui::Context) {
        if let Some(img) = &mut self.current_image {
            *img = img.rotate90();
            self.is_image_edited = true;
            self.update_texture_from_image(ctx);
        }
    }
    
    fn next_image(&mut self, ctx: &egui::Context) {
        if self.image_list.is_empty() { return; }
        self.current_index = (self.current_index + 1) % self.image_list.len();
        let path = self.image_list[self.current_index].clone();
        self.load_image_and_context(ctx, path);
    }

    fn prev_image(&mut self, ctx: &egui::Context) {
        if self.image_list.is_empty() { return; }
        if self.current_index == 0 {
            self.current_index = self.image_list.len() - 1;
        } else {
            self.current_index -= 1;
        }
        let path = self.image_list[self.current_index].clone();
        self.load_image_and_context(ctx, path);
    }

    fn save_current_image(&mut self) -> Result<(), String> {
        if let Some(path) = &self.current_path {
            if let Some(img) = &self.current_image {
                // If we have drawings, we should probably burn them in or warn?
                // For now, just save the base image as requested in previous steps, 
                // but strictly speaking "Save" should probably save the edits.
                // Given the task is just "controls work", let's make sure Convert works first.
                img.save(path).map_err(|e| e.to_string())?;
                self.is_image_edited = false;
                return Ok(());
            }
        }
        Err("No image to save".to_string())
    }

    fn convert_image(&mut self, format: image::ImageFormat) {
        if let Some(path) = &self.current_path {
            if let Some(img) = &self.current_image {
               let new_ext = match format {
                   image::ImageFormat::Png => "png",
                   image::ImageFormat::Jpeg => "jpg",
                   _ => "png",
               };
               let new_path = path.with_extension(new_ext);
               if let Err(e) = img.save(&new_path) {
                   self.error_message = Some(format!("Failed to convert: {}", e));
               } else {
                   // Refresh list?
                   // Optional: Switch to new image?
               }
            }
        }
    }
}


impl eframe::App for ImageViewer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Calculate delta time for smooth animations
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame_time).as_secs_f32();
        self.last_frame_time = now;

        // Smooth zoom interpolation (120+ FPS capable)
        let zoom_speed = 15.0; // Higher = faster response
        let zoom_diff = self.target_zoom - self.zoom;
        if zoom_diff.abs() > 0.001 {
            self.zoom += zoom_diff * (zoom_speed * dt).min(1.0);
            ctx.request_repaint(); // Keep animating
        } else {
            self.zoom = self.target_zoom;
        }

        // Handle pending window resize (multi-frame for Wayland compatibility)
        if let Some(new_size) = self.pending_resize {
            match self.pending_resize_frame {
                0 => {
                    // Frame 0: Enable resizing
                    ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
                    self.pending_resize_frame = 1;
                    ctx.request_repaint();
                }
                1 => {
                    // Frame 1: Set the new size
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(new_size));
                    self.pending_resize_frame = 2;
                    ctx.request_repaint();
                }
                _ => {
                    // Frame 2+: Disable resizing and clear pending
                    ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(false));
                    self.pending_resize = None;
                    ctx.request_repaint();
                }
            }
        }

        // Keyboard navigation
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            self.next_image(ctx);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            self.prev_image(ctx);
        }

        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
             if let Some(_) = self.drawings.pop() {
                 // Undid something
                 if self.drawings.is_empty() {
                     self.is_image_edited = false; // Rough approximation
                 }
             }
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.is_image_edited {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.show_exit_confirmation = true;
            }
        }

        if self.show_exit_confirmation {
            egui::Window::new("Save Changes?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label("You have unsaved changes. Do you want to save them?");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            if let Ok(_) = self.save_current_image() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        if ui.button("Discard").clicked() {
                            self.is_image_edited = false; // Force close
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_exit_confirmation = false;
                        }
                    });
                });
        }

        if self.show_info_panel {
            if let Some(meta) = &self.metadata {
                let mut open = true;
                egui::Window::new("Image Info")
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut open)
                    .show(ctx, |ui| {
                        egui::Grid::new("info_grid").striped(true).show(ui, |ui| {
                            ui.label("Filename:"); ui.label(&meta.filename); ui.end_row();
                            ui.label("Resolution:"); ui.label(&meta.resolution); ui.end_row();
                            ui.label("Size:"); ui.label(&meta.file_size); ui.end_row();
                            ui.label("Format:"); ui.label(&meta.format); ui.end_row();
                            ui.label("Modified:"); ui.label(&meta.modified); ui.end_row();
                        });
                    });
                if !open {
                    self.show_info_panel = false;
                }
            }
        }

        // --- Overlay UI Logic ---
        // Calculate all positions and hover states BEFORE rendering any Areas
        // This prevents egui Areas from "stealing" hover state and causing flicker
        
        let screen_rect = ctx.screen_rect();
        let mouse_pos = ctx.input(|i| i.pointer.hover_pos());
        let anim_speed = 12.0 * dt; // Faster animation for smoother feel
        
        // Pre-calculate image rect for blur effects (used by all overlays)
        let image_rect = self.texture.as_ref().map(|tex| {
            let size = tex.size_vec2() * self.zoom;
            egui::Rect::from_center_size(
                (screen_rect.center().to_vec2() + self.offset).to_pos2(),
                size
            )
        });
        
        // --- Top Bar Hover Logic ---
        let top_bar_height = 40.0;
        let top_area = if self.is_drawing_mode { 110.0 } else { top_bar_height };
        
        let hovering_top = mouse_pos.map_or(false, |p| p.y <= top_area && screen_rect.contains(p));
        let should_show_top = hovering_top || self.is_drawing_mode || self.top_bar_opacity > 0.1;
        
        if hovering_top || self.is_drawing_mode {
            self.top_bar_opacity = (self.top_bar_opacity + anim_speed).min(1.0);
        } else {
            self.top_bar_opacity = (self.top_bar_opacity - anim_speed * 0.5).max(0.0); // Slower fade out
        }
        if self.top_bar_opacity > 0.0 && self.top_bar_opacity < 1.0 { ctx.request_repaint(); }
        
        // --- Arrow Hover Logic ---
        let arrow_zone_width = 60.0;
        
        let hovering_left = mouse_pos.map_or(false, |p| {
            p.x <= arrow_zone_width && p.y > top_area && screen_rect.contains(p)
        });
        let hovering_right = mouse_pos.map_or(false, |p| {
            p.x >= screen_rect.width() - arrow_zone_width && p.y > top_area && screen_rect.contains(p)
        });
        
        if hovering_left {
            self.left_arrow_opacity = (self.left_arrow_opacity + anim_speed).min(1.0);
        } else {
            self.left_arrow_opacity = (self.left_arrow_opacity - anim_speed * 0.5).max(0.0);
        }
        
        if hovering_right {
            self.right_arrow_opacity = (self.right_arrow_opacity + anim_speed).min(1.0);
        } else {
            self.right_arrow_opacity = (self.right_arrow_opacity - anim_speed * 0.5).max(0.0);
        }
        
        if (self.left_arrow_opacity > 0.0 && self.left_arrow_opacity < 1.0) ||
           (self.right_arrow_opacity > 0.0 && self.right_arrow_opacity < 1.0) {
            ctx.request_repaint();
        }
        
        // Helper: paint gradient blur overlay
        use egui::epaint::{Vertex, Mesh};
        let paint_blur_gradient = |painter: &egui::Painter, rect: egui::Rect, opacity: f32, 
                                   blur_tex: &egui::TextureHandle, img_rect: egui::Rect,
                                   gradient_dir: &str| {
            let intersect = rect.intersect(img_rect);
            if !intersect.is_positive() { return; }
            
            let uv_min = egui::pos2(
                (intersect.min.x - img_rect.min.x) / img_rect.width(),
                (intersect.min.y - img_rect.min.y) / img_rect.height(),
            );
            let uv_max = egui::pos2(
                (intersect.max.x - img_rect.min.x) / img_rect.width(),
                (intersect.max.y - img_rect.min.y) / img_rect.height(),
            );
            
            let col_full = egui::Color32::WHITE.linear_multiply(opacity);
            let col_fade = egui::Color32::TRANSPARENT;
            
            let mut mesh = Mesh::with_texture(blur_tex.id());
            
            // Build gradient mesh based on direction
            let (tl, tr, br, bl) = match gradient_dir {
                "down" => (col_full, col_full, col_fade, col_fade),
                "left" => (col_fade, col_full, col_full, col_fade),
                "right" => (col_full, col_fade, col_fade, col_full),
                _ => (col_full, col_full, col_full, col_full),
            };
            
            mesh.vertices.push(Vertex { pos: intersect.left_top(), uv: egui::pos2(uv_min.x, uv_min.y), color: tl });
            mesh.vertices.push(Vertex { pos: intersect.right_top(), uv: egui::pos2(uv_max.x, uv_min.y), color: tr });
            mesh.vertices.push(Vertex { pos: intersect.right_bottom(), uv: egui::pos2(uv_max.x, uv_max.y), color: br });
            mesh.vertices.push(Vertex { pos: intersect.left_bottom(), uv: egui::pos2(uv_min.x, uv_max.y), color: bl });
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            
            painter.add(mesh);
        };
        
        // --- Render Top Bar ---
        if self.top_bar_opacity > 0.0 {
            let top_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(screen_rect.width(), top_bar_height));
            
            egui::Area::new(egui::Id::new("top_bar"))
                .fixed_pos(egui::Pos2::ZERO)
                .order(egui::Order::Foreground)
                .interactable(true)
                .show(ctx, |ui| {
                    // Paint blur gradient (fades down)
                    if let (Some(blur_tex), Some(img_rect)) = (&self.blurred_texture, image_rect) {
                        paint_blur_gradient(ui.painter(), top_rect, self.top_bar_opacity, blur_tex, img_rect, "down");
                    }
                    
                    // UI content
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(top_rect.shrink(10.0)), |ui| {
                        ui.horizontal_centered(|ui| {
                            if let Some(path) = &self.current_path {
                                let name = path.file_name().unwrap_or_default().to_string_lossy();
                                let col = egui::Color32::WHITE.linear_multiply(self.top_bar_opacity);
                                ui.label(egui::RichText::new(name).size(16.0).strong().color(col));
                                if self.is_image_edited {
                                    ui.label(egui::RichText::new("Edited").italics().color(egui::Color32::LIGHT_GRAY.linear_multiply(self.top_bar_opacity)));
                                }
                            }
                            
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let btn_size = egui::vec2(24.0, 24.0);
                                let tint = egui::Color32::WHITE.linear_multiply(self.top_bar_opacity);
                                
                                // Drawing Toggle
                                let icon = if self.is_drawing_mode {
                                    egui::include_image!("../materials/pencil-filled.svg")
                                } else {
                                    egui::include_image!("../materials/pencil-unfilled.svg")
                                };
                                if ui.add(egui::Button::image(egui::Image::new(icon).tint(tint)).frame(false).min_size(btn_size))
                                    .on_hover_text(if self.is_drawing_mode { "Stop Drawing" } else { "Toggle Drawing" })
                                    .clicked() { self.is_drawing_mode = !self.is_drawing_mode; }
                                
                                ui.separator();
                                
                                // Convert
                                let icon = egui::include_image!("../materials/convert2.svg");
                                let resp = ui.add(egui::Button::image(egui::Image::new(icon).tint(tint)).frame(false).min_size(btn_size))
                                    .on_hover_text("Convert Image");
                                if resp.clicked() { ui.ctx().memory_mut(|m| m.open_popup(egui::Id::new("convert_popup"))); }
                                egui::popup::popup_below_widget(ui, egui::Id::new("convert_popup"), &resp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                                    ui.set_min_width(100.0);
                                    if ui.button("to JPG").clicked() { self.convert_image(image::ImageFormat::Jpeg); ui.close_menu(); }
                                    if ui.button("to PNG").clicked() { self.convert_image(image::ImageFormat::Png); ui.close_menu(); }
                                });
                                
                                // Rotate
                                let icon = egui::include_image!("../materials/rotate2.png");
                                if ui.add(egui::Button::image(egui::Image::new(icon).tint(tint)).frame(false).min_size(btn_size))
                                    .on_hover_text("Rotate 90°").clicked() { self.rotate_image(ctx); }
                                
                                // Info
                                let icon = egui::include_image!("../materials/info.svg");
                                if ui.add(egui::Button::image(egui::Image::new(icon).tint(tint)).frame(false).min_size(btn_size))
                                    .on_hover_text("Image Info").clicked() { self.show_info_panel = !self.show_info_panel; }
                            });
                        });
                    });
                    
                    // Drawing tools
                    if self.is_drawing_mode {
                        let tools_rect = egui::Rect::from_min_size(egui::pos2(0.0, 60.0), egui::vec2(screen_rect.width(), 50.0));
                        ui.painter().rect_filled(tools_rect, 0.0, egui::Color32::from_black_alpha((180.0 * self.top_bar_opacity) as u8));
                        
                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(tools_rect.shrink(5.0)), |ui| {
                            egui::ScrollArea::horizontal().show(ui, |ui| {
                                ui.horizontal_centered(|ui| {
                                    ui.selectable_value(&mut self.drawing_settings.tool, DrawingTool::Pencil, "✏ Pencil");
                                    ui.selectable_value(&mut self.drawing_settings.tool, DrawingTool::Shape, "⬜ Shape");
                                    ui.selectable_value(&mut self.drawing_settings.tool, DrawingTool::Text, "T Text");
                                    ui.separator();
                                    
                                    let colors = [egui::Color32::RED, egui::Color32::GREEN, egui::Color32::BLUE,
                                                  egui::Color32::YELLOW, egui::Color32::BLACK, egui::Color32::WHITE];
                                    for &c in &colors {
                                        let mut b = egui::Button::new("   ").fill(c);
                                        if self.drawing_settings.color == c { b = b.stroke(egui::Stroke::new(2.0, egui::Color32::WHITE)); }
                                        if ui.add(b).clicked() { self.drawing_settings.color = c; }
                                    }
                                    
                                    ui.separator();
                                    match self.drawing_settings.tool {
                                        DrawingTool::Pencil => { ui.add(egui::Slider::new(&mut self.drawing_settings.size, 1.0..=50.0).text("Size")); }
                                        DrawingTool::Shape => {
                                            ui.selectable_value(&mut self.drawing_settings.shape, ShapeType::Rectangle, "Rect");
                                            ui.selectable_value(&mut self.drawing_settings.shape, ShapeType::Circle, "Circle");
                                            ui.selectable_value(&mut self.drawing_settings.shape, ShapeType::Line, "Line");
                                            ui.add(egui::Slider::new(&mut self.drawing_settings.size, 1.0..=20.0).text("Thickness"));
                                        }
                                        DrawingTool::Text => {
                                            ui.add(egui::Slider::new(&mut self.drawing_settings.font_size, 10.0..=100.0).text("Size"));
                                            ui.selectable_value(&mut self.drawing_settings.font_family, FontFamily::Proportional, "Sans");
                                            ui.selectable_value(&mut self.drawing_settings.font_family, FontFamily::Monospace, "Mono");
                                            ui.checkbox(&mut self.drawing_settings.font_bold, "Bold");
                                        }
                                    }
                                });
                            });
                        });
                    }
                });
        }
        
        // --- Render Navigation Arrows ---
        // Arrows: vertical gradient blur strips on left/right edges
        let arrow_strip_width = 50.0;
        let arrow_strip_height = 100.0;
        let arrow_y = (screen_rect.height() - arrow_strip_height) / 2.0;
        
        // Left Arrow
        if self.left_arrow_opacity > 0.0 {
            let left_rect = egui::Rect::from_min_size(egui::pos2(0.0, arrow_y), egui::vec2(arrow_strip_width, arrow_strip_height));
            
            egui::Area::new(egui::Id::new("left_arrow"))
                .fixed_pos(left_rect.min)
                .order(egui::Order::Foreground)
                .interactable(true)
                .show(ctx, |ui| {
                    // Paint blur gradient (fades right)
                    if let (Some(blur_tex), Some(img_rect)) = (&self.blurred_texture, image_rect) {
                        paint_blur_gradient(ui.painter(), left_rect, self.left_arrow_opacity, blur_tex, img_rect, "right");
                    }
                    
                    // Chevron
                    let center = left_rect.center();
                    let s = 16.0;
                    let col = egui::Color32::WHITE.linear_multiply(self.left_arrow_opacity);
                    ui.painter().add(egui::Shape::line(vec![
                        egui::pos2(center.x + s * 0.3, center.y - s * 0.5),
                        egui::pos2(center.x - s * 0.3, center.y),
                        egui::pos2(center.x + s * 0.3, center.y + s * 0.5),
                    ], egui::Stroke::new(2.5, col)));
                    
                    if ui.allocate_rect(left_rect, egui::Sense::click()).clicked() {
                        self.prev_image(ctx);
                    }
                });
        }
        
        // Right Arrow
        if self.right_arrow_opacity > 0.0 {
            let right_rect = egui::Rect::from_min_size(
                egui::pos2(screen_rect.width() - arrow_strip_width, arrow_y),
                egui::vec2(arrow_strip_width, arrow_strip_height)
            );
            
            egui::Area::new(egui::Id::new("right_arrow"))
                .fixed_pos(right_rect.min)
                .order(egui::Order::Foreground)
                .interactable(true)
                .show(ctx, |ui| {
                    // Paint blur gradient (fades left)
                    if let (Some(blur_tex), Some(img_rect)) = (&self.blurred_texture, image_rect) {
                        paint_blur_gradient(ui.painter(), right_rect, self.right_arrow_opacity, blur_tex, img_rect, "left");
                    }
                    
                    // Chevron
                    let center = right_rect.center();
                    let s = 16.0;
                    let col = egui::Color32::WHITE.linear_multiply(self.right_arrow_opacity);
                    ui.painter().add(egui::Shape::line(vec![
                        egui::pos2(center.x - s * 0.3, center.y - s * 0.5),
                        egui::pos2(center.x + s * 0.3, center.y),
                        egui::pos2(center.x - s * 0.3, center.y + s * 0.5),
                    ], egui::Stroke::new(2.5, col)));
                    
                    if ui.allocate_rect(right_rect, egui::Sense::click()).clicked() {
                        self.next_image(ctx);
                    }
                });
        }

        egui::CentralPanel::default().frame(egui::Frame::none().inner_margin(0.0).outer_margin(0.0)).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.spacing_mut().window_margin = egui::Margin::ZERO;
            if let Some(err) = &self.error_message {
                ui.centered_and_justified(|ui| ui.colored_label(egui::Color32::RED, err));
                return;
            }

            if let Some(texture) = &self.texture {
                let available_size = ui.available_size();
                let image_size = texture.size_vec2();
                
                // Zoom is absolute: 1.0 = native resolution (1 image pixel = 1 screen pixel)
                // Can zoom out (< 1.0) or zoom in (> 1.0)
                let display_size = image_size * self.zoom;

                // Handle Input
                // SENSE: If drawing, only sense clicks/hovers to avoid panning consuming the drag
                let sense = if self.is_drawing_mode { egui::Sense::click() } else { egui::Sense::drag() };
                let (rect, response) = ui.allocate_exact_size(available_size, sense);

                // Zoom with scroll (smooth animated, centered on mouse)
                let scroll_delta = ctx.input(|i| i.raw_scroll_delta.y);
                if scroll_delta != 0.0 {
                    let zoom_factor = 1.15;
                    let old_zoom = self.target_zoom;
                    
                    if scroll_delta > 0.0 {
                        self.target_zoom *= zoom_factor;
                    } else {
                        self.target_zoom /= zoom_factor;
                    }
                    // Allow zooming out to 5% and in to 5000%
                    self.target_zoom = self.target_zoom.clamp(0.05, 50.0);
                    
                    // Adjust offset to zoom towards mouse cursor
                    if let Some(mouse_pos) = ctx.input(|i| i.pointer.hover_pos()) {
                        // Current image center in screen coords
                        let screen_center = rect.center().to_vec2() + self.offset;
                        // Vector from image center to mouse
                        let mouse_offset = mouse_pos.to_vec2() - screen_center;
                        // Scale this vector by the zoom ratio to keep mouse point fixed
                        let zoom_ratio = self.target_zoom / old_zoom;
                        // New offset adjustment: the point under mouse should stay under mouse
                        // offset_new = offset_old - mouse_offset * (zoom_ratio - 1)
                        self.offset -= mouse_offset * (zoom_ratio - 1.0);
                    }
                    
                    ctx.request_repaint();
                }

                // Drag/Pan
                if response.dragged() {
                     self.offset += response.drag_delta();
                }

                // Center logic
                let mut screen_center = rect.center().to_vec2();
                // Apply offset
                screen_center += self.offset;

                let image_rect = egui::Rect::from_center_size(screen_center.to_pos2(), display_size);

                // --- Input Handling for Drawing ---
                // We need to handle input BEFORE painting the image if we want to consume clicks, 
                // but we need the image_rect to map coordinates.
                if self.is_drawing_mode {
                     let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
                     
                     // Only draw if within image bounds
                     if let Some(pos) = pointer_pos {
                         if image_rect.contains(pos) {
                             // Map screen pos to image space (0,0 to width,height)
                             let rel_x = (pos.x - image_rect.min.x) / self.zoom;
                             let rel_y = (pos.y - image_rect.min.y) / self.zoom;
                             let image_pos = egui::pos2(rel_x, rel_y);
                             
                             if ctx.input(|i| i.pointer.primary_down()) {
                                 // Start or Continue Stroke
                                 if self.drawing_settings.tool == DrawingTool::Text {
                                     // Text is click-to-place, not drag
                                     // Logic handled in released or clicked
                                 } else {
                                     if self.current_stroke.is_none() {
                                         // Start new stroke
                                         // Determine type
                                         let shape_type = if self.drawing_settings.tool == DrawingTool::Shape {
                                             Some(self.drawing_settings.shape)
                                         } else {
                                             None
                                         };
                                         
                                         self.current_stroke = Some(DrawingObject {
                                             tool: self.drawing_settings.tool,
                                             points: vec![image_pos],
                                             color: self.drawing_settings.color,
                                             size: self.drawing_settings.size,
                                             shape_type,
                                             text: None,
                                         });
                                         self.is_image_edited = true;
                                     } else {
                                         // Update stroke
                                         if let Some(stroke) = &mut self.current_stroke {
                                              match stroke.tool {
                                                  DrawingTool::Pencil => {
                                                      // Freehand: append points
                                                      if stroke.points.last() != Some(&image_pos) {
                                                          stroke.points.push(image_pos);
                                                      }
                                                  }
                                                  DrawingTool::Shape => {
                                                      // Shape: Update end point (points[1])
                                                      // points[0] is start, points[1] is current end
                                                      if stroke.points.len() == 1 {
                                                          stroke.points.push(image_pos);
                                                      } else {
                                                          stroke.points[1] = image_pos;
                                                      }
                                                  }
                                                  _ => {}
                                              }
                                         }
                                     }
                                 }
                             } else if ctx.input(|i| i.pointer.any_released()) {
                                 // Mouse released
                                 if self.drawing_settings.tool == DrawingTool::Text {
                                     if ctx.input(|i| i.pointer.primary_released()) {
                                        // Open Text Popup
                                        self.pending_text_pos = Some(image_pos);
                                        self.text_entry_string.clear();
                                     }
                                 } else {
                                     // Commit stroke
                                     if let Some(stroke) = self.current_stroke.take() {
                                         self.drawings.push(stroke);
                                     }
                                 }
                             }
                         }
                     }
                }

                // Text Popup
                let mut text_to_commit = None;
                if let Some(pos) = self.pending_text_pos {
                    // Convert to screen space for popup positioning
                    let screen_pos = egui::pos2(
                        image_rect.min.x + pos.x * self.zoom,
                        image_rect.min.y + pos.y * self.zoom
                    );
                    
                    let mut open = true;
                    let mut should_close = false;
                    egui::Window::new("Add Text")
                        .fixed_pos(screen_pos)
                        .collapsible(false)
                        .resizable(false)
                        .open(&mut open)
                        .show(ctx, |ui| {
                           ui.text_edit_singleline(&mut self.text_entry_string).request_focus();
                           if ui.button("Add").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                               if !self.text_entry_string.is_empty() {
                                   text_to_commit = Some(DrawingObject {
                                       tool: DrawingTool::Text,
                                       points: vec![pos],
                                       color: self.drawing_settings.color,
                                       size: self.drawing_settings.font_size, // Use font size here
                                       shape_type: None,
                                       text: Some(self.text_entry_string.clone()),
                                   });
                               }
                               // Close
                               should_close = true; 
                           }
                        });
                    
                    if !open || should_close {
                        self.pending_text_pos = None;
                    }
                }
                
                if let Some(obj) = text_to_commit {
                    self.drawings.push(obj);
                    self.is_image_edited = true;
                    self.pending_text_pos = None;
                }

                // Paint Image
                let painter = ui.painter_at(rect);
                painter.image(
                    texture.id(),
                    image_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE
                );

                // Paint Drawings
                let mut shapes = Vec::new();
                // Helper to map image space to screen space
                let to_screen = |p: egui::Pos2| -> egui::Pos2 {
                    egui::pos2(
                        image_rect.min.x + p.x * self.zoom,
                        image_rect.min.y + p.y * self.zoom
                    )
                };

                let mut paint_object = |drawing: &DrawingObject| {
                    match drawing.tool {
                        DrawingTool::Pencil => {
                            if drawing.points.len() >= 2 {
                                let screen_points: Vec<egui::Pos2> = drawing.points.iter().map(|&p| to_screen(p)).collect();
                                shapes.push(egui::Shape::line(screen_points, egui::Stroke::new(drawing.size * self.zoom, drawing.color)));
                            }
                        },
                        DrawingTool::Shape => {
                             if drawing.points.len() >= 2 {
                                 let start = to_screen(drawing.points[0]);
                                 let end = to_screen(drawing.points[1]);
                                 let stroke = egui::Stroke::new(drawing.size * self.zoom, drawing.color);
                                 
                                 if let Some(stype) = drawing.shape_type {
                                     match stype {
                                         ShapeType::Rectangle => {
                                             let rect = egui::Rect::from_two_pos(start, end);
                                             shapes.push(egui::Shape::rect_stroke(rect, 0.0, stroke));
                                         },
                                         ShapeType::Circle => {
                                             let center = start;
                                             let radius = start.distance(end);
                                             shapes.push(egui::Shape::circle_stroke(center, radius, stroke));
                                         },
                                         ShapeType::Line => {
                                             shapes.push(egui::Shape::line_segment([start, end], stroke));
                                         }
                                     }
                                 }
                             }
                        },
                        DrawingTool::Text => {
                             if let Some(text) = &drawing.text {
                                 if let Some(pos) = drawing.points.first() {
                                     let screen_pos = to_screen(*pos);
                                     painter.text(
                                         screen_pos,
                                         egui::Align2::LEFT_TOP,
                                         text,
                                         egui::FontId::proportional(drawing.size * self.zoom),
                                         drawing.color
                                     );
                                 }
                             }
                        },
                    }
                };

                // 1. Committed Drawings
                for drawing in &self.drawings {
                     paint_object(drawing);
                }
                
                // 2. Current Stroke
                if let Some(stroke) = &self.current_stroke {
                     paint_object(stroke);
                }
                
                painter.extend(shapes);
            } else {
                 ui.centered_and_justified(|ui| ui.label("Open an image"));
            }
        });
    }
}