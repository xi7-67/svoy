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

struct ImageViewer {
    texture: Option<egui::TextureHandle>,
    error_message: Option<String>,
    
    // Navigation
    current_path: Option<PathBuf>,
    image_list: Vec<PathBuf>,
    current_index: usize,

    // Transformation
    zoom: f32,
    target_zoom: f32,
    offset: egui::Vec2,
    last_frame_time: Instant,
}

impl ImageViewer {
    fn new(cc: &eframe::CreationContext<'_>, initial_path: Option<PathBuf>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        
        let mut viewer = Self {
            texture: None,
            error_message: None,
            current_path: None,
            image_list: Vec::new(),
            current_index: 0,
            zoom: 1.0,
            target_zoom: 1.0,
            offset: egui::Vec2::ZERO,
            last_frame_time: Instant::now(),
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
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                let texture = ctx.load_texture("img", color_image, egui::TextureOptions::LINEAR);
                self.texture = Some(texture);
                self.error_message = None;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load: {}", e));
                self.texture = None;
            }
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

        egui::CentralPanel::default().show(ctx, |ui| {
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
                let (rect, response) = ui.allocate_exact_size(available_size, egui::Sense::drag());

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

                // Paint
                let painter = ui.painter_at(rect);
                painter.image(
                    texture.id(),
                    image_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE
                );
            } else {
                 ui.centered_and_justified(|ui| ui.label("Open an image"));
            }
        });
    }
}