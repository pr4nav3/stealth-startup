"""Compose webcam labels from a face pose and a phone flag.

Looking-at-phone requires a visible phone. Head-down with no phone is
away-from-PC.

YuNet's 5 points are image-space. A 2D yaw/pitch ratio matches them.
solvePnP Euler angles do not: a normal screen-facing face comes back
around pitch -170, so every at-PC pose looked like "away."
"""

from __future__ import annotations

from typing import Optional, Tuple

Point = Tuple[float, float]

AT_PC = "at_pc"
AWAY_FROM_PC = "away_from_pc"
AT_PHONE = "at_phone"

# YuNet landmarks are in pixels. Ratios are scale-free.
# Yaw: nose vs eye midpoint, in units of inter-ocular distance.
# Pitch: how far the nose sits between the eyes and the mouth (0 = up, 1 = down).
YAW_LIMIT = 0.45
# Screen-looking from a top webcam is roughly 0.45–0.58. The old 0.2–0.8
# window treated a full nod as still at-PC. Tighten only the nod, not the screen.
PITCH_MIN = 0.28
PITCH_MAX = 0.64


def _mid(a: Point, b: Point) -> Point:
    return ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0)


def face_yaw_pitch(
    right_eye: Point,
    left_eye: Point,
    nose: Point,
    right_mouth: Point,
    left_mouth: Point,
) -> Optional[Tuple[float, float]]:
    eye = _mid(right_eye, left_eye)
    mouth = _mid(right_mouth, left_mouth)
    iod = ((left_eye[0] - right_eye[0]) ** 2 + (left_eye[1] - right_eye[1]) ** 2) ** 0.5
    if iod < 1.0:
        return None
    span = mouth[1] - eye[1]
    if abs(span) < 1.0:
        return None
    yaw = (nose[0] - eye[0]) / iod
    pitch = (nose[1] - eye[1]) / span
    return yaw, pitch


def looking_at_pc(
    right_eye: Point,
    left_eye: Point,
    nose: Point,
    right_mouth: Point,
    left_mouth: Point,
) -> bool:
    pose = face_yaw_pitch(right_eye, left_eye, nose, right_mouth, left_mouth)
    if pose is None:
        return False
    yaw, pitch = pose
    return abs(yaw) <= YAW_LIMIT and PITCH_MIN <= pitch <= PITCH_MAX


def classify(
    face_in_frame: bool,
    at_pc: bool,
    phone_in_frame: bool,
) -> dict:
    if not face_in_frame:
        return {
            "face_in_frame": False,
            "gaze": None,
            "phone_in_frame": phone_in_frame,
            "looking_at_phone": False,
        }

    if at_pc:
        gaze: Optional[str] = AT_PC
    elif phone_in_frame:
        gaze = AT_PHONE
    else:
        gaze = AWAY_FROM_PC

    return {
        "face_in_frame": True,
        "gaze": gaze,
        "phone_in_frame": phone_in_frame,
        "looking_at_phone": gaze == AT_PHONE,
    }


def phone_alert(
    phone_use: bool,
    strict: bool,
    phone_in_frame: bool,
    looking_at_phone: bool,
) -> bool:
    if not phone_use:
        return False
    if strict:
        return phone_in_frame
    return looking_at_phone
