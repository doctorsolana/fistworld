# Character creator art

`client/assets/ui/creator/` holds four text-free material sprites made with the
built-in image generator. Prompts and the approved original filenames are in
`creator-material-prompts.json`. The compressed runtime files are canonical;
large originals and comparison screens stay outside Git.

`import_creator_art.py --paper SOURCE --brass SOURCE --journey SOURCE --frame SOURCE`
only trims empty delivery margins, resizes and encodes the approved RGBA originals.
It does not remove backgrounds or retouch art. Running it without arguments audits
the existing catalogue (768 KiB delivery / 3 MiB decoded RGBA limits). Paper uses
optimized full-colour PNG to preserve subtle grain; metal sprites use indexed PNG.
The maintained set totals 561,226 bytes on disk and 2,158,080 bytes decoded as RGBA.

Parchment is pale in the reading area and stained/frayed around the perimeter.
The square arrows use a separate brass face so its rim is not crushed by slicing
a wide button. The primary action has a chamfered amber face. The outer frame has
an actual alpha opening and substantial scratched corner hardware. Text, arrows
and decorative diamonds remain native UI, with ordinary hover, focus and spring
feedback. Do not bake a screenshot or slogans into any surface.

The creator owns its `CreatorArtwork` handles; it reuses the shared ledger's
interaction skin through `LedgerButtonFace`. It does not replace encyclopedia
textures. Character creation is shown only at startup for an account needing its
first hero; the God panel no longer opens it. The maintained
`capture/scenarios/character-creator-new-player.ron` tour waits for all four handles
and exercises the composed UI at normal and small resolutions. Its 16 shots include
all wardrobe/skin selectors, forward idle, night lighting, mandatory-screen retention
and disconnected submission feedback. The startup-only tour passed at 1600×1000
and 1280×720; see `docs/VISUAL-CAPTURE.md` for inspected PNGs and sidecar evidence.
`character-creator.ron` remains the static mail
preset review, while the old God creator tour is retired.
