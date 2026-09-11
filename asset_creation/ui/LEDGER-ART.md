# Encyclopedia art

The runtime material catalogue lives in `client/assets/ui/ledger/`. Text and
interactive controls are native Bevy UI. Never ship a composited screen image.

`build_ledger.py` is the editable source for the tiny button faces, portrait ring
and pennant. The source coordinates allow deliberate imperfections while native
nine-slicing keeps edges crisp at different control sizes. Its PNG output uses
an indexed palette and alpha. Bevy slice borders must leave a nonzero centre.

The paper, wood, corner plate and portrait backdrop were generated with the
prompts in `ledger-material-prompts.json`. Large generation outputs and approved
screen concepts are review sources under ignored `logs/` and outside the repo;
the compressed runtime images are the canonical artwork. Paper/wood use 512px
JPEGs (quality 82), corner hardware uses a 192px alpha PNG, and the softly blurred
portrait backdrop uses a 512px indexed PNG. Runtime sampling shares these images.
Do not bake text or a character into the backdrop.

`village.jpg` is an illustrative medieval hamlet landscape, not a generated-world
survey. Its 768×512 JPEG is shared. Exact building images come from canonical Bevy
models through `build_ledger_thumbnails.py`; its capture recipes and generator
are maintained, while the large review PNGs and metadata stay in `logs/`.

Run `python3 asset_creation/ui/check_ledger_assets.py` after changing the catalogue.
It checks a 1.5 MiB delivery budget and 12 MiB decoded RGBA budget for all catalogue
images combined. Neither is a claim about total game memory: the separate dynamic
character portrait cache is hard capped at 32 MiB, with one worker and shared model
geometry. Referenced building art and identical outfits are reused, never copied
per list row.

The reading font is Libre Baskerville from Google Fonts:
https://github.com/google/fonts/tree/main/ofl/librebaskerville
The OFL license is bundled alongside the font. Cinzel headings and the existing
HUD typeface are retained. Do not substitute system fonts.
