# Encyclopedia art

The runtime material catalogue lives in `client/assets/ui/ledger/`. Text and
interactive controls are native Bevy UI. Never ship a composited screen image.

`build_ledger.py` is the editable source for the tiny button faces, portrait ring
and pennant. The source coordinates allow deliberate imperfections while native
nine-slicing keeps edges crisp at different control sizes. Its PNG output uses
an indexed palette and alpha. `button-close.png` has its own 64×64 square face,
preserving the complete brass rim under Bevy nine-slicing instead of squeezing
the wide tab artwork into a square. Bevy slice borders must leave a nonzero centre.

The paper, wood, corner plate and portrait backdrop were generated with the
prompts in `ledger-material-prompts.json`. Large generation outputs and approved
screen concepts are review sources under ignored `logs/` and outside the repo;
the compressed runtime images are the canonical artwork. Paper/wood use 512px
JPEGs (quality 82). The binding in `wood.jpg` is quiet dark chocolate leather with
fine organic wrinkles and broad restrained mottling, replacing the pronounced
horizontal grain. Corner hardware uses a 128px alpha PNG, and the softly blurred
portrait backdrop uses a 512px indexed PNG. Runtime sampling shares these images.
Do not bake text or a character into the backdrop.

Settlement illustrations are shared catalogue artwork, not generated-world
surveys. Each tier has a distinct settlement scale:

| File | Runtime dimensions | Visual character |
| --- | --- | --- |
| `hamlet.jpg` | 640×246 | Four cottages, gardens, well and footbridge |
| `village.jpg` | 640×213 | Village hall, several houses, fields and windmill |
| `town.jpg` | 640×246 | Clustered market town, awnings, hall and modest church |
| `city.jpg` | 640×246 | Dense neighbourhoods, high walls, towers and civic landmarks |

Exact building images come from canonical Bevy models through
`build_ledger_thumbnails.py`; its capture recipes and generator are maintained,
while the large review PNGs and metadata stay in `logs/`.

`import_ledger_paintings.py --hamlet SOURCE --village SOURCE --town SOURCE
--city SOURCE --corner SOURCE --binding SOURCE` performs only delivery resizing and encoding of
approved imagegen originals. Each flag is optional. Settlement images retain
their source aspect ratios, fit within 640×256 pixels and use quality-83 optimized
progressive JPEG. `--binding SOURCE` imports the approved square leather original
as a shared 512×512 quality-82 progressive JPEG. Originals remain untouched outside
the repository. The corner cap has actual
alpha; its widget crops the small canvas margin to meet the outside book binding.
The tiny wood-button atlas holds dark leather and selected amber faces. Its
interruptions and shallow chips are in the maintained native button generator.

Illustration edges use `shaders/ui/ledger_illustration.wgsl`: a shared transparent
fade with fixed grain, or a circular crop for directory medallions. The shader
preserves aspect ratio and does not alter canonical model images. Materials are
cached by illustration and finish, never duplicated per row. There are no animated
noise inputs, blurred-screen render passes or full-window mask textures.

Run `python3 asset_creation/ui/check_ledger_assets.py` after changing the catalogue.
It checks a 1.5 MiB delivery budget and 12 MiB decoded RGBA budget for all catalogue
images combined. Neither is a claim about total game memory: the separate dynamic
character portrait cache is hard capped at 32 MiB, with one worker and shared model
geometry. Referenced building art and identical outfits are reused, never copied
per list row.

The 2026-09-11 polished catalogue passes both budgets: **33 image files / 468,457
bytes** of compressed delivery and **12,107,264 bytes** (11.55 MiB) of decoded RGBA
equivalent. The four settlement illustrations account for 189,253 compressed
bytes and 2,434,560 decoded bytes. The Libre Baskerville font plus its licence add
176,348 bytes, giving **644,805 bytes** of ledger image/reading-font delivery.
For comparison, all 6 files under `client/assets/fonts/` total
411,007 bytes; the complete image catalogue plus that entire font directory is
879,464 bytes. Font files have no fixed decoded-RGBA equivalent: native text
atlas memory depends on the glyphs rendered, and is separate from this image audit.

The reading font is Libre Baskerville from Google Fonts:
https://github.com/google/fonts/tree/main/ofl/librebaskerville
The OFL license is bundled alongside the font. Cinzel headings and the existing
HUD typeface are retained. Do not substitute system fonts.
