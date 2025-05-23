use anyhow::Result;
use include_dir::Dir;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::image_loader::ImageSequence;
use crate::renderer::Renderer;

pub struct OverlayApplication<'a> {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    image_sequence: Option<ImageSequence>,
    image_directory: Option<PathBuf>,
    embedded_dir: Option<&'a Dir<'a>>,
    last_frame_time: Instant,
    frame_interval: Duration,
    current_frame_index: usize,
    frame_count: usize,
}

impl OverlayApplication<'static> {
    pub fn new_embedded(dir: &'static Dir, frame_interval: Duration) -> Self {
        Self {
            window: None,
            renderer: None,
            image_sequence: None,
            image_directory: None,
            embedded_dir: Some(dir),
            last_frame_time: Instant::now(),
            frame_interval,
            current_frame_index: 0,
            frame_count: 0,
        }
    }

    pub fn run(&mut self) -> Result<()> {
        let event_loop = EventLoop::new()?;

        // Load the image sequence based on which constructor was used
        self.image_sequence = if let Some(dir) = self.image_directory.as_ref() {
            Some(ImageSequence::load(dir)?)
        } else if let Some(dir) = self.embedded_dir {
            Some(ImageSequence::load_embedded(&dir)?)
        } else {
            return Err(anyhow::format_err!("No image source specified"));
        };

        if let Some(sequence) = &self.image_sequence {
            self.frame_count = sequence.count();
            log::info!("Loaded {} images in sequence", self.frame_count);
        } else {
            log::error!("Failed to load image sequence");
            return Ok(());
        }

        event_loop.run_app(self)?;

        Ok(())
    }

    fn update(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_frame_time) >= self.frame_interval {
            self.last_frame_time = now;

            if self.frame_count > 0 {
                self.current_frame_index = (self.current_frame_index + 1) % self.frame_count;

                if let Some(renderer) = &mut self.renderer {
                    renderer.set_current_texture_index(self.current_frame_index);
                }
            }
        }
    }

    fn render(&mut self) -> Result<()> {
        if let Some(renderer) = &mut self.renderer {
            renderer.render()?;
        }

        Ok(())
    }
}

impl ApplicationHandler for OverlayApplication<'static> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let (width, height) = if let Some(sequence) = &self.image_sequence {
            if let Some(image) = sequence.current_image() {
                let dimensions = image.dimensions();
                log::info!(
                    "Using image dimensions for window: {}x{}",
                    dimensions.0,
                    dimensions.1
                );
                (dimensions.0, dimensions.1)
            } else {
                log::info!("No image found, using default dimensions");
                (800, 600)
            }
        } else {
            log::info!("No image sequence found, using default dimensions");
            (800, 600)
        };

        let window_attributes = WindowAttributes::default()
            .with_title("PNG Overlay")
            .with_transparent(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(PhysicalSize::new(width, height));

        match event_loop.create_window(window_attributes) {
            Ok(window) => {
                let window_arc = Arc::new(window);
                self.window = Some(window_arc.clone());

                pollster::block_on(async {
                    match Renderer::new(window_arc).await {
                        Ok(mut renderer) => {
                            if let Some(sequence) = &self.image_sequence {
                                let all_images = sequence.get_all_images();

                                renderer.preload_images(&all_images);

                                log::info!("Preloaded {} images to GPU memory", all_images.len());
                            }

                            self.renderer = Some(renderer);
                        }
                        Err(err) => {
                            log::error!("Failed to create renderer: {}", err);
                            event_loop.exit();
                        }
                    }
                });
            }
            Err(err) => {
                log::error!("Failed to create window: {}", err);
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            winit::event::WindowEvent::CloseRequested => {
                log::info!("Window close requested");
                event_loop.exit();
            }
            winit::event::WindowEvent::Resized(size) => {
                log::info!("Window resized to {}x{}", size.width, size.height);
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                self.update();

                if let Err(err) = self.render() {
                    log::error!("Render error: {}", err);
                }

                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now.duration_since(self.last_frame_time) >= self.frame_interval {
            if let Some(window) = &self.window {
                window.request_redraw();
                event_loop.set_control_flow(ControlFlow::WaitUntil(now + self.frame_interval));
            }
        }
    }
}
