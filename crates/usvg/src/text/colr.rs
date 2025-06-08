// Copyright 2024 the Resvg Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::parser::OptionLog;
use rustybuzz::ttf_parser;
use skrifa::{color::{Brush, Extend, Transform}, outline::DrawSettings, MetadataProvider};
use std::fmt::Write as _;

struct Builder<'a>(&'a mut String);

impl Builder<'_> {
    fn finish(&mut self) {
        if !self.0.is_empty() {
            self.0.pop(); // remove trailing space
        }
    }
}

impl ttf_parser::OutlineBuilder for Builder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        use std::fmt::Write;
        write!(self.0, "M {} {} ", x, y).unwrap();
    }

    fn line_to(&mut self, x: f32, y: f32) {
        use std::fmt::Write;
        write!(self.0, "L {} {} ", x, y).unwrap();
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        use std::fmt::Write;
        write!(self.0, "Q {} {} {} {} ", x1, y1, x, y).unwrap();
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        use std::fmt::Write;
        write!(self.0, "C {} {} {} {} {} {} ", x1, y1, x2, y2, x, y).unwrap();
    }

    fn close(&mut self) {
        self.0.push_str("Z ");
    }
}

impl skrifa::outline::OutlinePen for Builder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        write!(self.0, "M {} {} ", x, y).unwrap();
    }

    fn line_to(&mut self, x: f32, y: f32) {
        write!(self.0, "L {} {} ", x, y).unwrap();
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        write!(self.0, "Q {} {} {} {} ", cx0, cy0, x, y).unwrap();
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        write!(self.0, "C {} {} {} {} {} {} ", cx0, cy0, cx1, cy1, x, y).unwrap();
    }

    fn close(&mut self) {
        self.0.push_str("Z ");
    }
}

trait XmlWriterExt {
    fn write_color_attribute(&mut self, name: &str, ts: ttf_parser::RgbaColor);
    fn write_transform_attribute(&mut self, name: &str, ts: Transform);
    fn write_spread_method_attribute(&mut self, method: Extend);
}

impl XmlWriterExt for xmlwriter::XmlWriter {
    fn write_color_attribute(&mut self, name: &str, color: ttf_parser::RgbaColor) {
        self.write_attribute_fmt(
            name,
            format_args!("rgb({}, {}, {})", color.red, color.green, color.blue),
        );
    }

    fn write_transform_attribute(&mut self, name: &str, ts: Transform) {
        if ts == Transform::default() {
            return;
        }

        self.write_attribute_fmt(
            name,
            format_args!(
                "matrix({} {} {} {} {} {})",
                ts.xx, ts.yx, ts.yx, ts.yy, ts.dx, ts.dy
            ),
        );
    }

    fn write_spread_method_attribute(&mut self, extend: Extend) {
        self.write_attribute(
            "spreadMethod",
            match extend {
                Extend::Pad => &"pad",
                Extend::Repeat => &"repeat",
                Extend::Reflect => &"reflect",
                _ => return,
            },
        );
    }
}

// NOTE: This is only a best-effort translation of COLR into SVG.
pub(crate) struct GlyphPainter<'a> {
    pub(crate) face: &'a ttf_parser::Face<'a>,
    pub(crate) font: &'a skrifa::FontRef<'a>,
    pub(crate) svg: &'a mut xmlwriter::XmlWriter,
    pub(crate) path_buf: &'a mut String,
    pub(crate) gradient_index: usize,
    pub(crate) clip_path_index: usize,
    pub(crate) palette_index: u16,
    pub(crate) transform: Transform,
    pub(crate) outline_transform: Transform,
    pub(crate) transforms_stack: Vec<Transform>,
}

impl<'a> GlyphPainter<'a> {
    fn write_gradient_stops(&mut self, stops: ttf_parser::colr::GradientStopsIter) {
        for stop in stops {
            self.svg.start_element("stop");
            self.svg.write_attribute("offset", &stop.stop_offset);
            self.svg.write_color_attribute("stop-color", stop.color);
            let opacity = f32::from(stop.color.alpha) / 255.0;
            self.svg.write_attribute("stop-opacity", &opacity);
            self.svg.end_element();
        }
    }

    fn paint_solid(&mut self, palette_index: u16, alpha: f32) {
        self.svg.start_element("path");
        self.svg.write_color_attribute("fill", color);
        let opacity = alpha / 255.0;
        self.svg.write_attribute("fill-opacity", &opacity);
        self.svg
            .write_transform_attribute("transform", self.outline_transform);
        self.svg.write_attribute("d", self.path_buf);
        self.svg.end_element();
    }

    fn paint_linear_gradient(&mut self, gradient: ttf_parser::colr::LinearGradient<'a>) {
        let gradient_id = format!("lg{}", self.gradient_index);
        self.gradient_index += 1;

        let gradient_transform = paint_transform(self.outline_transform, self.transform);

        // TODO: We ignore x2, y2. Have to apply them somehow.
        // TODO: The way spreadMode works in ttf and svg is a bit different. In SVG, the spreadMode
        // will always be applied based on x1/y1 and x2/y2. However, in TTF the spreadMode will
        // be applied from the first/last stop. So if we have a gradient with x1=0 x2=1, and
        // a stop at x=0.4 and x=0.6, then in SVG we will always see a padding, while in ttf
        // we will see the actual spreadMode. We need to account for that somehow.
        self.svg.start_element("linearGradient");
        self.svg.write_attribute("id", &gradient_id);
        self.svg.write_attribute("x1", &gradient.x0);
        self.svg.write_attribute("y1", &gradient.y0);
        self.svg.write_attribute("x2", &gradient.x1);
        self.svg.write_attribute("y2", &gradient.y1);
        self.svg.write_attribute("gradientUnits", &"userSpaceOnUse");
        self.svg.write_spread_method_attribute(gradient.extend);
        self.svg
            .write_transform_attribute("gradientTransform", gradient_transform);
        self.write_gradient_stops(
            gradient.stops(self.palette_index, self.face.variation_coordinates()),
        );
        self.svg.end_element();

        self.svg.start_element("path");
        self.svg
            .write_attribute_fmt("fill", format_args!("url(#{})", gradient_id));
        self.svg
            .write_transform_attribute("transform", self.outline_transform);
        self.svg.write_attribute("d", self.path_buf);
        self.svg.end_element();
    }

    fn paint_radial_gradient(&mut self, gradient: ttf_parser::colr::RadialGradient<'a>) {
        let gradient_id = format!("rg{}", self.gradient_index);
        self.gradient_index += 1;

        let gradient_transform = paint_transform(self.outline_transform, self.transform);

        self.svg.start_element("radialGradient");
        self.svg.write_attribute("id", &gradient_id);
        self.svg.write_attribute("cx", &gradient.x1);
        self.svg.write_attribute("cy", &gradient.y1);
        self.svg.write_attribute("r", &gradient.r1);
        self.svg.write_attribute("fr", &gradient.r0);
        self.svg.write_attribute("fx", &gradient.x0);
        self.svg.write_attribute("fy", &gradient.y0);
        self.svg.write_attribute("gradientUnits", &"userSpaceOnUse");
        self.svg.write_spread_method_attribute(gradient.extend);
        self.svg
            .write_transform_attribute("gradientTransform", gradient_transform);
        self.write_gradient_stops(
            gradient.stops(self.palette_index, self.face.variation_coordinates()),
        );
        self.svg.end_element();

        self.svg.start_element("path");
        self.svg
            .write_attribute_fmt("fill", format_args!("url(#{})", gradient_id));
        self.svg
            .write_transform_attribute("transform", self.outline_transform);
        self.svg.write_attribute("d", self.path_buf);
        self.svg.end_element();
    }

    fn paint_sweep_gradient(&mut self, _: ttf_parser::colr::SweepGradient<'a>) {
        println!("Warning: sweep gradients are not supported.");
    }
}

fn paint_transform(outline_transform: Transform, transform: Transform) -> Transform {
    let outline_transform = tiny_skia_path::Transform::from_row(
        outline_transform.a,
        outline_transform.b,
        outline_transform.c,
        outline_transform.d,
        outline_transform.e,
        outline_transform.f,
    );

    let gradient_transform = tiny_skia_path::Transform::from_row(
        transform.a,
        transform.b,
        transform.c,
        transform.d,
        transform.e,
        transform.f,
    );

    let gradient_transform = outline_transform
        .invert()
        .log_none(|| log::warn!("Failed to calculate transform for gradient in glyph."))
        .unwrap_or_default()
        .pre_concat(gradient_transform);

    Transform {
        a: gradient_transform.sx,
        b: gradient_transform.ky,
        c: gradient_transform.kx,
        d: gradient_transform.sy,
        e: gradient_transform.tx,
        f: gradient_transform.ty,
    }
}

impl GlyphPainter<'_> {
    fn clip_with_path(&mut self, path: &str) {
        let clip_id = format!("cp{}", self.clip_path_index);
        self.clip_path_index += 1;

        self.svg.start_element("clipPath");
        self.svg.write_attribute("id", &clip_id);
        self.svg.start_element("path");
        self.svg
            .write_transform_attribute("transform", self.outline_transform);
        self.svg.write_attribute("d", &path);
        self.svg.end_element();
        self.svg.end_element();

        self.svg.start_element("g");
        self.svg
            .write_attribute_fmt("clip-path", format_args!("url(#{})", clip_id));
    }
}

impl<'a> skrifa::color::ColorPainter for GlyphPainter<'a> {
    fn push_transform(&mut self, transform: Transform) {
        self.transforms_stack.push(self.transform);
        self.transform = transform * self.transform;
    }

    fn pop_transform(&mut self) {
        if let Some(ts) = self.transforms_stack.pop() {
            self.transform = ts;
        }
    }

    fn push_clip_glyph(&mut self, glyph_id: skrifa::GlyphId) {
        let mut path_buf = String::new();
        let mut builder = Builder(&mut path_buf);

        match self.font.outline_glyphs().get(glyph_id) {
            Some(outliner) => outliner.draw(DrawSettings::unhinted(size, location), pen),
            None => return,
        };
        builder.finish();

        // We have to write outline using the current transform.
        self.outline_transform = self.transform;
    }

    fn push_clip_box(&mut self, clip_box: skrifa::raw::types::BoundingBox<f32>) {
        let x_min = clip_box.x_min;
        let x_max = clip_box.x_max;
        let y_min = clip_box.y_min;
        let y_max = clip_box.y_max;

        let clip_path = format!(
            "M {} {} L {} {} L {} {} L {} {} Z",
            x_min, y_min, x_max, y_min, x_max, y_max, x_min, y_max
        );

        self.clip_with_path(&clip_path);
    }

    fn pop_clip(&mut self) {
        self.svg.end_element();
    }

    fn fill(&mut self, brush: Brush<'_>) {
        match brush {
            Brush::Solid {
                palette_index,
                alpha,
            } => self.paint_solid(palette_index, alpha),
            Brush::LinearGradient {
                p0,
                p1,
                color_stops,
                extend,
            } => self.paint_linear_gradient(p0, p1, color_stops, extend),
            Brush::RadialGradient {
                c0,
                r0,
                c1,
                r1,
                color_stops,
                extend,
            } => self.paint_radial_gradient(c0, r0, c1, r1, color_stops, extend),
            Brush::SweepGradient {
                c0,
                start_angle,
                end_angle,
                color_stops,
                extend,
            } => self.paint_sweep_gradient(c0, start_angle, end_angle, color_stops, extend),
        }
    }

    fn push_layer(&mut self, composite_mode: skrifa::color::CompositeMode) {
        self.svg.start_element("g");

        use skrifa::color::CompositeMode;
        // TODO: Need to figure out how to represent the other blend modes
        // in SVG.
        let composite_mode = match composite_mode {
            CompositeMode::SrcOver => "normal",
            CompositeMode::Screen => "screen",
            CompositeMode::Overlay => "overlay",
            CompositeMode::Darken => "darken",
            CompositeMode::Lighten => "lighten",
            CompositeMode::ColorDodge => "color-dodge",
            CompositeMode::ColorBurn => "color-burn",
            CompositeMode::HardLight => "hard-light",
            CompositeMode::SoftLight => "soft-light",
            CompositeMode::Difference => "difference",
            CompositeMode::Exclusion => "exclusion",
            CompositeMode::Multiply => "multiply",
            CompositeMode::HslHue => "hue",
            CompositeMode::HslSaturation => "saturation",
            CompositeMode::HslColor => "color",
            CompositeMode::HslLuminosity => "luminosity",
            _ => {
                println!("Warning: unsupported blend mode: {:?}", composite_mode);
                "normal"
            }
        };
        self.svg.write_attribute_fmt(
            "style",
            format_args!("mix-blend-mode: {}; isolation: isolate", composite_mode),
        );
    }
}

impl<'a> ttf_parser::colr::Painter<'a> for GlyphPainter<'a> {
    fn outline_glyph(&mut self, glyph_id: ttf_parser::GlyphId) {
        self.path_buf.clear();
        let mut builder = Builder(self.path_buf);
        match self.face.outline_glyph(glyph_id, &mut builder) {
            Some(v) => v,
            None => return,
        };
        builder.finish();

        // We have to write outline using the current transform.
        self.outline_transform = self.transform;
    }

    fn push_layer(&mut self, mode: ttf_parser::colr::CompositeMode) {
        self.svg.start_element("g");

        use ttf_parser::colr::CompositeMode;
        // TODO: Need to figure out how to represent the other blend modes
        // in SVG.
        let mode = match mode {
            CompositeMode::SourceOver => "normal",
            CompositeMode::Screen => "screen",
            CompositeMode::Overlay => "overlay",
            CompositeMode::Darken => "darken",
            CompositeMode::Lighten => "lighten",
            CompositeMode::ColorDodge => "color-dodge",
            CompositeMode::ColorBurn => "color-burn",
            CompositeMode::HardLight => "hard-light",
            CompositeMode::SoftLight => "soft-light",
            CompositeMode::Difference => "difference",
            CompositeMode::Exclusion => "exclusion",
            CompositeMode::Multiply => "multiply",
            CompositeMode::Hue => "hue",
            CompositeMode::Saturation => "saturation",
            CompositeMode::Color => "color",
            CompositeMode::Luminosity => "luminosity",
            _ => {
                println!("Warning: unsupported blend mode: {:?}", mode);
                "normal"
            }
        };
        self.svg.write_attribute_fmt(
            "style",
            format_args!("mix-blend-mode: {}; isolation: isolate", mode),
        );
    }

    fn pop_layer(&mut self) {
        self.svg.end_element(); // g
    }

    fn push_transform(&mut self, transform: ttf_parser::Transform) {
        self.transforms_stack.push(self.transform);
        self.transform = ttf_parser::Transform::combine(self.transform, transform);
    }

    fn paint(&mut self, paint: ttf_parser::colr::Paint<'a>) {
        match paint {
            ttf_parser::colr::Paint::Solid(color) => self.paint_solid(color),
            ttf_parser::colr::Paint::LinearGradient(lg) => self.paint_linear_gradient(lg),
            ttf_parser::colr::Paint::RadialGradient(rg) => self.paint_radial_gradient(rg),
            ttf_parser::colr::Paint::SweepGradient(sg) => self.paint_sweep_gradient(sg),
        }
    }

    fn pop_transform(&mut self) {
        if let Some(ts) = self.transforms_stack.pop() {
            self.transform = ts;
        }
    }

    fn push_clip(&mut self) {
        self.clip_with_path(&self.path_buf.clone());
    }

    fn pop_clip(&mut self) {
        self.svg.end_element();
    }

    fn push_clip_box(&mut self, clipbox: ttf_parser::colr::ClipBox) {
        let x_min = clipbox.x_min;
        let x_max = clipbox.x_max;
        let y_min = clipbox.y_min;
        let y_max = clipbox.y_max;

        let clip_path = format!(
            "M {} {} L {} {} L {} {} L {} {} Z",
            x_min, y_min, x_max, y_min, x_max, y_max, x_min, y_max
        );

        self.clip_with_path(&clip_path);
    }
}
