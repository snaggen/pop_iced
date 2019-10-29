use crate::{quad, Image, Primitive, Quad, Transformation};
use iced_native::{
    renderer::Debugger, renderer::Windowed, Background, Color, Layout,
    MouseCursor, Point, Rectangle, Widget,
};

use raw_window_handle::HasRawWindowHandle;
use wgpu::{
    Adapter, BackendBit, CommandEncoderDescriptor, Device, DeviceDescriptor,
    Extensions, Limits, PowerPreference, Queue, RequestAdapterOptions, Surface,
    SwapChain, SwapChainDescriptor, TextureFormat, TextureUsage,
};
use wgpu_glyph::{GlyphBrush, GlyphBrushBuilder, Section};

use std::{cell::RefCell, rc::Rc};

mod button;
mod checkbox;
mod column;
mod image;
mod radio;
mod row;
mod scrollable;
mod slider;
mod text;

pub struct Renderer {
    surface: Surface,
    device: Device,
    queue: Queue,
    quad_pipeline: quad::Pipeline,
    image_pipeline: crate::image::Pipeline,

    glyph_brush: Rc<RefCell<GlyphBrush<'static, ()>>>,
}

pub struct Target {
    width: u16,
    height: u16,
    transformation: Transformation,
    swap_chain: SwapChain,
}

pub struct Layer<'a> {
    bounds: Rectangle<u32>,
    y_offset: u32,
    quads: Vec<Quad>,
    images: Vec<Image>,
    text: Vec<wgpu_glyph::Section<'a>>,
    layers: Vec<Layer<'a>>,
}

impl<'a> Layer<'a> {
    pub fn new(bounds: Rectangle<u32>, y_offset: u32) -> Self {
        Self {
            bounds,
            y_offset,
            quads: Vec::new(),
            images: Vec::new(),
            text: Vec::new(),
            layers: Vec::new(),
        }
    }
}

impl Renderer {
    fn new<W: HasRawWindowHandle>(window: &W) -> Self {
        let adapter = Adapter::request(&RequestAdapterOptions {
            power_preference: PowerPreference::LowPower,
            backends: BackendBit::all(),
        })
        .expect("Request adapter");

        let (mut device, queue) = adapter.request_device(&DeviceDescriptor {
            extensions: Extensions {
                anisotropic_filtering: false,
            },
            limits: Limits { max_bind_groups: 2 },
        });

        let surface = Surface::create(window);

        // TODO: Think about font loading strategy
        // Loading system fonts with fallback may be a good idea
        let font: &[u8] =
            include_bytes!("../../examples/resources/Roboto-Regular.ttf");

        let glyph_brush = GlyphBrushBuilder::using_font_bytes(font)
            .build(&mut device, TextureFormat::Bgra8UnormSrgb);

        let quad_pipeline = quad::Pipeline::new(&mut device);
        let image_pipeline = crate::image::Pipeline::new(&mut device);

        Self {
            surface,
            device,
            queue,
            quad_pipeline,
            image_pipeline,

            glyph_brush: Rc::new(RefCell::new(glyph_brush)),
        }
    }

    fn target(&self, width: u16, height: u16) -> Target {
        Target {
            width,
            height,
            transformation: Transformation::orthographic(width, height),
            swap_chain: self.device.create_swap_chain(
                &self.surface,
                &SwapChainDescriptor {
                    usage: TextureUsage::OUTPUT_ATTACHMENT,
                    format: TextureFormat::Bgra8UnormSrgb,
                    width: u32::from(width),
                    height: u32::from(height),
                    present_mode: wgpu::PresentMode::Vsync,
                },
            ),
        }
    }

    fn draw(
        &mut self,
        (primitive, mouse_cursor): &(Primitive, MouseCursor),
        target: &mut Target,
    ) -> MouseCursor {
        log::debug!("Drawing");

        let frame = target.swap_chain.get_next_texture();

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { todo: 0 });

        let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                attachment: &frame.view,
                resolve_target: None,
                load_op: wgpu::LoadOp::Clear,
                store_op: wgpu::StoreOp::Store,
                clear_color: wgpu::Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
            }],
            depth_stencil_attachment: None,
        });

        let mut layer = Layer::new(
            Rectangle {
                x: 0,
                y: 0,
                width: u32::from(target.width),
                height: u32::from(target.height),
            },
            0,
        );

        self.draw_primitive(primitive, &mut layer);
        self.flush(target.transformation, &layer, &mut encoder, &frame.view);

        self.queue.submit(&[encoder.finish()]);

        *mouse_cursor
    }

    fn draw_primitive<'a>(
        &mut self,
        primitive: &'a Primitive,
        layer: &mut Layer<'a>,
    ) {
        match primitive {
            Primitive::None => {}
            Primitive::Group { primitives } => {
                // TODO: Inspect a bit and regroup (?)
                for primitive in primitives {
                    self.draw_primitive(primitive, layer)
                }
            }
            Primitive::Text {
                content,
                bounds,
                size,
                color,
                horizontal_alignment,
                vertical_alignment,
            } => {
                let x = match horizontal_alignment {
                    iced_native::text::HorizontalAlignment::Left => bounds.x,
                    iced_native::text::HorizontalAlignment::Center => {
                        bounds.x + bounds.width / 2.0
                    }
                    iced_native::text::HorizontalAlignment::Right => {
                        bounds.x + bounds.width
                    }
                };

                let y = match vertical_alignment {
                    iced_native::text::VerticalAlignment::Top => bounds.y,
                    iced_native::text::VerticalAlignment::Center => {
                        bounds.y + bounds.height / 2.0
                    }
                    iced_native::text::VerticalAlignment::Bottom => {
                        bounds.y + bounds.height
                    }
                };

                layer.text.push(Section {
                    text: &content,
                    screen_position: (x, y),
                    bounds: (bounds.width, bounds.height),
                    scale: wgpu_glyph::Scale { x: *size, y: *size },
                    color: color.into_linear(),
                    layout: wgpu_glyph::Layout::default()
                        .h_align(match horizontal_alignment {
                            iced_native::text::HorizontalAlignment::Left => {
                                wgpu_glyph::HorizontalAlign::Left
                            }
                            iced_native::text::HorizontalAlignment::Center => {
                                wgpu_glyph::HorizontalAlign::Center
                            }
                            iced_native::text::HorizontalAlignment::Right => {
                                wgpu_glyph::HorizontalAlign::Right
                            }
                        })
                        .v_align(match vertical_alignment {
                            iced_native::text::VerticalAlignment::Top => {
                                wgpu_glyph::VerticalAlign::Top
                            }
                            iced_native::text::VerticalAlignment::Center => {
                                wgpu_glyph::VerticalAlign::Center
                            }
                            iced_native::text::VerticalAlignment::Bottom => {
                                wgpu_glyph::VerticalAlign::Bottom
                            }
                        }),
                    ..Default::default()
                })
            }
            Primitive::Quad {
                bounds,
                background,
                border_radius,
            } => {
                layer.quads.push(Quad {
                    position: [bounds.x, bounds.y - layer.y_offset as f32],
                    scale: [bounds.width, bounds.height],
                    color: match background {
                        Background::Color(color) => color.into_linear(),
                    },
                    border_radius: u32::from(*border_radius),
                });
            }
            Primitive::Image { path, bounds } => {
                layer.images.push(Image {
                    path: path.clone(),
                    position: [bounds.x, bounds.y],
                    scale: [bounds.width, bounds.height],
                });
            }
            Primitive::Scrollable {
                bounds,
                offset,
                content,
            } => {
                let mut new_layer = Layer::new(
                    Rectangle {
                        x: bounds.x as u32,
                        y: bounds.y as u32 - layer.y_offset,
                        width: bounds.width as u32,
                        height: bounds.height as u32,
                    },
                    layer.y_offset + offset,
                );

                // TODO: Primitive culling
                self.draw_primitive(content, &mut new_layer);

                layer.layers.push(new_layer);
            }
        }
    }

    fn flush(
        &mut self,
        transformation: Transformation,
        layer: &Layer,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) {
        let translated = transformation
            * Transformation::translate(0.0, -(layer.y_offset as f32));

        if layer.quads.len() > 0 {
            self.quad_pipeline.draw(
                &mut self.device,
                encoder,
                &layer.quads,
                transformation,
                layer.bounds,
                target,
            );
        }

        if layer.images.len() > 0 {
            self.image_pipeline.draw(
                &mut self.device,
                encoder,
                &layer.images,
                translated,
                layer.bounds,
                target,
            );
        }

        if layer.text.len() > 0 {
            let mut glyph_brush = self.glyph_brush.borrow_mut();

            for text in layer.text.iter() {
                glyph_brush.queue(text);
            }

            glyph_brush
                .draw_queued_with_transform_and_scissoring(
                    &mut self.device,
                    encoder,
                    target,
                    translated.into(),
                    wgpu_glyph::Region {
                        x: layer.bounds.x,
                        y: layer.bounds.y,
                        width: layer.bounds.width,
                        height: layer.bounds.height,
                    },
                )
                .expect("Draw text");
        }

        for layer in layer.layers.iter() {
            self.flush(transformation, layer, encoder, target);
        }
    }
}

impl iced_native::Renderer for Renderer {
    type Output = (Primitive, MouseCursor);
}

impl Windowed for Renderer {
    type Target = Target;

    fn new<W: HasRawWindowHandle>(window: &W) -> Self {
        Self::new(window)
    }

    fn target(&self, width: u16, height: u16) -> Target {
        self.target(width, height)
    }

    fn draw(
        &mut self,
        output: &Self::Output,
        target: &mut Target,
    ) -> MouseCursor {
        self.draw(output, target)
    }
}

impl Debugger for Renderer {
    fn explain<Message>(
        &mut self,
        widget: &dyn Widget<Message, Self>,
        layout: Layout<'_>,
        cursor_position: Point,
        color: Color,
    ) -> Self::Output {
        let mut primitives = Vec::new();
        let (primitive, cursor) = widget.draw(self, layout, cursor_position);

        explain_layout(layout, color, &mut primitives);
        primitives.push(primitive);

        (Primitive::Group { primitives }, cursor)
    }
}

fn explain_layout(
    layout: Layout,
    color: Color,
    primitives: &mut Vec<Primitive>,
) {
    // TODO: Draw borders instead
    primitives.push(Primitive::Quad {
        bounds: layout.bounds(),
        background: Background::Color(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.05,
        }),
        border_radius: 0,
    });

    for child in layout.children() {
        explain_layout(child, color, primitives);
    }
}
