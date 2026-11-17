//! Markup layers: optional content groups (ISO 32000-2 §8.11) made for drawing on, like sheets
//! of tracing paper over a plan. A layer is listed in `/OCProperties` and shown by default; a
//! comment or measurement drawn on it carries `/OC` naming it (§12.5.2, Table 166), so it shows,
//! hides and prints with the layer.

use pdfcraft_cos::{Dict, Document, ObjRef, Object, PdfString};

use crate::{EditError, edit::Edit};

impl crate::Document {
    /// `edit` (a new comment, drawing or measurement) drawn on the layer named `layer`, which
    /// [`Edit::OnLayer`] creates, in the same undo step, when the document doesn't have it yet.
    pub fn on_layer(&self, layer: &str, edit: Edit) -> Edit {
        Edit::OnLayer { layer: layer.into(), edit: Box::new(edit) }
    }
}

/// The layer every document starts with in the Layers panel, over the original PDF. It is
/// written into the file with the first markup drawn on it ([`Document::on_layer`]), so opening
/// a file never changes it.
pub const DEFAULT_LAYER: &str = "Layer 1";

/// Longest layer name, in bytes.
pub const MAX_NAME: usize = 128;
/// Most layers a document may list for this edit to add another.
const MAX_LAYERS: usize = 4096;

fn invalid(s: impl Into<String>) -> EditError {
    EditError::Invalid(s.into())
}

/// The catalog's `/OCProperties`: where it is stored (an indirect object, or inline) and its value.
fn properties(doc: &Document) -> Result<(ObjRef, Option<ObjRef>, Dict), EditError> {
    let root = doc.root().ok_or_else(|| invalid("the document has no catalog"))?;
    let catalog = doc.get(root).as_dict().cloned().ok_or_else(|| invalid("the document's catalog isn't a dictionary"))?;
    Ok(match catalog.get(b"OCProperties") {
        Some(Object::Ref(r)) => (root, Some(*r), doc.get(*r).as_dict().cloned().unwrap_or_default()),
        Some(Object::Dict(d)) => (root, None, d.clone()),
        _ => (root, None, Dict::new()),
    })
}

fn array(doc: &Document, d: &Dict, key: &[u8]) -> Vec<Object> {
    d.get(key).map(|v| doc.resolve(v)).and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

/// The layers' names and objects, in `/OCGs` order.
pub(crate) fn layers(doc: &Document) -> Vec<(String, ObjRef)> {
    let Ok((_, _, props)) = properties(doc) else { return Vec::new() };
    array(doc, &props, b"OCGs")
        .iter()
        .take(MAX_LAYERS)
        .filter_map(|o| {
            let r = o.as_ref()?;
            let name = doc.get(r).as_dict().and_then(|d| d.get(b"Name").map(|n| doc.resolve(n))).and_then(|n| n.as_string().map(|s| s.to_text()))?;
            Some((name, r))
        })
        .collect()
}

/// Add a layer named `name`, shown, at the end of the layer list.
pub(crate) fn add_layer(doc: &mut Document, name: &str) -> Result<(), EditError> {
    let name = name.trim();
    if name.is_empty() || name.len() > MAX_NAME {
        return Err(invalid(format!("a layer needs a name of 1 to {MAX_NAME} bytes")));
    }
    if layers(doc).iter().any(|(n, _)| n == name) {
        return Err(invalid(format!("there is already a layer named {name}; choose another name")));
    }
    let (root, props_ref, mut props) = properties(doc)?;
    let mut all = array(doc, &props, b"OCGs");
    if all.len() >= MAX_LAYERS {
        return Err(invalid("the document has too many layers"));
    }
    let mut group = Dict::new();
    group.set(b"Type".to_vec(), Object::name("OCG"));
    group.set(b"Name".to_vec(), PdfString::text(name));
    let r = Object::Ref(doc.add(Object::Dict(group)));
    let mut config = props.get(b"D").map(|d| doc.resolve(d)).and_then(|d| d.as_dict().cloned()).unwrap_or_default();
    let mut order = array(doc, &config, b"Order");
    // Without `/BaseState /OFF` a group is on unless `/OFF` lists it; `/ON` makes it explicit.
    let mut on = array(doc, &config, b"ON");
    all.push(r.clone());
    order.push(r.clone());
    on.push(r);
    props.set(b"OCGs".to_vec(), Object::Array(all));
    config.set(b"Order".to_vec(), Object::Array(order));
    config.set(b"ON".to_vec(), Object::Array(on));
    props.set(b"D".to_vec(), Object::Dict(config));
    match props_ref {
        Some(r) => doc.set(r, Object::Dict(props)),
        None => doc.update_dict(root, |c| c.set(b"OCProperties".to_vec(), Object::Dict(props))).map_err(|e| invalid(e.to_string()))?,
    }
    Ok(())
}

/// Run [`Edit::OnLayer`]: `edit`, then what it added marked as the layer's content. A layer the
/// document doesn't have yet is made first, in the same undo step.
pub(crate) fn run_on_layer(doc: &mut Document, layer: &str, edit: &Edit, cx: &mut crate::edit::EditCtx) -> Result<(), EditError> {
    let page = page_of(edit)?;
    let group = match layer_named(doc, layer) {
        Ok(group) => group,
        Err(_) => {
            add_layer(doc, layer)?;
            layer_named(doc, layer)?
        }
    };
    let before = annotation_count(doc, page);
    crate::edit::run_edit(doc, edit, cx)?;
    put_on_layer(doc, page, before, group)
}

/// The page an edit drawn on a layer adds to: only comments and measurements can be.
pub(crate) fn page_of(edit: &Edit) -> Result<usize, EditError> {
    match edit {
        Edit::AddAnnotation(a) => Ok(a.page),
        Edit::AddMeasurement(m) => Ok(m.page),
        _ => Err(invalid("only a new comment, drawing or measurement can be drawn on a layer")),
    }
}

/// How many annotations `page` lists.
pub(crate) fn annotation_count(doc: &Document, page: usize) -> usize {
    pdfcraft_model::pages(doc)
        .get(page)
        .and_then(|p| p.dict.get(b"Annots").map(|a| doc.resolve(a)))
        .and_then(|a| a.as_array().map(Vec::len))
        .unwrap_or(0)
}

/// The layer named `layer`, or an error that lists the layers there are.
pub(crate) fn layer_named(doc: &Document, layer: &str) -> Result<ObjRef, EditError> {
    let Some((_, group)) = layers(doc).into_iter().find(|(n, _)| n == layer) else {
        let names: Vec<String> = layers(doc).into_iter().map(|(n, _)| n).collect();
        return Err(invalid(if names.is_empty() {
            "the document has no layers; add one first".to_string()
        } else {
            format!("no layer named {layer}; the layers are: {}", names.join(", "))
        }));
    };
    Ok(group)
}

/// Mark the annotations of `page` from index `from` on as content of the layer `group`.
pub(crate) fn put_on_layer(doc: &mut Document, page: usize, from: usize, group: ObjRef) -> Result<(), EditError> {
    let annots = pdfcraft_model::pages(doc)
        .get(page)
        .and_then(|p| p.dict.get(b"Annots").map(|a| doc.resolve(a)))
        .and_then(|a| a.as_array().cloned())
        .unwrap_or_default();
    for o in annots.iter().skip(from) {
        let Some(r) = o.as_ref() else { continue };
        // A popup belongs to its parent and follows it; marking it too is harmless and keeps
        // it hidden with the layer.
        doc.update_dict(r, |d| d.set(b"OC".to_vec(), Object::Ref(group))).map_err(|e| invalid(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewAnnotation, Session, Shape, Style};
    use std::sync::Arc;

    fn session() -> (Session, crate::DocId) {
        let mut s = Session::new().with_clock(|| 1_700_000_000);
        let id = s.open("fixture.pdf", None, Arc::new(crate::tests::fixture(2)), None).unwrap();
        (s, id)
    }

    fn ink(page: usize) -> Edit {
        Edit::AddAnnotation(NewAnnotation {
            page,
            shape: Shape::Ink { strokes: vec![vec![[10.0, 10.0], [50.0, 60.0]]] },
            style: Style::default(),
            contents: String::new(),
            author: "K&D".into(),
        })
    }

    #[test]
    fn a_new_layer_is_listed_and_shown() {
        let (mut s, id) = session();
        s.apply(id, Edit::AddLayer { name: "Esquisse".into() }).unwrap();
        s.apply(id, Edit::AddLayer { name: "Cotations".into() }).unwrap();
        let doc = s.get(id).unwrap();
        let names: Vec<(&str, bool)> = doc.info.layers.iter().map(|l| (l.name.as_str(), l.visible)).collect();
        assert_eq!(names, vec![("Esquisse", true), ("Cotations", true)]);
    }

    #[test]
    fn duplicate_and_empty_names_are_refused() {
        let (mut s, id) = session();
        s.apply(id, Edit::AddLayer { name: "A".into() }).unwrap();
        assert!(s.apply(id, Edit::AddLayer { name: "A".into() }).is_err());
        assert!(s.apply(id, Edit::AddLayer { name: "  ".into() }).is_err());
        assert!(s.apply(id, Edit::AddLayer { name: "x".repeat(MAX_NAME + 1) }).is_err());
    }

    #[test]
    fn a_drawing_on_a_layer_is_its_content() {
        let (mut s, id) = session();
        s.apply(id, Edit::AddLayer { name: "Esquisse".into() }).unwrap();
        s.apply(id, ink(1)).unwrap();
        s.apply(id, Edit::OnLayer { layer: "Esquisse".into(), edit: Box::new(ink(1)) }).unwrap();
        let doc = s.get(id).unwrap();
        let cos = &doc.editor.as_ref().unwrap().cos;
        let (_, group) = layers(cos).into_iter().next().unwrap();
        let page = &pdfcraft_model::pages(cos)[1];
        let annots = cos.resolve(page.dict.get(b"Annots").unwrap()).as_array().cloned().unwrap();
        let oc: Vec<Option<Object>> = annots.iter().map(|a| cos.get(a.as_ref().unwrap()).as_dict().unwrap().get(b"OC").cloned()).collect();
        // The first drawing is on no layer; the second's own annotation (and any popup) is.
        assert_eq!(oc[0], None);
        assert!(oc[1..].iter().all(|o| *o == Some(Object::Ref(group))));
    }

    #[test]
    fn drawing_with_another_edit_or_on_a_bad_name_is_refused_and_changes_nothing() {
        let (mut s, id) = session();
        assert!(s.apply(id, Edit::OnLayer { layer: " ".into(), edit: Box::new(ink(0)) }).is_err());
        assert_eq!(annotation_count(&s.get(id).unwrap().editor.as_ref().unwrap().cos, 0), 0);
        assert!(s.get(id).unwrap().info.layers.is_empty());
        s.apply(id, Edit::AddLayer { name: "A".into() }).unwrap();
        let err = s.apply(id, Edit::OnLayer { layer: "A".into(), edit: Box::new(Edit::DeletePages { pages: vec![0] }) }).unwrap_err();
        assert!(err.to_string().contains("only a new comment"), "{err}");
    }

    /// Dark pixels of page `page` as the viewer draws it now.
    fn ink_pixels(s: &Session, id: crate::DocId, page: usize) -> usize {
        let doc = s.get(id).unwrap();
        let config = s.render_config(id).unwrap();
        let mut r = pdfcraft_render::PageRenderer::new(doc.display.clone(), config);
        let out = r.render(pdfcraft_render::RenderRequest { page, scale: 1.0, ..Default::default() });
        out.rgba.chunks_exact(4).filter(|p| p[3] > 128 && (p[0] < 240 || p[1] < 240 || p[2] < 240)).count()
    }

    #[test]
    fn hiding_a_layer_hides_the_drawings_on_it() {
        let (mut s, id) = session();
        s.apply(id, Edit::AddLayer { name: "Esquisse".into() }).unwrap();
        let blank = ink_pixels(&s, id, 1);
        s.apply(id, Edit::OnLayer { layer: "Esquisse".into(), edit: Box::new(ink(1)) }).unwrap();
        let drawn = ink_pixels(&s, id, 1);
        assert!(drawn > blank, "{drawn} > {blank}");
        assert!(s.set_layer_visible(id, 0, false));
        assert_eq!(ink_pixels(&s, id, 1), blank);
    }

    #[test]
    fn the_default_layer_is_made_with_the_first_markup_in_one_undo_step() {
        let (mut s, id) = session();
        assert!(s.get(id).unwrap().info.layers.is_empty());
        let edit = s.get(id).unwrap().on_layer(DEFAULT_LAYER, ink(0));
        // The undo step keeps the markup's own name.
        assert_eq!(edit.label(), ink(0).label());
        s.apply(id, edit).unwrap();
        let edit = s.get(id).unwrap().on_layer(DEFAULT_LAYER, ink(1));
        s.apply(id, edit).unwrap();
        assert_eq!(s.get(id).unwrap().info.layers.len(), 1);
        s.undo(id).unwrap();
        s.undo(id).unwrap();
        assert!(s.get(id).unwrap().info.layers.is_empty());
        assert_eq!(annotation_count(&s.get(id).unwrap().editor.as_ref().unwrap().cos, 0), 0);
    }

    #[test]
    fn adding_a_layer_is_undone_in_one_step() {
        let (mut s, id) = session();
        s.apply(id, Edit::AddLayer { name: "A".into() }).unwrap();
        assert!(s.undo(id).is_ok());
        assert!(s.get(id).unwrap().info.layers.is_empty());
    }
}
