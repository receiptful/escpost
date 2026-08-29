"""Loading and validation for shared renderer/printer conformance cases."""

from __future__ import annotations

import tomllib
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Case:
    directory: Path
    profile: str
    input_bytes: bytes

    @classmethod
    def load(cls, directory: str | Path) -> Case:
        directory = Path(directory).resolve()
        manifest = _load_manifest(directory / "case.toml")
        _require_schema_version(manifest)
        _reject_unknown_fields(manifest)

        return cls(
            directory=directory,
            profile=_require_string(manifest, "profile"),
            input_bytes=load_hex_input(directory / "input.hex"),
        )


class CaseError(ValueError):
    """A conformance case is missing, malformed, or internally inconsistent."""


def _load_manifest(path: Path) -> dict:
    try:
        with path.open("rb") as manifest_file:
            return tomllib.load(manifest_file)
    except FileNotFoundError as error:
        raise CaseError(f"case manifest does not exist: {path}") from error
    except tomllib.TOMLDecodeError as error:
        raise CaseError(f"invalid case manifest {path}: {error}") from error


def _require_schema_version(manifest: dict) -> None:
    version = manifest.get("schema_version")
    if version != 1:
        raise CaseError(f"unsupported case schema version {version!r}")


def _reject_unknown_fields(manifest: dict) -> None:
    unknown = set(manifest) - {
        "schema_version",
        "name",
        "profile",
    }
    if unknown:
        raise CaseError(f"unknown case field {sorted(unknown)[0]!r}")


def _require_string(manifest: dict, field: str) -> str:
    value = manifest.get(field)
    if not isinstance(value, str) or not value:
        raise CaseError(f"case field {field!r} must be a non-empty string")
    return value


def load_hex_input(path: Path) -> bytes:
    """Load a Git-versioned hexadecimal ESC/POS fixture."""
    try:
        tokens = path.read_text(encoding="ascii").split()
    except (FileNotFoundError, UnicodeDecodeError) as error:
        raise CaseError(f"could not read hexadecimal input {path}: {error}") from error

    decoded = bytearray()
    for index, token in enumerate(tokens, start=1):
        invalid_character = any(
            character not in "0123456789abcdefABCDEF" for character in token
        )
        if len(token) != 2 or invalid_character:
            raise CaseError(f"invalid hexadecimal byte {token!r} at token {index}")
        decoded.append(int(token, 16))
    return bytes(decoded)
