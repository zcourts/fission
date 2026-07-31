use crate::{Color, Fill, LineCap, LineJoin, Stroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineSvgFillRule {
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineSvgPaintOrder {
    FillAndStroke,
    StrokeAndFill,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InlineSvgPathSegment {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlineSvgPath {
    pub segments: Vec<InlineSvgPathSegment>,
    pub transform: [f32; 6],
    pub fill: Option<Fill>,
    pub fill_rule: InlineSvgFillRule,
    pub stroke: Option<Stroke>,
    pub paint_order: InlineSvgPaintOrder,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlineSvg {
    pub width: f32,
    pub height: f32,
    pub paths: Vec<InlineSvgPath>,
}

pub fn parse_inline_svg(content: &str) -> Option<InlineSvg> {
    let tree = usvg::Tree::from_str(content, &usvg::Options::default()).ok()?;
    let width = tree.size().width();
    let height = tree.size().height();
    let mut paths = Vec::new();
    collect_group_paths(tree.root(), 1.0, width, height, &mut paths);
    Some(InlineSvg {
        width,
        height,
        paths,
    })
}

fn collect_group_paths(
    group: &usvg::Group,
    parent_opacity: f32,
    document_width: f32,
    document_height: f32,
    paths: &mut Vec<InlineSvgPath>,
) {
    let opacity = parent_opacity * group.opacity().get();
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => {
                collect_group_paths(group, opacity, document_width, document_height, paths);
            }
            usvg::Node::Path(path) if path.is_visible() => {
                let fill = path.fill().and_then(|fill| {
                    paint_to_fill(
                        fill.paint(),
                        opacity * fill.opacity().get(),
                        document_width,
                        document_height,
                    )
                });
                let stroke = path.stroke().and_then(|stroke| {
                    paint_to_stroke(stroke, opacity, document_width, document_height)
                });

                // usvg keeps geometry-only placeholders in the tree. They must
                // remain non-painting even when a whole-SVG icon override is
                // applied, otherwise common fill="none" view-box paths become
                // visible rectangles.
                if path.fill().is_none() && path.stroke().is_none() {
                    continue;
                }

                let segments = path
                    .data()
                    .segments()
                    .map(|segment| match segment {
                        usvg::tiny_skia_path::PathSegment::MoveTo(point) => {
                            InlineSvgPathSegment::MoveTo(point.x, point.y)
                        }
                        usvg::tiny_skia_path::PathSegment::LineTo(point) => {
                            InlineSvgPathSegment::LineTo(point.x, point.y)
                        }
                        usvg::tiny_skia_path::PathSegment::QuadTo(control, point) => {
                            InlineSvgPathSegment::QuadTo(control.x, control.y, point.x, point.y)
                        }
                        usvg::tiny_skia_path::PathSegment::CubicTo(
                            control_one,
                            control_two,
                            point,
                        ) => InlineSvgPathSegment::CubicTo(
                            control_one.x,
                            control_one.y,
                            control_two.x,
                            control_two.y,
                            point.x,
                            point.y,
                        ),
                        usvg::tiny_skia_path::PathSegment::Close => InlineSvgPathSegment::Close,
                    })
                    .collect();
                let transform = path.abs_transform();
                paths.push(InlineSvgPath {
                    segments,
                    transform: [
                        transform.sx,
                        transform.ky,
                        transform.kx,
                        transform.sy,
                        transform.tx,
                        transform.ty,
                    ],
                    fill,
                    fill_rule: match path.fill().map(usvg::Fill::rule) {
                        Some(usvg::FillRule::EvenOdd) => InlineSvgFillRule::EvenOdd,
                        _ => InlineSvgFillRule::NonZero,
                    },
                    stroke,
                    paint_order: match path.paint_order() {
                        usvg::PaintOrder::FillAndStroke => InlineSvgPaintOrder::FillAndStroke,
                        usvg::PaintOrder::StrokeAndFill => InlineSvgPaintOrder::StrokeAndFill,
                    },
                });
            }
            usvg::Node::Path(_) | usvg::Node::Image(_) | usvg::Node::Text(_) => {}
        }
    }
}

fn paint_to_fill(
    paint: &usvg::Paint,
    opacity: f32,
    document_width: f32,
    document_height: f32,
) -> Option<Fill> {
    match paint {
        usvg::Paint::Color(color) => Some(Fill::Solid(render_color(*color, opacity))),
        usvg::Paint::LinearGradient(gradient) => Some(Fill::LinearGradient {
            start: (
                normalized(gradient.x1(), document_width),
                normalized(gradient.y1(), document_height),
            ),
            end: (
                normalized(gradient.x2(), document_width),
                normalized(gradient.y2(), document_height),
            ),
            stops: gradient
                .stops()
                .iter()
                .map(|stop| {
                    (
                        stop.offset().get(),
                        render_color(stop.color(), opacity * stop.opacity().get()),
                    )
                })
                .collect(),
        }),
        usvg::Paint::RadialGradient(gradient) => Some(Fill::RadialGradient {
            center: (
                normalized(gradient.cx(), document_width),
                normalized(gradient.cy(), document_height),
            ),
            radius: normalized(gradient.r().get(), document_width.max(document_height)),
            stops: gradient
                .stops()
                .iter()
                .map(|stop| {
                    (
                        stop.offset().get(),
                        render_color(stop.color(), opacity * stop.opacity().get()),
                    )
                })
                .collect(),
        }),
        usvg::Paint::Pattern(_) => None,
    }
}

fn paint_to_stroke(
    stroke: &usvg::Stroke,
    group_opacity: f32,
    document_width: f32,
    document_height: f32,
) -> Option<Stroke> {
    Some(Stroke {
        fill: paint_to_fill(
            stroke.paint(),
            group_opacity * stroke.opacity().get(),
            document_width,
            document_height,
        )?,
        width: stroke.width().get(),
        dash_array: stroke.dasharray().map(<[f32]>::to_vec),
        line_cap: match stroke.linecap() {
            usvg::LineCap::Butt => LineCap::Butt,
            usvg::LineCap::Round => LineCap::Round,
            usvg::LineCap::Square => LineCap::Square,
        },
        line_join: match stroke.linejoin() {
            usvg::LineJoin::Round => LineJoin::Round,
            usvg::LineJoin::Bevel => LineJoin::Bevel,
            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => LineJoin::Miter,
        },
    })
}

fn render_color(color: usvg::Color, opacity: f32) -> Color {
    Color {
        r: color.red,
        g: color.green,
        b: color.blue,
        a: (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
    }
}

fn normalized(value: f32, extent: f32) -> f32 {
    if extent.is_finite() && extent > 0.0 {
        value / extent
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_inline_svg, InlineSvgFillRule, InlineSvgPaintOrder};
    use crate::{Color, Fill};

    #[test]
    fn preserves_authored_solid_paint_and_group_inheritance() {
        let svg = parse_inline_svg(
            r##"<svg viewBox="0 0 20 10">
                <g fill="#12a150">
                    <rect width="10" height="10"/>
                    <rect x="10" width="10" height="10" fill="#2563eb"/>
                </g>
            </svg>"##,
        )
        .expect("parse inline SVG");

        assert_eq!(svg.paths.len(), 2);
        assert_eq!(
            svg.paths[0].fill,
            Some(Fill::Solid(Color {
                r: 0x12,
                g: 0xa1,
                b: 0x50,
                a: 0xff,
            }))
        );
        assert_eq!(
            svg.paths[1].fill,
            Some(Fill::Solid(Color {
                r: 0x25,
                g: 0x63,
                b: 0xeb,
                a: 0xff,
            }))
        );
        assert_eq!(svg.paths[0].fill_rule, InlineSvgFillRule::NonZero);
        assert_eq!(svg.paths[0].paint_order, InlineSvgPaintOrder::FillAndStroke);
    }

    #[test]
    fn drops_non_painting_view_box_placeholders() {
        let svg = parse_inline_svg(
            r#"<svg viewBox="0 0 24 24">
                <rect fill="none" width="24" height="24"/>
                <path d="M0 0h10v10H0z"/>
            </svg>"#,
        )
        .expect("parse inline SVG");

        assert_eq!(svg.paths.len(), 1);
    }
}
