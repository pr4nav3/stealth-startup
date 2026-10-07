#!/usr/bin/env python3
"""Checks that do not need a model or a camera."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

from detectors import CELL_PHONE_CLASS, PERSON_CLASS, Box, nms_per_class, phone_views


class PhoneViewTests(unittest.TestCase):
    def test_wide_frame_adds_the_lower_center_crop(self):
        frame = np.zeros((100, 200, 3), dtype=np.uint8)
        views = phone_views(frame)
        self.assertEqual(len(views), 2)
        self.assertEqual(views[1][0].shape[:2], (100, 100))

    def test_square_frame_is_not_cropped_again(self):
        frame = np.zeros((80, 80, 3), dtype=np.uint8)
        self.assertEqual(len(phone_views(frame)), 1)


class NmsTests(unittest.TestCase):
    def test_overlapping_phone_and_person_both_survive(self):
        phone = Box(0, 0, 40, 40, 0.9, CELL_PHONE_CLASS)
        person = Box(0, 0, 40, 40, 0.8, PERSON_CLASS)
        kept = nms_per_class([phone, person])
        self.assertEqual({box.class_id for box in kept}, {CELL_PHONE_CLASS, PERSON_CLASS})


if __name__ == "__main__":
    unittest.main()
