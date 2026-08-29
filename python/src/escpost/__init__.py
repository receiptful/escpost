"""Python interface for the ESCPost rendering engine."""

from ._native import render, render_result

__all__ = ["render", "render_result"]
