"""What ships where. One table, read by both the exporter and the icon renderer.

Each script used to carry its own list, which is how `wood.png` once shipped blank and how the
wardrobe sheets ended up captioned with the wrong outfits. A single mapping cannot disagree with
itself.

  object name -> (glb path under client/assets/, icon filename or None)
"""

ITEMS = {
    # carried resource bundles -- attach to `attach.carry` on the chest
    "WoodBundle":    ("game_assets/resources/carried/WoodBundle.glb",  "wood.png"),
    "WheatSheaf":    ("game_assets/resources/carried/WheatSheaf.glb",  "wheat.png"),
    "FishBasket":    ("game_assets/resources/carried/FishBasket.glb",  "fish.png"),
    "StoneBundle":   ("game_assets/resources/carried/StoneBundle.glb", "stone.png"),
    "IronBundle":    ("game_assets/resources/carried/IronBundle.glb",  "iron.png"),
    "FlourSack":     ("game_assets/resources/carried/FlourSack.glb",   "flour.png"),
    "BreadBasket":   ("game_assets/resources/carried/BreadBasket.glb", "bread.png"),
    "WoolFleece":    ("game_assets/resources/carried/WoolFleece.glb",  "wool.png"),
    "MeatHaunch":    ("game_assets/resources/carried/MeatHaunch.glb",  "meat.png"),
    # hand tools -- attach to `attach.tool.R` on the right hand
    "AxeFelling":    ("game_assets/tools/AxeFelling.glb",    "axe.png"),
    "HammerFraming": ("game_assets/tools/HammerFraming.glb", "hammer.png"),
    "ScytheMowing":  ("game_assets/tools/ScytheMowing.glb",  "scythe.png"),
}
