#!/usr/bin/env python3
"""Runnable check for gaze / phone composition. No camera, no models."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from labels import AT_PC, AT_PHONE, AWAY_FROM_PC, classify, looking_at_pc, phone_alert


def frontal():
    return (40.0, 40.0), (80.0, 40.0), (60.0, 65.0), (48.0, 90.0), (72.0, 90.0)


class LookingAtPcTests(unittest.TestCase):
    def test_frontal_face_is_at_pc(self):
        self.assertTrue(looking_at_pc(*frontal()))

    def test_realistic_webcam_face_is_at_pc(self):
        # Typical laptop-cam proportions. solvePnP Euler on these was ~-174
        # pitch, which made every screen-looking pose "away."
        self.assertTrue(
            looking_at_pc(
                (250.0, 140.0),
                (320.0, 140.0),
                (285.0, 175.0),
                (262.0, 210.0),
                (308.0, 210.0),
            )
        )

    def test_screen_like_face_is_at_pc(self):
        # Webcam is above the screen; a slight nod is still at-PC.
        right_eye, left_eye, _, right_mouth, left_mouth = frontal()
        nose = (60.0, 68.0)
        self.assertTrue(
            looking_at_pc(right_eye, left_eye, nose, right_mouth, left_mouth)
        )

    def test_head_down_is_not_at_pc(self):
        right_eye, left_eye, _, right_mouth, left_mouth = frontal()
        nose = (60.0, 82.0)
        self.assertFalse(
            looking_at_pc(right_eye, left_eye, nose, right_mouth, left_mouth)
        )

    def test_head_up_is_not_at_pc(self):
        right_eye, left_eye, _, right_mouth, left_mouth = frontal()
        nose = (60.0, 48.0)
        self.assertFalse(
            looking_at_pc(right_eye, left_eye, nose, right_mouth, left_mouth)
        )

    def test_head_turned_is_not_at_pc(self):
        right_eye, left_eye, _, right_mouth, left_mouth = frontal()
        nose = (38.0, 65.0)
        self.assertFalse(
            looking_at_pc(right_eye, left_eye, nose, right_mouth, left_mouth)
        )


class ClassifyTests(unittest.TestCase):
    def test_no_face(self):
        out = classify(False, False, False)
        self.assertEqual(
            out,
            {
                "face_in_frame": False,
                "gaze": None,
                "phone_in_frame": False,
                "looking_at_phone": False,
            },
        )

    def test_at_pc_with_desk_phone_is_not_looking_at_phone(self):
        out = classify(True, True, True)
        self.assertEqual(out["gaze"], AT_PC)
        self.assertTrue(out["phone_in_frame"])
        self.assertFalse(out["looking_at_phone"])

    def test_looking_down_without_phone_is_away(self):
        out = classify(True, False, False)
        self.assertEqual(out["gaze"], AWAY_FROM_PC)
        self.assertFalse(out["looking_at_phone"])

    def test_looking_down_with_phone_is_at_phone(self):
        out = classify(True, False, True)
        self.assertEqual(out["gaze"], AT_PHONE)
        self.assertTrue(out["looking_at_phone"])


class PhoneAlertTests(unittest.TestCase):
    def test_off_never_fires(self):
        self.assertFalse(phone_alert(False, True, True, True))

    def test_not_strict_needs_looking_at_phone(self):
        self.assertFalse(phone_alert(True, False, True, False))
        self.assertTrue(phone_alert(True, False, True, True))

    def test_strict_fires_on_phone_in_frame(self):
        self.assertTrue(phone_alert(True, True, True, False))
        self.assertFalse(phone_alert(True, True, False, False))


if __name__ == "__main__":
    unittest.main()
