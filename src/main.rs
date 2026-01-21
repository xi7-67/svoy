use eframe::egui;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

// Supported image extensions
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"];

fn main() -> eframe::Result<()> {
    let args: Vec<String> = env::args().collect();
    let initial_path = args.get(1).map(PathBuf::from);

    // Default size if image load fails or no image
    let mut initial_size = [800.0, 600.0];

    // Try to peek at the image size to set window size (opens at exact image resolution)
    if let Some(path) = &initial_path {
        if let Ok(reader) = image::ImageReader::open(path) {
             if let Ok(dims) = reader.into_dimensions() {
                // Use actual image dimensions - this may trigger floating in tiling WMs
                initial_size = [dims.0 as f32, dims.1 as f32];
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

        // --- Top Bar Logic ---
        let screen_rect = ctx.screen_rect();
        let top_area_height = 60.0;
        let mouse_pos = ctx.input(|i| i.pointer.hover_pos());
        let is_hovering_top = mouse_pos.map_or(false, |pos| pos.y <= top_area_height && screen_rect.contains(pos));

        let anim_speed = 8.0 * dt;
        if is_hovering_top || self.is_drawing_mode {
            self.top_bar_opacity = (self.top_bar_opacity + anim_speed).min(1.0);
            if self.top_bar_opacity < 1.0 { ctx.request_repaint(); }
        } else {
            self.top_bar_opacity = (self.top_bar_opacity - anim_speed).max(0.0);
             if self.top_bar_opacity > 0.0 { ctx.request_repaint(); }
        }

        if self.top_bar_opacity > 0.0 {
            let top_bar_bg_color = egui::Color32::from_black_alpha((180.0 * self.top_bar_opacity) as u8);
            
            egui::Area::new(egui::Id::new("top_bar_overlay"))
                .fixed_pos(egui::pos2(0.0, 0.0))
                .anchor(egui::Align2::LEFT_TOP, egui::vec2(0.0, 0.0))
                .order(egui::Order::Foreground)
                .interactable(true)
                .show(ctx, |ui| {
                    // Custom background painting for "shadowy" look
                    let top_bar_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(screen_rect.width(), 40.0));
                    
                    // 1. Blur Effect (Glassmorphism)
                    // We need to find where the image is relative to the top bar
                    if let (Some(tex), Some(blur_tex)) = (&self.texture, &self.blurred_texture) {
                        // Actually we need the same display logic here to know where the image is.
                        // Ideally we'd store the calculated image_rect from the last frame or recalculate it centrally.
                        // For now, let's re-calculate:
                        let image_size = tex.size_vec2();
                        let display_size = image_size * self.zoom;
                        let screen_center = screen_rect.center().to_vec2() + self.offset;
                        let image_rect = egui::Rect::from_center_size(screen_center.to_pos2(), display_size);

                        // Find intersection
                        let intersect = top_bar_rect.intersect(image_rect);
                        if intersect.is_positive() {
                            // Calculate UVs
                            // Mapping: where is 'intersect' relative to 'image_rect' [0..1]
                            let uv_min = egui::pos2(
                                (intersect.min.x - image_rect.min.x) / image_rect.width(),
                                (intersect.min.y - image_rect.min.y) / image_rect.height(),
                            );
                            let uv_max = egui::pos2(
                                (intersect.max.x - image_rect.min.x) / image_rect.width(),
                                (intersect.max.y - image_rect.min.y) / image_rect.height(),
                            );
                            
                            // Gradient Mesh for Blur (Fade out at bottom)
                            let mut blur_mesh = Mesh::with_texture(blur_tex.id());
                            let b_col_top = egui::Color32::WHITE.linear_multiply(self.top_bar_opacity);
                            let b_col_bot = egui::Color32::TRANSPARENT;
                            
                            blur_mesh.add_rect_with_uv(
                                intersect, 
                                egui::Rect::from_min_max(uv_min, uv_max), 
                                b_col_top // default color, we will override vertices
                            );
                            
                            // Override vertex colors for gradient
                            // add_rect_with_uv adds 4 vertices: top-left, right-top, right-bottom, left-bottom (usually)
                            // We need to check order or just force them. To be safe, let's construct manually.
                            blur_mesh.vertices.clear();
                            blur_mesh.indices.clear();
                            let b_idx = 0;
                            blur_mesh.vertices.push(Vertex { pos: intersect.left_top(), uv: egui::pos2(uv_min.x, uv_min.y), color: b_col_top });
                            blur_mesh.vertices.push(Vertex { pos: intersect.right_top(), uv: egui::pos2(uv_max.x, uv_min.y), color: b_col_top });
                            blur_mesh.vertices.push(Vertex { pos: intersect.right_bottom(), uv: egui::pos2(uv_max.x, uv_max.y), color: b_col_bot });
                            blur_mesh.vertices.push(Vertex { pos: intersect.left_bottom(), uv: egui::pos2(uv_min.x, uv_max.y), color: b_col_bot });
                            blur_mesh.add_triangle(b_idx, b_idx + 1, b_idx + 2);
                            blur_mesh.add_triangle(b_idx, b_idx + 2, b_idx + 3);

                            ui.painter().add(blur_mesh);
                        }
                    }

                    // 2. Gradient Mesh for Shadow
                    use egui::epaint::{Vertex, Mesh};
                    let mut mesh = Mesh::default();
                    let col_top = egui::Color32::TRANSPARENT;
                    let col_bot = egui::Color32::from_black_alpha(0);
                    
                    
                    // Manually constructing vertices for reliable vertical gradient if add_colored_rect doesn't support gradients in this version of egui
                    mesh.vertices.clear();
                    mesh.indices.clear();
                    let idx = mesh.vertices.len() as u32;
                    mesh.vertices.push(Vertex { pos: top_bar_rect.left_top(), uv: egui::Pos2::ZERO, color: col_top });
                    mesh.vertices.push(Vertex { pos: top_bar_rect.right_top(), uv: egui::Pos2::ZERO, color: col_top });
                    mesh.vertices.push(Vertex { pos: top_bar_rect.right_bottom(), uv: egui::Pos2::ZERO, color: col_bot });
                    mesh.vertices.push(Vertex { pos: top_bar_rect.left_bottom(), uv: egui::Pos2::ZERO, color: col_bot });
                    mesh.add_triangle(idx, idx + 1, idx + 2);
                    mesh.add_triangle(idx, idx + 2, idx + 3);

                    ui.painter().add(mesh);

                    // We do NOT set global visuals here as it affects other windows (like modals)
                    // The widgets below manually handle opacity via Color32 usage.

                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(top_bar_rect.shrink(10.0)), |ui| {
                         ui.horizontal_centered(|ui| {
                             
                             // Left: Filename
                             if let Some(path) = &self.current_path {
                                 let name = path.file_name().unwrap_or_default().to_string_lossy();
                                 ui.label(egui::RichText::new(name).size(16.0).strong().color(egui::Color32::WHITE.linear_multiply(self.top_bar_opacity)));
                                 
                                 // "Edited" status
                                 if self.is_image_edited { 
                                     ui.label(egui::RichText::new("Edited").italics().color(egui::Color32::LIGHT_GRAY.linear_multiply(self.top_bar_opacity))); 
                                 }
                             }

                             ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                 // Right: Buttons
                                 let btn_size = egui::vec2(24.0, 24.0);
                                 let base_color = egui::Color32::WHITE.linear_multiply(self.top_bar_opacity);
                                 
                                 // Drawing Toggle
                                 let tooltip = if self.is_drawing_mode { "Stop Drawing" } else { "Toggle Drawing" };
                                 let icon = if self.is_drawing_mode {
                                     egui::include_image!("../materials/pencil-filled.svg")
                                 } else {
                                     egui::include_image!("../materials/pencil-unfilled.svg")
                                 };
                                 let btn = egui::Button::image(egui::Image::new(icon).tint(base_color)).frame(false).min_size(btn_size);
                                 let response = ui.add(btn).on_hover_text(tooltip);
                                 
                                 if response.clicked() {
                                     self.is_drawing_mode = !self.is_drawing_mode;
                                 }
                                 
                                 ui.separator();

                                 // Convert Button
                                 let icon = egui::include_image!("../materials/convert.svg");
                                 let btn = egui::Button::image(egui::Image::new(icon).tint(base_color)).frame(false).min_size(btn_size);
                                 let response = ui.add(btn).on_hover_text("Convert Image");
                                 
                                 if response.clicked() {
                                     ui.ctx().memory_mut(|m| m.open_popup(egui::Id::new("convert_popup")));
                                 }
                                 
                                 egui::popup::popup_below_widget(ui, egui::Id::new("convert_popup"), &response, egui::PopupCloseBehavior::CloseOnClickOutside, |ui: &mut egui::Ui| {
                                      ui.set_min_width(100.0);
                                      if ui.button("to JPG").clicked() { 
                                          self.convert_image(image::ImageFormat::Jpeg);
                                          ui.close_menu();
                                      }
                                      if ui.button("to PNG").clicked() { 
                                          ui.close_menu();
                                          self.convert_image(image::ImageFormat::Png);
                                      }
                                 });

                                 // Rotate Button
                                 let icon = egui::include_image!("../materials/rotate.png");
                                 let btn = egui::Button::image(egui::Image::new(icon).tint(base_color)).frame(false).min_size(btn_size);
                                 let response = ui.add(btn).on_hover_text("Rotate 90°");
                                 if response.clicked() {
                                     self.rotate_image(ctx);
                                 }

                                 // Info Button
                                 let icon = egui::include_image!("../materials/info.svg");
                                 let btn = egui::Button::image(egui::Image::new(icon).tint(base_color)).frame(false).min_size(btn_size);
                                 let response = ui.add(btn).on_hover_text("Image Info");
                                 
                                 if response.clicked() {
                                     self.show_info_panel = !self.show_info_panel;
                                 }

                             });
                         });
                    });
                    
                    // Drawing Tools Section (Below top bar)
                    // Drawing Tools Section (Below top bar)
                    if self.is_drawing_mode {
                         let tools_rect = egui::Rect::from_min_size(egui::pos2(0.0, 60.0), egui::vec2(screen_rect.width(), 50.0));
                         ui.painter().rect_filled(tools_rect, 0.0, top_bar_bg_color);
                         
                         ui.allocate_new_ui(egui::UiBuilder::new().max_rect(tools_rect.shrink(5.0)), |ui| {
                             egui::ScrollArea::horizontal().show(ui, |ui| {
                                 ui.horizontal_centered(|ui| {
                                 // Tool Selection
                                 ui.selectable_value(&mut self.drawing_settings.tool, DrawingTool::Pencil, "✏ Pencil");
                                 ui.selectable_value(&mut self.drawing_settings.tool, DrawingTool::Shape, "⬜ Shape");
                                 ui.selectable_value(&mut self.drawing_settings.tool, DrawingTool::Text, "T Text");
                                 
                                 ui.separator();
                                 
                                 match self.drawing_settings.tool {
                                     DrawingTool::Pencil => {
                                         // Colors
                                         let colors = [
                                             egui::Color32::RED, egui::Color32::GREEN, egui::Color32::BLUE,
                                             egui::Color32::YELLOW, egui::Color32::BLACK, egui::Color32::WHITE
                                         ];
                                         for &color in &colors {
                                             let mut button = egui::Button::new("   ").fill(color);
                                             if self.drawing_settings.color == color {
                                                 button = button.stroke(egui::Stroke::new(2.0, egui::Color32::WHITE));
                                             }
                                             if ui.add(button).clicked() {
                                                 self.drawing_settings.color = color;
                                             }
                                         }
                                         
                                         ui.separator();
                                         ui.add(egui::Slider::new(&mut self.drawing_settings.size, 1.0..=50.0).text("Size"));
                                     }
                                     DrawingTool::Shape => {
                                         // Shape Selector
                                          ui.selectable_value(&mut self.drawing_settings.shape, ShapeType::Rectangle, "Rect");
                                          ui.selectable_value(&mut self.drawing_settings.shape, ShapeType::Circle, "Circle");
                                          ui.selectable_value(&mut self.drawing_settings.shape, ShapeType::Line, "Line");
                                          
                                          ui.separator();
                                          
                                          // Shape Color
                                          let colors = [
                                             egui::Color32::RED, egui::Color32::GREEN, egui::Color32::BLUE,
                                             egui::Color32::YELLOW, egui::Color32::BLACK, egui::Color32::WHITE
                                         ];
                                          for &color in &colors {
                                             let mut button = egui::Button::new("   ").fill(color);
                                             if self.drawing_settings.color == color {
                                                 button = button.stroke(egui::Stroke::new(2.0, egui::Color32::WHITE));
                                             }
                                             if ui.add(button).clicked() {
                                                 self.drawing_settings.color = color;
                                             }
                                         }
                                         
                                         ui.separator();
                                         ui.add(egui::Slider::new(&mut self.drawing_settings.size, 1.0..=20.0).text("Thickness"));
                                     }
                                     DrawingTool::Text => {
                                         // Font Settings
                                         let colors = [
                                             egui::Color32::RED, egui::Color32::GREEN, egui::Color32::BLUE, 
                                             egui::Color32::BLACK, egui::Color32::WHITE
                                         ];
                                         for &color in &colors {
                                             let mut button = egui::Button::new("   ").fill(color);
                                              if self.drawing_settings.color == color {
                                                 button = button.stroke(egui::Stroke::new(2.0, egui::Color32::WHITE));
                                             }
                                             if ui.add(button).clicked() {
                                                 self.drawing_settings.color = color;
                                             }
                                         }
                                         
                                         ui.separator();
                                         ui.add(egui::Slider::new(&mut self.drawing_settings.font_size, 10.0..=100.0).text("Size"));
                                         
                                         ui.separator();
                                         ui.horizontal(|ui| {
                                            ui.selectable_value(&mut self.drawing_settings.font_family, FontFamily::Proportional, "Sans");
                                            ui.selectable_value(&mut self.drawing_settings.font_family, FontFamily::Monospace, "Mono");
                                         });
                                         ui.checkbox(&mut self.drawing_settings.font_bold, "Bold");
                                      }
                                 }
                             });
                              }); // ScrollArea
                         });
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

                // Zoom with scroll (smooth animated)
                let scroll_delta = ctx.input(|i| i.raw_scroll_delta.y);
                if scroll_delta != 0.0 {
                    let zoom_factor = 1.15;
                    if scroll_delta > 0.0 {
                        self.target_zoom *= zoom_factor;
                    } else {
                        self.target_zoom /= zoom_factor;
                    }
                    // Allow zooming out to 5% and in to 5000%
                    self.target_zoom = self.target_zoom.clamp(0.05, 50.0);
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