"""Widget registry.

A widget is a class constructed with its config section (a dict) and exposing
``draw(img, draw, box, ctx)``. To add one (e.g. xkcd), create a module here,
decorate the class with ``@register("name")`` and add ``[[layout]] widget = "name"``
to config.toml.
"""
from __future__ import annotations

import logging
import time
from typing import Callable

WIDGETS: dict[str, type] = {}
log = logging.getLogger(__name__)


def register(name: str):
    def deco(cls):
        WIDGETS[name] = cls
        return cls
    return deco


class Cached:
    """Keeps the last good result of an expensive fetch; serves stale data on failure."""

    def __init__(self, fetch: Callable[[], object], ttl_seconds: float):
        self.fetch, self.ttl = fetch, ttl_seconds
        self.value, self.stamp, self.error = None, 0.0, None

    def get(self):
        if self.value is None or time.time() - self.stamp > self.ttl:
            try:
                self.value, self.stamp, self.error = self.fetch(), time.time(), None
            except Exception as e:
                log.warning("fetch failed: %s", e)
                self.error = str(e)
                self.stamp = time.time() - self.ttl + 60  # retry in a minute, not every render
                if self.value is None:
                    raise
        return self.value


from . import clock, weather, calendar, status, xkcd  # noqa: E402,F401  (registers widgets)
