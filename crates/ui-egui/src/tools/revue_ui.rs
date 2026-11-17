//! Revue PDF K&D: a drawing board for plans on a large touch display. Brushes that behave like
//! an architect's pens, the drawing scale read from the sheet, a scale ruler laid on the page to
//! measure and draw along, smart fill (tap inside a room for its area) and markup layers, like
//! sheets of tracing paper over the plan.
//!
//! The behaviour follows public descriptions of drawing-board apps; the look, names and brushes
//! are PdfCraft's own.
use egui::{Color32, Pos2, Stroke, vec2};
use pdfcraft_engine::{
    DEFAULT_LAYER, Document, Edit, Shape, SmartFill, Style,
    measure::{self, Kind, NewMeasurement, PageUnit, Scale, region::FillOptions, ruler, scale_text::FoundScale},
};

use crate::{LeftPanel, PdfCraftApp, QuickTool, comments::CommentTool, measure_ui, theme, theme::Tokens, widgets};

/// A pen of the brush box: a comment tool with its colour, width (points) and opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Brush {
    pub command: &'static str,
    pub name: &'static str,
    pub tool: CommentTool,
    pub color: [f64; 3],
    pub width: f64,
    pub opacity: f64,
}

/// The brush box. Widths are in points on the sheet (0.35 mm ≈ 1 pt).
pub(crate) const BRUSHES: [Brush; 6] = [
    Brush { command: "revue.pen", name: "Technical pen", tool: CommentTool::Ink, color: [0.08, 0.08, 0.1], width: 0.7, opacity: 1.0 },
    Brush { command: "revue.liner", name: "Fine liner", tool: CommentTool::Ink, color: [0.05, 0.2, 0.55], width: 1.4, opacity: 1.0 },
    Brush { command: "revue.marker", name: "Marker", tool: CommentTool::Ink, color: [0.8, 0.12, 0.12], width: 4.0, opacity: 0.85 },
    Brush { command: "revue.pencil", name: "Graphite pencil", tool: CommentTool::Ink, color: [0.32, 0.32, 0.34], width: 0.9, opacity: 0.7 },
    Brush { command: "revue.wash", name: "Colour wash", tool: CommentTool::Ink, color: [0.2, 0.55, 0.85], width: 14.0, opacity: 0.25 },
    Brush {
        command: "revue.highlighter",
        name: "Highlighter pen",
        tool: CommentTool::FreehandHighlight,
        color: [1.0, 0.86, 0.1],
        width: 12.0,
        opacity: 0.4,
    },
];

/// Drawing scales offered as one-tap buttons (the N of 1:N).
const QUICK_RATIOS: [f64; 6] = [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
/// How far (view points) a stroke may stray from the ruler's edge and still be drawn along it.
const STRAIGHTEN_REACH: f64 = 14.0;
/// The ruler's depth across, in view points (about 2 cm on the sheet).
const RULER_DEPTH: f64 = 56.0;
/// The share of a stroke's points that must lie along the ruler for the stroke to be straightened.
const STRAIGHTEN_SHARE: f64 = 0.8;

type FillReceiver = std::sync::mpsc::Receiver<Result<SmartFill, String>>;

/// A scale ruler on one page, in view points (y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ruler {
    pub page: usize,
    /// Its zero, on its measuring edge.
    pub origin: [f64; 2],
    /// Radians, clockwise on screen from the page's x axis.
    pub angle: f64,
    pub length: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RulerDrag {
    /// Moving the ruler, held this far (view points) from its zero.
    Move([f64; 2]),
    /// Turning it about its zero.
    Turn,
}

/// Revue's state for one document tab.
pub(crate) struct RevueView {
    /// The layer new markups go on. The default one exists in the file once something is drawn
    /// on it.
    pub active_layer: String,
    /// New markups go on [`Self::active_layer`].
    pub on_layers: bool,
    pub ruler: Option<Ruler>,
    /// Strokes drawn along the ruler come out straight.
    pub straighten: bool,
    drag: Option<RulerDrag>,
    pub fill_options: FillOptions,
    /// A smart fill being read on a thread: its page.
    fill: Option<(usize, FillReceiver)>,
    /// Scales read from a page's text, per (page, edit generation).
    scales: Option<(usize, u64, Vec<FoundScale>)>,
    pub new_layer: String,
    pub error: Option<String>,
    pub notice: Option<String>,
}

impl Default for RevueView {
    fn default() -> Self {
        Self {
            active_layer: DEFAULT_LAYER.into(),
            on_layers: true,
            ruler: None,
            straighten: true,
            drag: None,
            fill_options: FillOptions::default(),
            fill: None,
            scales: None,
            new_layer: String::new(),
            error: None,
            notice: None,
        }
    }
}

/// Pick `brush`: its tool becomes the current one, drawing with its style.
pub(crate) fn pick_brush(app: &mut PdfCraftApp, brush: &Brush) {
    let mut style = app.comment_prefs.style(brush.tool);
    style.color = brush.color;
    style.opacity = brush.opacity;
    app.comment_prefs.set_style(brush.tool, style);
    app.comment_prefs.set_width(brush.tool, brush.width);
    app.comment_prefs.set_opacity(brush.tool, brush.opacity);
    app.quick_tool = QuickTool::Comment(brush.tool);
}

/// The brush whose style the tool now draws with, if any.
fn current_brush(app: &PdfCraftApp) -> Option<&'static Brush> {
    let QuickTool::Comment(tool) = app.quick_tool else { return None };
    let style = app.comment_prefs.style(tool);
    BRUSHES.iter().find(|b| b.tool == tool && b.color == style.color && (b.width - style.width).abs() < 1e-6)
}

/// Whether the Revue command `id` is the tool in use, for its toolbar button.
pub(crate) fn is_active(app: &PdfCraftApp, id: &str) -> bool {
    match id {
        "revue.eraser" => app.quick_tool == QuickTool::Comment(CommentTool::Eraser),
        "revue.ruler" => app.quick_tool == QuickTool::Measure(measure_ui::Tool::Ruler),
        "revue.fill" => app.quick_tool == QuickTool::Measure(measure_ui::Tool::Fill),
        _ => current_brush(app).is_some_and(|b| b.command == id),
    }
}

/// The Revue commands.
pub(crate) fn command(app: &mut PdfCraftApp, id: &str) {
    if let Some(brush) = BRUSHES.iter().find(|b| b.command == id) {
        pick_brush(app, brush);
        return;
    }
    let open_panel = |app: &mut PdfCraftApp| {
        app.left = LeftPanel::Tool("revue");
        app.left_open = true;
    };
    match id {
        "revue.eraser" => app.quick_tool = QuickTool::Comment(CommentTool::Eraser),
        "revue.ruler" => {
            if let Some((index, _)) = app.active_ids() {
                let page = app.views[index].current;
                toggle_ruler(&mut app.views[index].revue, page);
                app.quick_tool = QuickTool::Measure(measure_ui::Tool::Ruler);
            }
        }
        "revue.fill" => {
            open_panel(app);
            if let Some((index, _)) = app.active_ids() {
                app.views[index].measure.cancel();
            }
            app.quick_tool = QuickTool::Measure(measure_ui::Tool::Fill);
        }
        "revue.read_scale" => {
            open_panel(app);
            read_scales(app);
        }
        "revue.layers" => {
            app.right = Some(crate::RightPanel::Layers);
        }
        "revue.layer_add" => {
            if let Some((index, id)) = app.active_ids() {
                let n = app.session.get(id).map_or(0, |d| d.info.layers.len());
                let name = next_layer_name(app.session.get(id), n);
                add_layer(app, index, &name);
            }
        }
        _ => open_panel(app),
    }
}

/// A layer name the document doesn't have: "Layer 2", "Layer 3"…
fn next_layer_name(doc: Option<&Document>, count: usize) -> String {
    let taken = |name: &str| doc.is_some_and(|d| d.info.layers.iter().any(|l| l.name == name)) || name == DEFAULT_LAYER && count == 0;
    (count.saturating_add(1)..count.saturating_add(10_000)).map(|n| format!("Layer {n}")).find(|n| !taken(n)).unwrap_or_else(|| "Layer".into())
}

/// Add a layer named `name` to tab `index`'s document and draw on it from now on.
fn add_layer(app: &mut PdfCraftApp, index: usize, name: &str) {
    let name = name.trim().to_string();
    if name.is_empty() {
        return;
    }
    // The default layer isn't in the file until something is drawn on it: add it first, so the
    // new one is listed after it, as the panel shows them.
    let missing_default = app.active_ids().and_then(|(_, id)| app.session.get(id)).is_some_and(|d| d.info.layers.is_empty()) && name != DEFAULT_LAYER;
    let edit = if missing_default {
        Edit::Batch { label: "Add layer".into(), edits: vec![Edit::AddLayer { name: DEFAULT_LAYER.into() }, Edit::AddLayer { name: name.clone() }] }
    } else {
        Edit::AddLayer { name: name.clone() }
    };
    if app.apply_edit(edit)
        && let Some(view) = app.views.get_mut(index)
    {
        view.revue.active_layer = name;
        view.revue.on_layers = true;
        view.revue.new_layer.clear();
    }
}

fn toggle_ruler(state: &mut RevueView, page: usize) {
    state.ruler = match state.ruler {
        Some(r) if r.page == page => None,
        _ => Some(Ruler { page, origin: [72.0, 144.0], angle: 0.0, length: 360.0 }),
    };
    state.drag = None;
}

/// Read the scales written on the current page into the panel.
fn read_scales(app: &mut PdfCraftApp) {
    let Some((index, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let page = app.views[index].current;
    let found = doc.page_scales(page);
    let state = &mut app.views[index].revue;
    state.error = found.is_empty().then(|| tl!("No drawing scale is written on this page. Pick one or calibrate from two points.").to_string());
    state.scales = Some((page, doc.edit_generation(), found));
}

/// A whole-page drawing scale of 1:N, measuring in metres.
fn ratio_edit(doc: &Document, page: usize, n: f64) -> Result<Edit, String> {
    let info = doc.info.pages.get(page).ok_or("no such page")?;
    let a = doc.measurement_to_user(page, [0.0, 0.0])?;
    let b = doc.measurement_to_user(page, [1.0, 0.0])?;
    // 1 mm on the sheet is N mm, which is N / 1000 m.
    let scale = Scale::ratio(a, b, 1.0, PageUnit::Mm, n / 1000.0, "m", 2).map_err(|e| e.to_string())?;
    let p = info.view_to_user(0.0, 0.0);
    let q = info.view_to_user(info.width, info.height);
    Ok(Edit::SetMeasurementScale {
        page,
        bbox: [f64::from(p[0].min(q[0])), f64::from(p[1].min(q[1])), f64::from(p[0].max(q[0])), f64::from(p[1].max(q[1]))],
        name: measure_ui::format_ratio(n),
        scale,
    })
}

fn apply_ratio(app: &mut PdfCraftApp, n: f64) {
    let Some((index, id)) = app.active_ids() else { return };
    let page = app.views[index].current;
    let edit = app.session.get(id).map(|doc| ratio_edit(doc, page, n));
    match edit {
        Some(Ok(edit)) => {
            if app.apply_edit(edit) {
                let state = &mut app.views[index].revue;
                state.error = None;
                state.notice = Some(crate::i18n::fmt(tl!("This page now measures at {scale}."), &[("scale", &measure_ui::format_ratio(n))]));
                // The measure panel reads the page's scale again.
                app.views[index].measure = Default::default();
            }
        }
        Some(Err(e)) => app.views[index].revue.error = Some(e),
        None => {}
    }
}

/// What a new markup becomes when Revue adds it: strokes drawn along the ruler straightened, then
/// put on the active layer.
pub(crate) fn prepare(state: &RevueView, doc: &Document, edit: Edit) -> Edit {
    let edit = straighten(state, doc, edit);
    if !state.on_layers || !drawable(&edit) || !doc.allows_annotation() {
        return edit;
    }
    doc.on_layer(&state.active_layer, edit)
}

/// The edits a layer can take: markups, not signatures, stamps, attachments or redactions.
fn drawable(edit: &Edit) -> bool {
    match edit {
        Edit::AddMeasurement(_) => true,
        Edit::AddAnnotation(a) => !matches!(
            a.shape,
            Shape::Signature { .. }
                | Shape::TypedSignature { .. }
                | Shape::Stamp { .. }
                | Shape::CustomStamp { .. }
                | Shape::Attachment { .. }
                | Shape::Redact { .. }
        ),
        _ => false,
    }
}

/// An ink stroke drawn along the ruler, laid on its edge.
fn straighten(state: &RevueView, doc: &Document, edit: Edit) -> Edit {
    let (Some(r), true) = (state.ruler, state.straighten) else { return edit };
    let Edit::AddAnnotation(mut a) = edit else { return edit };
    if a.page != r.page {
        return Edit::AddAnnotation(a);
    }
    let Shape::Ink { strokes } = &mut a.shape else { return Edit::AddAnnotation(a) };
    for stroke in strokes.iter_mut() {
        let view: Option<Vec<[f64; 2]>> = stroke.iter().map(|p| doc.measurement_to_view(a.page, *p).ok()).collect();
        let Some(view) = view else { continue };
        if let Some(straight) = straightened(&view, &r) {
            let user: Option<Vec<[f64; 2]>> = straight.iter().map(|p| doc.measurement_to_user(a.page, *p).ok()).collect();
            if let Some(user) = user {
                *stroke = user;
            }
        }
    }
    Edit::AddAnnotation(a)
}

/// A stroke (view points) whose points lie mostly along `r`'s edge, as the straight line between
/// its extremes on that edge; `None` for one drawn elsewhere.
fn straightened(stroke: &[[f64; 2]], r: &Ruler) -> Option<Vec<[f64; 2]>> {
    if stroke.len() < 2 {
        return None;
    }
    let near = stroke.iter().filter(|p| ruler::distance_to_edge(**p, r.origin, r.angle) <= STRAIGHTEN_REACH).count();
    if (near as f64) < STRAIGHTEN_SHARE * stroke.len() as f64 {
        return None;
    }
    let (c, s) = (r.angle.cos(), r.angle.sin());
    let along = |p: &[f64; 2]| (p[0] - r.origin[0]) * c + (p[1] - r.origin[1]) * s;
    let first = stroke.first()?;
    let last = stroke.last()?;
    let (t0, t1) = (along(first), along(last));
    if (t1 - t0).abs() < 1.0 {
        return None;
    }
    Some(vec![[r.origin[0] + t0 * c, r.origin[1] + t0 * s], [r.origin[0] + t1 * c, r.origin[1] + t1 * s]])
}

/// Real-world units per view point along the ruler on `page`, and their unit.
fn real_per_point(doc: &Document, page: usize, r: &Ruler, units: crate::units::Unit) -> Option<(f64, String)> {
    let a = doc.measurement_to_user(page, r.origin).ok()?;
    let b = doc.measurement_to_user(page, [r.origin[0] + r.angle.cos(), r.origin[1] + r.angle.sin()]).ok()?;
    let scale = doc.measurement_scale_or(page, a, units.per_point(), units.label()).ok()?;
    let reading = measure::reading(Kind::Distance, &[a, b], &scale).ok()?;
    (reading.value.is_finite() && reading.value > 0.0).then_some((reading.value, scale.unit))
}

/// Smart fill and the ruler on page `cx.page`, for their tools: a tap fills, a press on the ruler
/// moves it, on its end handle turns it, elsewhere lays it there.
pub(crate) fn page_input(
    ui: &egui::Ui,
    clicks: measure_ui::Clicks,
    doc: &Document,
    view: &mut crate::canvas::DocView,
    cx: &crate::comments::PageCx<'_>,
    tool: measure_ui::Tool,
) {
    let page = cx.page;
    let xf = cx.xf;
    let Some(pos) = cx.ptr.within(xf.rect) else {
        if !cx.ptr.primary_down {
            view.revue.drag = None;
        }
        return;
    };
    let (vx, vy) = xf.screen_to_view(pos);
    let at = [f64::from(vx), f64::from(vy)];
    match tool {
        measure_ui::Tool::Fill => {
            view.cursor.claim(crate::page_cursor::Layer::ToolArea, egui::CursorIcon::Crosshair);
            if clicks.single && view.revue.fill.is_none() && doc.allows_annotation() {
                start_fill(ui.ctx(), &mut view.revue, doc, page, at);
            }
        }
        measure_ui::Tool::Ruler => {
            view.cursor.claim(crate::page_cursor::Layer::ToolArea, egui::CursorIcon::Grab);
            let state = &mut view.revue;
            if clicks.free && cx.ptr.pressed().is_some() {
                state.drag = match state.ruler {
                    Some(r) if r.page == page && near_handle(&r, at, xf.zoom()) => Some(RulerDrag::Turn),
                    Some(r) if r.page == page && on_body(&r, at) => Some(RulerDrag::Move([at[0] - r.origin[0], at[1] - r.origin[1]])),
                    _ => {
                        let length = state.ruler.map_or(360.0, |r| r.length);
                        let angle = state.ruler.map_or(0.0, |r| r.angle);
                        state.ruler = Some(Ruler { page, origin: at, angle, length });
                        Some(RulerDrag::Move([0.0, 0.0]))
                    }
                };
            }
            if cx.ptr.primary_down
                && let (Some(drag), Some(r)) = (state.drag, state.ruler.as_mut())
                && r.page == page
            {
                match drag {
                    RulerDrag::Move(held) => r.origin = [at[0] - held[0], at[1] - held[1]],
                    RulerDrag::Turn => {
                        let (dx, dy) = (at[0] - r.origin[0], at[1] - r.origin[1]);
                        if dx.hypot(dy) > 4.0 {
                            r.angle = snap_angle(dy.atan2(dx), cx.ptr.modifiers.shift);
                        }
                    }
                }
            } else if !cx.ptr.primary_down {
                state.drag = None;
            }
        }
        _ => {}
    }
}

/// An angle on the nearest 15° when it is within 2° of it, or always with `snap`.
fn snap_angle(angle: f64, snap: bool) -> f64 {
    let step = 15f64.to_radians();
    let nearest = (angle / step).round() * step;
    if snap || (angle - nearest).abs() < 2f64.to_radians() { nearest } else { angle }
}

fn local(r: &Ruler, at: [f64; 2]) -> (f64, f64) {
    let (c, s) = (r.angle.cos(), r.angle.sin());
    let (dx, dy) = (at[0] - r.origin[0], at[1] - r.origin[1]);
    (dx * c + dy * s, -dx * s + dy * c)
}

/// The ruler's body lies on the side below its edge (positive across).
fn on_body(r: &Ruler, at: [f64; 2]) -> bool {
    let (along, across) = local(r, at);
    (-8.0..=r.length + 8.0).contains(&along) && (-8.0..=RULER_DEPTH).contains(&across)
}

fn handle(r: &Ruler) -> [f64; 2] {
    [r.origin[0] + r.length * r.angle.cos(), r.origin[1] + r.length * r.angle.sin()]
}

/// Within a finger's reach (about 24 pixels) of the turning handle at the ruler's far end.
fn near_handle(r: &Ruler, at: [f64; 2], zoom: f32) -> bool {
    let h = handle(r);
    let reach = 24.0 / f64::from(zoom.max(0.05));
    (at[0] - h[0]).hypot(at[1] - h[1]) <= reach
}

#[cfg(not(target_arch = "wasm32"))]
fn start_fill(ctx: &egui::Context, state: &mut RevueView, doc: &Document, page: usize, at: [f64; 2]) {
    let job = doc.smart_fill_job(page, at, state.fill_options);
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("smart-fill".into()).spawn(move || {
        // The renderer runs behind its own panic guard; an escaped panic is an error message.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).unwrap_or_else(|_| Err("smart fill stopped unexpectedly".into()));
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    match spawned {
        Ok(_) => {
            state.fill = Some((page, rx));
            state.error = None;
        }
        Err(e) => state.error = Some(e.to_string()),
    }
}

#[cfg(target_arch = "wasm32")]
fn start_fill(_ctx: &egui::Context, state: &mut RevueView, doc: &Document, page: usize, at: [f64; 2]) {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = tx.send(doc.smart_fill(page, at, state.fill_options));
    state.fill = Some((page, rx));
}

/// A finished smart fill becomes an area measurement.
pub(crate) fn poll(ctx: &egui::Context, view: &mut crate::canvas::DocView, doc: &Document, author: &str, units: crate::units::Unit) {
    let Some((page, rx)) = view.revue.fill.as_ref() else { return };
    let page = *page;
    let result = match rx.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return;
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("smart fill stopped unexpectedly".into()),
    };
    view.revue.fill = None;
    let fill = match result {
        Ok(fill) => fill,
        Err(e) => {
            view.revue.error = Some(e);
            return;
        }
    };
    let Some(first) = fill.points.first().copied() else { return };
    let scale = match doc.measurement_scale_or(page, first, units.per_point(), units.label()) {
        Ok(scale) => scale,
        Err(e) => {
            view.revue.error = Some(e);
            return;
        }
    };
    if let Ok(reading) = measure::reading(Kind::Area, &fill.points, &scale) {
        view.revue.notice = Some(reading.label.clone());
        view.measure.reading = Some(reading);
    }
    let blue = [0.0, 0.47, 0.84];
    view.pending_edit = Some(Edit::AddMeasurement(NewMeasurement {
        page,
        kind: Kind::Area,
        points: fill.points,
        scale,
        style: Style { color: blue, fill: Some(blue), fill_opacity: Some(0.18), width: 1.0, ..Style::default() },
        label: String::new(),
        author: author.into(),
    }));
}

/// Draw the ruler on page `page` when it lies there, graduated at the page's scale.
pub(crate) fn paint_ruler(
    ui: &egui::Ui,
    view: &crate::canvas::DocView,
    doc: &Document,
    page: usize,
    xf: &crate::canvas::PageXform,
    units: crate::units::Unit,
) {
    let Some(r) = view.revue.ruler.filter(|r| r.page == page) else { return };
    let screen = |p: [f64; 2]| xf.norm_to_screen(p[0] as f32 / xf.pw.max(1e-3), p[1] as f32 / xf.ph.max(1e-3));
    let (c, s) = (r.angle.cos(), r.angle.sin());
    let along = |t: f64, across: f64| [r.origin[0] + t * c - across * s, r.origin[1] + t * s + across * c];
    let body = [along(0.0, 0.0), along(r.length, 0.0), along(r.length, RULER_DEPTH), along(0.0, RULER_DEPTH)].map(screen);
    let painter = ui.painter_at(xf.rect);
    let edge = Color32::from_rgb(20, 70, 140);
    painter.add(egui::Shape::convex_polygon(body.to_vec(), Color32::from_rgba_unmultiplied(235, 244, 255, 200), Stroke::new(1.0, edge)));
    painter.line_segment([body[0], body[1]], Stroke::new(2.0, edge));
    let px_per_point = f64::from(screen(along(0.0, 0.0)).distance(screen(along(1.0, 0.0))));
    if let Some((real, unit)) = real_per_point(doc, page, &r, units) {
        let (_, ticks) = ruler::ticks(r.length, real, px_per_point, 7.0);
        for tick in &ticks {
            let depth = match tick.kind {
                ruler::TickKind::Major => 16.0,
                ruler::TickKind::Half => 11.0,
                ruler::TickKind::Minor => 6.0,
            } / px_per_point.max(1e-3);
            painter.line_segment([screen(along(tick.at, 0.0)), screen(along(tick.at, depth))], Stroke::new(1.0, edge));
            if tick.kind == ruler::TickKind::Major {
                let label = trim_number(tick.value);
                painter.text(screen(along(tick.at, depth + 3.0 / px_per_point)), egui::Align2::CENTER_TOP, label, theme::regular(11.0), edge);
            }
        }
        painter.text(
            screen(along(r.length * 0.5, RULER_DEPTH * 0.72)),
            egui::Align2::CENTER_CENTER,
            format!("{}   {:.1}°", unit, (-r.angle.to_degrees()).rem_euclid(360.0)),
            theme::semibold(12.0),
            edge,
        );
    }
    let h = screen(handle(&r));
    painter.circle(h, 10.0, Color32::from_rgb(20, 70, 140), Stroke::new(2.0, Color32::WHITE));
    painter.circle_filled(screen(r.origin), 4.0, edge);
}

/// "2", "0.5", "12.25": a graduation's value without trailing zeros.
fn trim_number(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.into() }
}

/// A big touch button: at least 48 pixels tall, the full width it is given.
fn big_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let button =
        egui::Button::new(egui::RichText::new(label).font(theme::medium(15.0))).selected(selected).min_size(vec2(ui.available_width(), 48.0));
    ui.add(button)
}

fn swatch(ui: &egui::Ui, rect: egui::Rect, brush: &Brush) {
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let color = Color32::from_rgba_unmultiplied(b(brush.color[0]), b(brush.color[1]), b(brush.color[2]), b(brush.opacity.max(0.3)));
    let y = rect.center().y;
    let width = (brush.width as f32 * 1.6).clamp(1.5, 16.0);
    ui.painter().line_segment([Pos2::new(rect.left() + 12.0, y), Pos2::new(rect.left() + 52.0, y)], Stroke::new(width, color));
}

/// The Revue panel.
pub(crate) fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    egui::ScrollArea::vertical().id_salt("revue-panel").show(ui, |ui| panel_body(app, ui, t));
}

fn heading(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(10.0);
    ui.label(egui::RichText::new(text).font(theme::semibold(14.0)).color(t.text));
    ui.add_space(4.0);
}

fn panel_body(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    widgets::begin_dialog(ui);
    let Some((index, id)) = app.active_ids() else {
        ui.label(tl!("Open a PDF to draw on it."));
        return;
    };
    // Brushes.
    heading(ui, t, tl!("Brushes"));
    let current = current_brush(app).map(|b| b.command);
    let mut picked = None;
    for brush in &BRUSHES {
        let resp = big_button(ui, &format!("            {}", tl!(brush.name)), current == Some(brush.command));
        swatch(ui, resp.rect, brush);
        if resp.clicked() {
            picked = Some(brush);
        }
    }
    if let Some(brush) = picked {
        pick_brush(app, brush);
    }
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 8.0) / 3.0;
        for (label, tool) in [("Eraser", QuickTool::Comment(CommentTool::Eraser)), ("Select", QuickTool::Select), ("Pan", QuickTool::Hand)] {
            let b =
                egui::Button::new(egui::RichText::new(tl!(label)).font(theme::medium(14.0))).selected(app.quick_tool == tool).min_size(vec2(w, 44.0));
            if ui.add(b).clicked() {
                app.quick_tool = tool;
            }
        }
    });
    if let QuickTool::Comment(tool) = app.quick_tool
        && matches!(tool, CommentTool::Ink | CommentTool::FreehandHighlight)
    {
        let mut width = app.comment_prefs.style(tool).width;
        if ui.add(egui::Slider::new(&mut width, 0.25..=24.0).logarithmic(true).text(tl!("Width (pt)"))).changed() {
            app.comment_prefs.set_width(tool, width);
        }
        let mut opacity = app.comment_prefs.style(tool).opacity * 100.0;
        if ui.add(egui::Slider::new(&mut opacity, 10.0..=100.0).text(tl!("Opacity %"))).changed() {
            app.comment_prefs.set_opacity(tool, opacity / 100.0);
        }
    }

    // Draw to scale.
    heading(ui, t, tl!("Drawing scale"));
    let Some(doc) = app.session.get(id) else { return };
    let page = app.views[index].current;
    let generation = doc.edit_generation();
    let current_scale = doc
        .measurement_to_user(page, [0.0, 0.0])
        .ok()
        .and_then(|a| doc.measurement_scale(page, a).ok())
        .map(|s| s.ratio)
        .unwrap_or_else(|| tl!("Not set: true size").to_string());
    ui.label(egui::RichText::new(format!("{} {}: {current_scale}", tl!("Page"), page + 1)).color(t.text_faint));
    let mut ratio = None;
    if big_button(ui, tl!("Read scale from page"), false).clicked() {
        read_scales(app);
    }
    let Some(doc) = app.session.get(id) else { return };
    if let Some((p, g, found)) = &app.views[index].revue.scales
        && *p == page
        && *g == generation
    {
        for f in found {
            let label = format!("{}   ({})", measure_ui::format_ratio(f.ratio), f.text.trim());
            if big_button(ui, &label, false).on_hover_text(tl!("Apply this scale to the page")).clicked() {
                ratio = Some(f.ratio);
            }
        }
    }
    ui.horizontal_wrapped(|ui| {
        for n in QUICK_RATIOS {
            let b = egui::Button::new(egui::RichText::new(measure_ui::format_ratio(n)).font(theme::medium(14.0))).min_size(vec2(72.0, 44.0));
            if ui.add_enabled(doc.allows_annotation(), b).clicked() {
                ratio = Some(n);
            }
        }
    });
    if big_button(ui, tl!("Calibrate from two points"), app.quick_tool == QuickTool::Measure(measure_ui::Tool::Calibrate)).clicked() {
        app.left = LeftPanel::Tool("measure");
        app.views[index].measure.cancel();
        app.quick_tool = QuickTool::Measure(measure_ui::Tool::Calibrate);
    }

    // Measure.
    heading(ui, t, tl!("Measure"));
    let mut tool = None;
    for (label, value) in [
        ("Smart fill: tap inside a room", measure_ui::Tool::Fill),
        ("Distance", measure_ui::Tool::Distance),
        ("Perimeter", measure_ui::Tool::Perimeter),
        ("Area, point by point", measure_ui::Tool::Area),
    ] {
        if big_button(ui, tl!(label), app.quick_tool == QuickTool::Measure(value)).clicked() {
            tool = Some(value);
        }
    }
    let state = &mut app.views[index].revue;
    let mut gaps = state.fill_options.close_gaps as f32;
    if ui.add(egui::Slider::new(&mut gaps, 0.0..=12.0).step_by(1.0).text(tl!("Close gaps (doors)"))).changed() {
        state.fill_options.close_gaps = gaps as u32;
    }
    if state.fill.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tl!("Finding the room's walls…"));
        });
    }
    if let Some(reading) = &app.views[index].measure.reading {
        ui.label(egui::RichText::new(&reading.label).font(theme::semibold(18.0)).color(t.text));
    }

    // Ruler.
    heading(ui, t, tl!("Scale ruler"));
    let state = &mut app.views[index].revue;
    let shown = state.ruler.is_some_and(|r| r.page == page);
    if big_button(ui, if shown { tl!("Hide ruler") } else { tl!("Show ruler") }, shown).clicked() {
        toggle_ruler(state, page);
        if state.ruler.is_some() {
            tool = Some(measure_ui::Tool::Ruler);
        }
    }
    if let Some(r) = state.ruler.as_mut() {
        ui.horizontal_wrapped(|ui| {
            for deg in [0.0, 45.0, 90.0, 135.0] {
                let b = egui::Button::new(egui::RichText::new(format!("{deg}°")).font(theme::medium(14.0))).min_size(vec2(56.0, 44.0));
                if ui.add(b).clicked() {
                    r.angle = -f64::to_radians(deg);
                }
            }
        });
        let mut length = r.length;
        if ui.add(egui::Slider::new(&mut length, 72.0..=1440.0).text(tl!("Length (pt)"))).changed() {
            r.length = length;
        }
        ui.add(widgets::checkbox(&mut state.straighten, tl!("Draw straight along the ruler")));
        if ui.button(tl!("Move or turn the ruler")).clicked() {
            tool = Some(measure_ui::Tool::Ruler);
        }
    }

    // Layers.
    heading(ui, t, tl!("Layers"));
    layers_section(app, ui, t, index, id);

    let state = &mut app.views[index].revue;
    if let Some(notice) = &state.notice {
        ui.colored_label(t.accent, notice);
    }
    if let Some(error) = &state.error {
        ui.colored_label(Color32::from_rgb(190, 50, 50), error);
    }
    if let Some(n) = ratio {
        apply_ratio(app, n);
    }
    if let Some(tool) = tool {
        app.views[index].measure.cancel();
        app.quick_tool = QuickTool::Measure(tool);
    }
}

/// The document's layers with the one drawn on, their eyes and a new-layer field. A document
/// without layers shows the default layer, written to the file with the first markup.
pub(crate) fn layers_section(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, index: usize, id: pdfcraft_engine::DocId) {
    let Some(doc) = app.session.get(id) else { return };
    let mut rows: Vec<(Option<usize>, String, bool)> =
        doc.info.layers.iter().enumerate().map(|(i, l)| (Some(i), l.name.clone(), l.visible)).collect();
    if rows.is_empty() {
        rows.push((None, DEFAULT_LAYER.into(), true));
    }
    let allowed = doc.allows_annotation();
    let state = &mut app.views[index].revue;
    ui.add(widgets::checkbox(&mut state.on_layers, tl!("Draw new markups on the active layer")));
    let mut toggle = None;
    for (i, name, visible) in &rows {
        ui.horizontal(|ui| {
            let eye = crate::icons::button(
                ui,
                if *visible { "eye" } else { "eye-off" },
                40.0,
                false,
                if *visible { tl!("Hide layer") } else { tl!("Show layer") },
            );
            if eye.clicked()
                && let Some(i) = i
            {
                toggle = Some((*i, !*visible));
            }
            let active = state.on_layers && state.active_layer == *name;
            let b =
                egui::Button::new(egui::RichText::new(name).font(theme::medium(14.0))).selected(active).min_size(vec2(ui.available_width(), 40.0));
            if ui.add(b).on_hover_text(tl!("Draw on this layer")).clicked() {
                state.active_layer = name.clone();
                state.on_layers = true;
            }
        });
    }
    if !rows.iter().any(|(_, n, _)| *n == state.active_layer) && state.on_layers {
        ui.colored_label(t.text_faint, crate::i18n::fmt(tl!("New markups create the layer {name}."), &[("name", &state.active_layer)]));
    }
    let mut add = None;
    ui.horizontal(|ui| {
        ui.add(widgets::line(&mut state.new_layer).hint_text(tl!("New layer name")).desired_width(ui.available_width() - 64.0).char_limit(128));
        if ui.add_enabled(allowed, egui::Button::new(tl!("Add")).min_size(vec2(56.0, 40.0))).clicked() {
            add = Some(state.new_layer.clone());
        }
    });
    if let Some((i, visible)) = toggle
        && app.session.set_layer_visible(id, i, visible)
    {
        app.views[index].invalidate_content();
    }
    if let Some(name) = add {
        let name =
            if name.trim().is_empty() { next_layer_name(app.session.get(id), app.session.get(id).map_or(0, |d| d.info.layers.len())) } else { name };
        add_layer(app, index, &name);
    }
    let _ = t;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ruler() -> Ruler {
        Ruler { page: 0, origin: [100.0, 100.0], angle: 0.0, length: 300.0 }
    }

    #[test]
    fn a_stroke_along_the_edge_comes_out_straight() {
        let stroke: Vec<[f64; 2]> = (0..20).map(|i| [110.0 + 10.0 * f64::from(i), 100.0 + if i % 2 == 0 { 3.0 } else { -2.0 }]).collect();
        let straight = straightened(&stroke, &ruler()).unwrap();
        assert_eq!(straight, vec![[110.0, 100.0], [300.0, 100.0]]);
    }

    #[test]
    fn a_stroke_drawn_away_from_the_ruler_is_left_alone() {
        let stroke: Vec<[f64; 2]> = (0..20).map(|i| [110.0 + 10.0 * f64::from(i), 160.0]).collect();
        assert!(straightened(&stroke, &ruler()).is_none());
        // A dot, or a single point, stays as drawn.
        assert!(straightened(&[[120.0, 100.0], [120.4, 100.0]], &ruler()).is_none());
        assert!(straightened(&[[120.0, 100.0]], &ruler()).is_none());
    }

    #[test]
    fn a_turned_ruler_straightens_along_its_angle() {
        let r = Ruler { angle: std::f64::consts::FRAC_PI_2, ..ruler() };
        let stroke: Vec<[f64; 2]> = (0..10).map(|i| [101.0, 120.0 + 10.0 * f64::from(i)]).collect();
        let s = straightened(&stroke, &r).unwrap();
        assert!((s[0][0] - 100.0).abs() < 1e-9 && (s[1][1] - 210.0).abs() < 1e-9, "{s:?}");
    }

    #[test]
    fn angles_snap_to_fifteen_degrees_near_them() {
        assert_eq!(snap_angle(1f64.to_radians(), false), 0.0);
        assert!((snap_angle(7f64.to_radians(), false) - 7f64.to_radians()).abs() < 1e-12);
        assert_eq!(snap_angle(7f64.to_radians(), true), 0.0);
        assert!((snap_angle(8f64.to_radians(), true) - 15f64.to_radians()).abs() < 1e-12);
    }

    #[test]
    fn the_ruler_body_and_handle_are_where_they_are_drawn() {
        let r = ruler();
        assert!(on_body(&r, [200.0, 120.0]));
        assert!(!on_body(&r, [200.0, 80.0]));
        assert!(near_handle(&r, [400.0, 100.0], 1.0));
        assert!(!near_handle(&r, [300.0, 100.0], 1.0));
    }

    #[test]
    fn signatures_and_stamps_stay_off_layers() {
        let ink = Edit::AddAnnotation(pdfcraft_engine::NewAnnotation {
            page: 0,
            shape: Shape::Ink { strokes: vec![] },
            style: Style::default(),
            contents: String::new(),
            author: String::new(),
        });
        assert!(drawable(&ink));
        assert!(!drawable(&Edit::DeletePages { pages: vec![0] }));
    }

    #[test]
    fn graduations_read_without_trailing_zeros() {
        assert_eq!(trim_number(2.0), "2");
        assert_eq!(trim_number(0.5), "0.5");
        assert_eq!(trim_number(0.0), "0");
        assert_eq!(trim_number(12.25), "12.25");
    }

    #[test]
    fn new_layers_get_free_names() {
        assert_eq!(next_layer_name(None, 0), "Layer 2");
        assert_eq!(next_layer_name(None, 2), "Layer 3");
    }
}
