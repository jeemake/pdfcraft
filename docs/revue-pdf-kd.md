# Revue PDF K&D

Revue PDF K&D is PdfCraft set up as a drawing board for reviewing plans on a large touch
display, such as an 86-inch interactive panel. It keeps everything PdfCraft does: reading,
comments, forms, organising pages and measuring. It adds pens, scale tools and layers for
architects.

## Where the ideas come from

Two published apps guided the feature list. Only their public descriptions were used. Revue
copies none of their code, artwork, icons, names or wording. Its icons come from Lucide, and its
look is PdfCraft's own.

| Idea | Seen in | In Revue |
| --- | --- | --- |
| Pen-first canvas with a movable, hideable tool palette | Drawboard PDF (radial menu, tools drawer) | A floating toolbar with 48-pixel buttons, preloaded with the Revue tools. Drag it anywhere or dock it to an edge. |
| Measure length, area and perimeter after setting a page scale | Drawboard PDF (Measure tab, Set page scale, Calibrate) | Distance, perimeter, area, calibrating from two points, and one-tap 1:20 to 1:1000 scales |
| Draw to scale, and read the scale from the sheet | Morpholio Trace (set any drawing to scale) | **Read scale from page** finds "SCALE 1:100", "Échelle 1/50" or "1/100e" in the sheet's text. It is metric only. |
| A ruler for straight lines and angles | Trace (Super Ruler, protractor), Drawboard (ruler, protractor) | **Scale ruler**: its graduations are in metres at the page's scale. Drag it to move it, and drag its end handle to turn it (it snaps to 15°). Ink drawn along its edge comes out straight. |
| Area that fills as you draw | Trace (Smart Fill) | **Smart fill**: tap inside a room to measure its area. Door gaps are closed, and text or furniture inside the room still counts as floor. |
| A range of pens | Trace (technical pen, markers, pencil, watercolour) | Technical pen, fine liner, marker, graphite pencil, colour wash, highlighter and eraser |
| Layers, like sheets of tracing paper | Trace (layer control) | **Markup layers**: each document opens with *Layer 1* over the original PDF. You can add more layers, choose the one you draw on, and show or hide each one. |

## Using it on the touch display

- **Draw**: tap a pen on the toolbar, then draw with a finger or stylus. Pinch to zoom, and pick
  *Pan* to move around the sheet with one finger.
- **Set the scale**: open *All tools ▸ Revue drawing board* and tap **Read scale from page**,
  then tap the scale it finds. If the sheet has none, tap a scale button (1:50, 1:100…) or use
  **Calibrate from two points** and type a known length.
- **Measure a room**: tap the paint-bucket tool, then tap inside the room. Its area appears and
  is saved as an area measurement. If the fill leaks through a doorway, raise **Close gaps
  (doors)** and tap again.
- **Ruler**: tap the ruler tool, then tap where the ruler should go. Drag the ruler body to move
  it, and drag the round handle to turn it. Choose a pen and draw along the top edge to get a
  straight line.
- **Layers**: the Layers panel on the right lists the layers. Tap the eye to show or hide a
  layer. Pick the layer you draw on under *Draw on*, and add more with **New markup layer**.
  *Layer 1* is written into the PDF only with the first markup, so opening a file never changes
  it.

Layers are standard PDF optional content, so other PDF readers show and hide them too.

## Installing on the display (Windows)

1. Build the installer on a Windows machine with the release workflow, or run
   `packaging/windows/package.ps1`. It produces an `.msi`.
2. Copy the `.msi` to the display's PC and double-click it. Leave *Add to the PDF context menu*
   ticked.
3. To open a plan, right-click the PDF and choose **Open with ▸ Revue PDF K&D**, or **Open with
   Revue PDF K&D** on the classic menu (on Windows 11 it is under *Show more options*). To always
   open PDFs in Revue, choose **Open with ▸ Choose another app**, pick Revue PDF K&D and tick
   *Always*.

The installer never takes over an existing PDF default by itself. You choose.

## Headless use

Every feature also works from `pdfcraft-cli run` and MCP. These tools are new:

- `measure_read_scale`
- `measure_fill`
- `layer_add`

`layer_set` shows or hides a layer.
