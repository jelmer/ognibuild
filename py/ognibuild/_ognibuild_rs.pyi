from typing import Callable, TypeVar

from breezy.tree import Tree

from .fix_build import BuildFixer

DEFAULT_LIMIT = 200

def sanitize_session_name(name: str) -> str: ...
def generate_session_id(name: str) -> str: ...
def resolve_error(error, phase, fixers) -> bool: ...

T = TypeVar("T")

def iterate_with_build_fixers(
    fixers: list[BuildFixer],
    phase: list[str],
    cb: Callable[[], T],
    limit=DEFAULT_LIMIT,
) -> T: ...
def shebang_binary(p) -> str: ...
def get_user(session: Session) -> str: ...
def which(session: Session, name: str) -> str: ...

class Session:
    def __enter__(self) -> Session: ...
    def __exit__(self, exc_type, exc_value, traceback) -> bool | None: ...

    def create_home(self) -> None: ...

    def chdir(self, path: str) -> None: ...

    location: str

    def external_path(self, path: str) -> str: ...

    def check_call(self, args: list[str], cwd: str | None = None, user: str | None = None, env: dict[str, str] | None = None) -> None: ...

    def setup_from_directory(self, path: str, subdir: str | None = None) -> tuple[str, str]: ...

    def setup_from_vcs(self, tree: Tree, include_controldir: bool | None = None, subdir: str | None = None) -> tuple[str, str]: ...

    def Popen(self, args: list[str], cwd: str | None = None, user: str | None = None, stdout = None, stderr = None, stdin = None, env: dict[str, str] | None = None) -> ChildProcess: ...

    is_temporary: bool


def run_with_tee(
    session: Session,
    args: list[str],
    cwd: str | None = None,
    user: str | None = None,
    env: dict[str, str] | None = None,
    stdin = None,
    stdout = None,
    stderr = None,
) -> tuple[int, list[str]]: ...


class ChildProcess:
    ...

    returncode: int | None

    def poll(self) -> int | None: ...


class PlainSession(Session):
    def __init__(self) -> None: ...


class SchrootSession(Session):
    def __init__(self, chroot, session_prefix: str | None = None) -> None: ...


class DistCatcher:

    def __init__(self, directories: list[str]) -> None: ...

    def __enter__(self) -> DistCatcher: ...

    def __exit__(self, exc_type, exc_value, traceback) -> bool | None: ...

    def find_files(self) -> str | None: ...

    def copy_single(self, path: str) -> None: ...

    @staticmethod
    def default(directory: str) -> DistCatcher: ...


class NoSessionOpen(Exception):
    pass


class SessionSetupFailure(Exception):

    errlines: list[str]
    reason: str


class SessionAlreadyOpen(Exception):
    pass


class DistNoTarball(Exception):
    pass
