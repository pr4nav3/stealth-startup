"""Phone + presence labels with temporal hysteresis. Gaze is paused."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass, field
from typing import Deque, Optional


@dataclass
class Hysteresis:
    """k-of-n to turn on; clear streak to turn off."""

    window: int
    on_threshold: int
    off_clear: int
    _history: Deque[bool] = field(default_factory=deque)
    _state: bool = False
    _clear_streak: int = 0

    def __post_init__(self) -> None:
        self._history = deque(maxlen=self.window)

    def update(self, raw: bool) -> bool:
        self._history.append(raw)
        if self._state:
            if raw:
                self._clear_streak = 0
            else:
                self._clear_streak += 1
                if self._clear_streak >= self.off_clear:
                    self._state = False
                    self._clear_streak = 0
            return self._state

        hits = sum(1 for v in self._history if v)
        if len(self._history) >= self.on_threshold and hits >= self.on_threshold:
            self._state = True
            self._clear_streak = 0
        return self._state

    def reset(self) -> None:
        self._history.clear()
        self._state = False
        self._clear_streak = 0


@dataclass
class TrackerState:
    phone: Hysteresis
    presence: Hysteresis

    @classmethod
    def default(cls) -> "TrackerState":
        return cls(
            phone=Hysteresis(window=5, on_threshold=3, off_clear=2),
            presence=Hysteresis(window=5, on_threshold=2, off_clear=4),
        )


def raw_present(face_in_frame: bool, person_in_frame: bool) -> bool:
    return face_in_frame or person_in_frame


def compose_frame(
    state: TrackerState,
    *,
    face_in_frame: bool,
    person_in_frame: bool,
    phone_raw: bool,
    phone_count: int = 0,
    phone_confidence: float = 0.0,
) -> dict:
    present_raw = raw_present(face_in_frame, person_in_frame)
    phone_on = state.phone.update(phone_raw)
    present = state.presence.update(present_raw)

    return {
        "face_in_frame": face_in_frame,
        "person_in_frame": person_in_frame,
        "present": present,
        "gaze": None,
        "phone_in_frame": phone_on,
        "looking_at_phone": phone_on,
        "phone_count": int(phone_count),
        "phone_confidence": float(phone_confidence),
    }


def phone_alert(phone_use: bool, strict: bool, phone_in_frame: bool, looking_at_phone: bool) -> bool:
    if not phone_use:
        return False
    if strict:
        return phone_in_frame
    return looking_at_phone
