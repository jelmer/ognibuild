from typing import Callable, TypeVar

from breezy.tree import Tree

from .fix_build import BuildFixer

DEFAULT_LIMIT = 200

def sanitize_session_name(name: str) -> str: ...
def generate_session_id(name: str) -> str: ...
def export_vcs_tree(
    tree: Tree, path: str, subpath: str | None = None
) -> None: ...
def resolve_error(error, phase, fixers) -> bool: ...

T = TypeVar("T")

def iterate_with_build_fixers(
    fixers: list[BuildFixer],
    phase: list[str],
    cb: Callable[[], T], limit=DEFAULT_LIMIT
) -> T: ...
