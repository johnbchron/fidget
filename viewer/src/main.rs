use anyhow::Result;
use clap::Parser;
use eframe::egui;
use env_logger::Env;
use fidget::{eval::Family, render::RenderConfig};
use log::info;
use nalgebra::{Transform2, Vector2};
use notify::Watcher;

use std::{
    hash::{Hash, Hasher},
    path::Path,
};

/// Simple test program
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Render `.dot` files representing compilation
    #[clap(short, long)]
    dot: bool,

    /// File to watch
    target: String,
}

fn file_watcher_thread(
    path: &Path,
    rx: crossbeam_channel::Receiver<()>,
    tx: crossbeam_channel::Sender<String>,
) {
    use std::collections::hash_map::DefaultHasher;

    let read_file = || {
        let contents = std::fs::read(path).unwrap();
        let mut hasher = DefaultHasher::new();
        contents.hash(&mut hasher);
        (contents, hasher.finish())
    };
    let (contents, mut hash) = read_file();
    tx.send(String::from_utf8(contents).unwrap());

    // Wait for a file change notification
    while let Ok(()) = rx.recv() {
        let (contents, new_hash) = read_file();
        if new_hash != hash {
            hash = new_hash;
            info!("file contents changed!");
            tx.send(String::from_utf8(contents).unwrap());
        }
    }
}

fn rhai_script_thread(
    rx: crossbeam_channel::Receiver<String>,
    tx: crossbeam_channel::Sender<Result<fidget::rhai::ScriptContext, String>>,
) {
    while let Ok(script) = rx.recv() {
        info!("Got script!");
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .init();
    let args = Args::parse();

    let (file_watcher_tx, file_watcher_rx) = crossbeam_channel::unbounded();
    let (rhai_script_tx, rhai_script_rx) = crossbeam_channel::unbounded();
    let (rhai_result_tx, rhai_result_rx) = crossbeam_channel::unbounded();
    let path = Path::new(&args.target).to_owned();
    std::thread::spawn(move || {
        file_watcher_thread(&path, file_watcher_rx, rhai_script_tx)
    });
    std::thread::spawn(move || {
        rhai_script_thread(rhai_script_rx, rhai_result_tx)
    });

    // Automatically select the best implementation for your platform.
    let mut watcher = notify::recommended_watcher(move |res| match res {
        Ok(event) => {
            println!("{event:?}");
            file_watcher_tx.send(()).unwrap();
        }
        Err(e) => panic!("watch error: {:?}", e),
    })?;
    watcher
        .watch(Path::new(&args.target), notify::RecursiveMode::NonRecursive)?;

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Fidget",
        options,
        Box::new(|_cc| Box::new(ViewerApp::default())),
    );

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////

struct TwoDCamera {
    // 2D camera parameters
    scale: f32,
    offset: egui::Vec2,
    drag_start: Option<egui::Vec2>,
}

impl TwoDCamera {
    /// Converts from mouse position to a UV position within the render window
    fn mouse_to_uv(
        &self,
        rect: egui::Rect,
        uv: egui::Rect,
        p: egui::Pos2,
    ) -> egui::Vec2 {
        let r = (p - rect.min) / (rect.max - rect.min);
        const ONE: egui::Vec2 = egui::Vec2::new(1.0, 1.0);
        let pos = uv.min.to_vec2() * (ONE - r) + uv.max.to_vec2() * r;
        let out = ((pos * 2.0) - ONE) * self.scale;
        egui::Vec2::new(out.x, -out.y) + self.offset
    }
}

impl Default for TwoDCamera {
    fn default() -> Self {
        TwoDCamera {
            drag_start: None,
            scale: 1.0,
            offset: egui::Vec2::ZERO,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum TwoDMode {
    Color,
    Sdf,
    Debug,
}

////////////////////////////////////////////////////////////////////////////////

struct ThreeDCamera {
    // 2D camera parameters
    scale: f32,
    offset: nalgebra::Vector3<f32>,
    drag_start: Option<egui::Vec2>,
}

impl ThreeDCamera {
    fn mouse_to_uv(
        &self,
        rect: egui::Rect,
        uv: egui::Rect,
        p: egui::Pos2,
    ) -> egui::Vec2 {
        panic!()
    }
}

impl Default for ThreeDCamera {
    fn default() -> Self {
        ThreeDCamera {
            drag_start: None,
            scale: 1.0,
            offset: nalgebra::Vector3::zeros(),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum ThreeDMode {
    Color,
    Heightmap,
}

////////////////////////////////////////////////////////////////////////////////

enum RenderMode {
    TwoD(TwoDCamera, TwoDMode),
    ThreeD(ThreeDCamera, ThreeDMode),
}

impl RenderMode {
    fn set_2d_mode(&mut self, mode: TwoDMode) {
        match self {
            RenderMode::TwoD(.., m) => *m = mode,
            RenderMode::ThreeD(..) => {
                *self = RenderMode::TwoD(TwoDCamera::default(), mode)
            }
        }
    }
    fn set_3d_mode(&mut self, mode: ThreeDMode) {
        match self {
            RenderMode::TwoD(..) => {
                *self = RenderMode::ThreeD(ThreeDCamera::default(), mode)
            }
            RenderMode::ThreeD(_camera, m) => {
                *m = mode;
            }
        }
    }
}

struct ViewerApp {
    // Current image
    texture: Option<egui::TextureHandle>,

    // Evaluator engine
    engine: fidget::rhai::Engine,

    /// Current render mode
    mode: RenderMode,

    // Current contents of the editable script
    script: String,

    // Most recent result, or an error string
    out: Result<fidget::rhai::ScriptContext, String>,
}

////////////////////////////////////////////////////////////////////////////////

impl Default for ViewerApp {
    fn default() -> Self {
        let engine = fidget::rhai::Engine::new();

        Self {
            texture: None,
            engine,
            script: "".to_owned(), //"draw(circle(0, 0, 0.5))".to_owned(),
            out: Err("".to_string()),

            mode: RenderMode::TwoD(TwoDCamera::default(), TwoDMode::Color),
        }
    }
}

impl ViewerApp {
    fn solarized(&mut self, ctx: &egui::Context) {
        /*
        let mut theme = egui::Visuals::dark();

        let f = |c: Option<syntect::highlighting::Color>| {
            c.map(|c| egui::Color32::from_rgb(c.r, c.g, c.b)).unwrap()
        };
        let highlight = crate::highlight::get_theme();
        let sol = highlight.settings;

        theme.extreme_bg_color = f(sol.background);
        theme.widgets.noninteractive.bg_fill = f(sol.gutter);
        theme.widgets.noninteractive.bg_stroke = egui::Stroke::none();
        theme.widgets.hovered.bg_stroke =
            egui::Stroke::new(1.0, f(sol.selection_border));

        theme.widgets.hovered.bg_fill = f(sol.selection);
        theme.widgets.active.bg_fill = f(sol.selection_border);

        theme.selection.bg_fill = f(sol.selection);
        theme.selection.stroke =
            egui::Stroke::new(1.0, f(sol.selection_border));
        theme.widgets.noninteractive.fg_stroke =
            egui::Stroke::new(0.0, f(sol.foreground));

        ctx.set_visuals(theme);
        */
    }

    fn render(
        &self,
        tape: fidget::eval::Tape<fidget::jit::Eval>,
        image_size: usize,
        color: [u8; 3],
        pixels: &mut [egui::Color32],
    ) {
        match &self.mode {
            RenderMode::TwoD(camera, mode) => {
                let mat = Transform2::from_matrix_unchecked(
                    Transform2::identity()
                        .matrix()
                        .append_scaling(camera.scale)
                        .append_translation(&Vector2::new(
                            camera.offset.x,
                            camera.offset.y,
                        )),
                );

                let config = RenderConfig {
                    image_size,
                    tile_sizes: fidget::jit::Eval::tile_sizes_2d().to_vec(),
                    threads: 8,

                    mat,
                };
                match mode {
                    TwoDMode::Color => {
                        let image = fidget::render::render2d(
                            tape,
                            &config,
                            &fidget::render::BitRenderMode,
                        );
                        for i in 0..pixels.len() {
                            if image[i] {
                                pixels[i] =
                                    egui::Color32::from_rgba_unmultiplied(
                                        color[0],
                                        color[1],
                                        color[2],
                                        u8::MAX,
                                    );
                            }
                        }
                    }

                    TwoDMode::Sdf => {
                        let image = fidget::render::render2d(
                            tape,
                            &config,
                            &fidget::render::SdfRenderMode,
                        );
                        for i in 0..pixels.len() {
                            pixels[i] = egui::Color32::from_rgba_unmultiplied(
                                image[i][0],
                                image[i][1],
                                image[i][2],
                                u8::MAX,
                            );
                        }
                    }

                    TwoDMode::Debug => {
                        let image = fidget::render::render2d(
                            tape,
                            &config,
                            &fidget::render::DebugRenderMode,
                        );
                        for i in 0..pixels.len() {
                            let p = image[i].as_debug_color();
                            pixels[i] = egui::Color32::from_rgba_unmultiplied(
                                p[0],
                                p[1],
                                p[2],
                                u8::MAX,
                            );
                        }
                    }
                }
            }
            RenderMode::ThreeD(camera, mode) => {
                unimplemented!()
            }
        };
    }
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.solarized(ctx);

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open").clicked() {
                        println!("HI")
                    }
                });
                ui.menu_button("Config", |ui| {
                    let mut mode_3d = match &self.mode {
                        RenderMode::TwoD(..) => None,
                        RenderMode::ThreeD(_camera, mode) => Some(*mode),
                    };
                    ui.radio_value(
                        &mut mode_3d,
                        Some(ThreeDMode::Heightmap),
                        "3D heightmap",
                    );
                    ui.radio_value(
                        &mut mode_3d,
                        Some(ThreeDMode::Color),
                        "3D color",
                    );
                    if let Some(m) = mode_3d {
                        self.mode.set_3d_mode(m);
                    }
                    ui.separator();
                    let mut mode_2d = match &self.mode {
                        RenderMode::TwoD(_camera, mode) => Some(*mode),
                        RenderMode::ThreeD(..) => None,
                    };
                    ui.radio_value(
                        &mut mode_2d,
                        Some(TwoDMode::Debug),
                        "2D debug",
                    );
                    ui.radio_value(&mut mode_2d, Some(TwoDMode::Sdf), "2D SDF");
                    ui.radio_value(
                        &mut mode_2d,
                        Some(TwoDMode::Color),
                        "2D Color",
                    );

                    if let Some(m) = mode_2d {
                        self.mode.set_2d_mode(m);
                    }
                });
            });
        });

        let rect = ctx.available_rect();
        let size = rect.max - rect.min;
        let max_size = size.x.max(size.y);
        let image_size = (max_size * ctx.pixels_per_point()) as usize;

        let mut image = egui::ImageData::Color(egui::ColorImage::new(
            [image_size; 2],
            egui::Color32::BLACK,
        ));
        let pixels = match &mut image {
            egui::ImageData::Color(c) => &mut c.pixels,
            _ => panic!(),
        };

        // Render shapes into self.texture
        let render_start = std::time::Instant::now();

        if let Ok(script_ctx) = &self.out {
            for s in script_ctx.shapes.iter() {
                let tape: fidget::eval::Tape<fidget::jit::Eval> =
                    script_ctx.context.get_tape(s.shape).unwrap();
                self.render(tape, image_size, s.color_rgb, pixels);
            }
        }

        match self.texture.as_mut() {
            Some(t) => {
                if t.size() == [image_size; 2] {
                    t.set(image, egui::TextureFilter::Linear)
                } else {
                    *t = ctx.load_texture(
                        "tex",
                        image,
                        egui::TextureFilter::Linear,
                    )
                }
            }
            None => {
                let texture =
                    ctx.load_texture("tex", image, egui::TextureFilter::Linear);
                self.texture = Some(texture);
            }
        }
        let dt = render_start.elapsed();

        let uv = if size.x > size.y {
            let r = (1.0 - (size.y / size.x)) / 2.0;
            egui::Rect {
                min: egui::Pos2::new(0.0, r),
                max: egui::Pos2::new(1.0, 1.0 - r),
            }
        } else {
            let r = (1.0 - (size.x / size.y)) / 2.0;
            egui::Rect {
                min: egui::Pos2::new(r, 0.0),
                max: egui::Pos2::new(1.0 - r, 1.0),
            }
        };

        let r = egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                let pos = ui.next_widget_position();
                let size = ui.available_size();
                let painter = ui.painter_at(egui::Rect {
                    min: pos,
                    max: pos + size,
                });

                if let Some(t) = self.texture.as_ref() {
                    let mut mesh = egui::Mesh::with_texture(t.id());
                    mesh.add_rect_with_uv(rect, uv, egui::Color32::WHITE);
                    painter.add(mesh);
                }

                let layout = painter.layout(
                    format!(
                        "Image size: {0}x{0}\nRender time: {dt:.2?}",
                        image_size
                    ),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                    f32::INFINITY,
                );
                let padding = egui::Vec2 { x: 10.0, y: 10.0 };
                let text_corner = rect.max - layout.size();
                painter.rect_filled(
                    egui::Rect {
                        min: text_corner - 2.0 * padding,
                        max: rect.max,
                    },
                    egui::Rounding::none(),
                    egui::Color32::from_black_alpha(128),
                );
                painter.galley(text_corner - padding, layout);

                // Return events from the canvas in the inner response
                ui.interact(
                    rect,
                    egui::Id::new("canvas"),
                    egui::Sense::click_and_drag(),
                )
            });

        // Handle pan and zoom
        match &mut self.mode {
            RenderMode::TwoD(camera, ..) => {
                if let Some(pos) = r.inner.interact_pointer_pos() {
                    if let Some(start) = camera.drag_start {
                        camera.offset = egui::Vec2::ZERO;
                        let pos = camera.mouse_to_uv(rect, uv, pos);
                        camera.offset = start - pos;
                    } else {
                        let pos = camera.mouse_to_uv(rect, uv, pos);
                        camera.drag_start = Some(pos);
                    }
                } else {
                    camera.drag_start = None;
                }

                if r.inner.hovered() {
                    let mouse_pos = ctx.input().pointer.hover_pos();
                    let pos_before =
                        mouse_pos.map(|p| camera.mouse_to_uv(rect, uv, p));
                    camera.scale /= (ctx.input().scroll_delta.y / 100.0).exp2();
                    if let Some(pos_before) = pos_before {
                        let pos_after =
                            camera.mouse_to_uv(rect, uv, mouse_pos.unwrap());
                        camera.offset += pos_before - pos_after;
                    }
                }
            }
            RenderMode::ThreeD(camera, ..) => {
                unimplemented!()
            }
        }
    }
}
