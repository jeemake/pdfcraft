//! Measuring a plan without calibrating it by hand: the drawing scale written on a page, and
//! the area enclosed around a point (smart fill).

use std::sync::Arc;

use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest};

use crate::{Document, guard, measure};

/// Pixels per point smart fill reads a page at, before [`measure::region::MAX_PIXELS`] lowers it.
const FILL_PIXELS_PER_POINT: f64 = 3.0;
/// Most of [`measure::region::MAX_PIXELS`] a page raster may use.
const FILL_PIXEL_BUDGET: f64 = 16_000_000.0;

/// What smart fill found: the outline in PDF user space and the raster resolution it was
/// read at (pixels per point), which bounds its precision.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    pub points: Vec<measure::Point>,
    pub pixels_per_point: f64,
}

impl Document {
    /// The drawing scales written on `page` (title blocks, "SCALE 1:100", `1/4" = 1'-0"`), the
    /// labelled ones first. Empty when the page has no text or the document can't be read.
    pub fn page_scales(&self, page: usize) -> Vec<measure::scale_text::FoundScale> {
        if page >= self.info.pages.len() {
            return Vec::new();
        }
        let mut text = String::new();
        for line in self.text_lines(page) {
            if text.len() > measure::scale_text::MAX_TEXT {
                break;
            }
            text.push_str(&line.text);
            // Lines stay apart, so "1" at the end of one and ":100" opening the next don't meet.
            text.push_str("  \n");
        }
        measure::scale_text::find(&text)
    }

    /// Smart fill on `page` at `at` (view points: top-left origin, y down), as work to run away
    /// from the window: it renders the page and traces the enclosed region around the point.
    pub fn smart_fill_job(
        &self,
        page: usize,
        at: measure::Point,
        options: measure::region::FillOptions,
    ) -> impl FnOnce() -> Result<Fill, String> + Send + 'static {
        let size = self.info.pages.get(page).map(|p| (f64::from(p.width), f64::from(p.height)));
        let bytes = self.display.clone();
        // The page as drawn, without comments: earlier fills and markups mustn't wall it in.
        let config = RenderConfig { hide_comments: true, system_fonts: false, ..self.config.clone() };
        let cos = self.editor.as_ref().map(|e| e.cos.clone());
        move || {
            let (w, h) = size.ok_or("no such page")?;
            let cos = cos.ok_or("the document can't be read")?;
            let outline = fill_outline(&bytes, &config, page, w, h, at, &options)?;
            let points =
                outline.0.iter().map(|p| measure::view_to_user(&cos, page, *p).map_err(|e| e.to_string())).collect::<Result<Vec<_>, String>>()?;
            Ok(Fill { points, pixels_per_point: outline.1 })
        }
    }

    /// [`Self::smart_fill_job`], run here.
    pub fn smart_fill(&self, page: usize, at: measure::Point, options: measure::region::FillOptions) -> Result<Fill, String> {
        self.smart_fill_job(page, at, options)()
    }
}

/// The region's outline in view points, and the pixels per point it was read at.
fn fill_outline(
    bytes: &Arc<Vec<u8>>,
    config: &RenderConfig,
    page: usize,
    w: f64,
    h: f64,
    at: measure::Point,
    options: &measure::region::FillOptions,
) -> Result<(Vec<measure::Point>, f64), String> {
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return Err("the page has no size".into());
    }
    if !(at[0].is_finite() && at[1].is_finite()) || at[0] < 0.0 || at[1] < 0.0 || at[0] > w || at[1] > h {
        return Err(measure::region::FillError::Outside.to_string());
    }
    let scale = FILL_PIXELS_PER_POINT.min((FILL_PIXEL_BUDGET / (w * h)).sqrt()).max(0.05);
    let rendered = guard(|| {
        let mut renderer = PageRenderer::new(bytes.clone(), config.clone());
        renderer.render(RenderRequest { page, scale: scale as f32, ..Default::default() })
    })?;
    if let Some(e) = rendered.error {
        return Err(format!("the page could not be read: {e}"));
    }
    let (pw, ph) = (rendered.width as usize, rendered.height as usize);
    let lum = measure::region::luminance(&rendered.rgba, pw, ph).ok_or("the page is too large to fill")?;
    // The raster's own pixels per point, which rounding of its size may change slightly.
    let (sx, sy) = (pw as f64 / w, ph as f64 / h);
    let region = measure::region::fill(&lum, pw, ph, [at[0] * sx, at[1] * sy], options).map_err(|e| e.to_string())?;
    Ok((region.outline.iter().map(|p| [p[0] / sx, p[1] / sy]).collect(), sx.min(sy)))
}

#[cfg(test)]
mod tests {
    use crate::{Session, tests::fixture};
    use std::sync::Arc;

    /// One 200 × 300 page with a 100 × 80 pt room (2 pt walls), and a title "ECHELLE 1/50".
    fn plan() -> Vec<u8> {
        let body = "0 g 50 150 104 2 re f 50 230 104 2 re f 50 150 2 82 re f 152 150 2 82 re f BT /F1 10 Tf 20 20 Td (ECHELLE 1/50) Tj ET";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [4 0 R] /Count 1 /MediaBox [0 0 200 300] >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << /Font << /F1 3 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    fn open(bytes: Vec<u8>) -> (Session, crate::DocId) {
        let mut s = Session::new();
        let id = s.open("plan.pdf", None, Arc::new(bytes), None).unwrap();
        (s, id)
    }

    #[test]
    fn the_scale_in_a_title_block_is_read() {
        let (s, id) = open(plan());
        let found = s.get(id).unwrap().page_scales(0);
        assert_eq!(found.first().map(|f| f.ratio), Some(50.0));
        assert!(s.get(id).unwrap().page_scales(9).is_empty());
        let (s, id) = open(fixture(1));
        assert!(s.get(id).unwrap().page_scales(0).is_empty());
    }

    #[test]
    fn a_tap_inside_the_room_finds_its_walls() {
        let (s, id) = open(plan());
        // View space is y down: the room's inside spans x 52..152, y 300-230=70 .. 300-152=148.
        let fill = s.get(id).unwrap().smart_fill(0, [100.0, 110.0], Default::default()).unwrap();
        let area = pdfcraft_measure::region::area(&fill.points);
        // 100 × 78 pt inside the walls, within a pixel's width all round.
        let tolerance = 2.0 * (100.0 + 78.0) / fill.pixels_per_point + 4.0;
        assert!((area - 100.0 * 78.0).abs() < tolerance, "area {area}");
    }

    #[test]
    fn a_tap_outside_any_room_or_off_the_page_is_an_error() {
        let (s, id) = open(plan());
        let doc = s.get(id).unwrap();
        assert!(doc.smart_fill(0, [10.0, 290.0], Default::default()).unwrap_err().contains("isn't enclosed"));
        assert!(doc.smart_fill(0, [-5.0, 10.0], Default::default()).is_err());
        assert!(doc.smart_fill(0, [f64::NAN, 10.0], Default::default()).is_err());
        assert!(doc.smart_fill(3, [10.0, 10.0], Default::default()).is_err());
    }
}
