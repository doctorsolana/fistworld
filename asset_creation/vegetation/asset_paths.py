"""Portable locations for the one maintained runtime vegetation catalogue.

Generators retain editable sources beside themselves and export GLBs directly to
their runtime family. Preview output and intermediate textures belong in renders.
"""

from pathlib import Path

SOURCE = Path(__file__).resolve().parent
ROOT = SOURCE.parents[1]
ENVIRONMENT = ROOT / "client/assets/game_assets/environment"
RENDERS = SOURCE / "renders"
FAMILIES = (
    "trees/broadleaf", "trees/conifer", "trees/dead",
    "rocks", "bushes", "ferns", "flowers", "grass",
)


def runtime_directory(family):
    if family not in FAMILIES:
        raise ValueError(f"Unknown vegetation family: {family}")
    return ENVIRONMENT / family


def runtime_glbs():
    """Discover vegetation and ground scatter, excluding animals and work props."""
    return sorted(
        path
        for family in FAMILIES
        for path in runtime_directory(family).glob("*.glb")
    )


def runtime_glb(filename):
    """Resolve a filename without keeping a second per-model registry."""
    matches = [path for path in runtime_glbs() if path.name == filename]
    if len(matches) != 1:
        raise ValueError(
            f"Expected one runtime vegetation asset {filename!r}, found {len(matches)}"
        )
    return matches[0]
