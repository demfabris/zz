#!/usr/bin/env python3

import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "layout_v1", pathlib.Path(__file__).with_name("layout-v1.py")
)
layout_v1 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(layout_v1)

TILED = [
    (
        '{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":37,"h":24,"x":0,"y":0,"l":1,"i":0,"I":"%0"},{"t":"v","w":42,"h":24,"x":38,"y":0,"c":[{"t":"p","w":42,"h":12,"x":38,"y":0,"l":0,"i":1,"I":"%1"},{"t":"h","w":42,"h":11,"x":38,"y":13,"c":[{"t":"p","w":21,"h":11,"x":38,"y":13,"a":true,"i":2,"I":"%2"},{"t":"p","w":20,"h":11,"x":60,"y":13,"i":3,"I":"%3"}]}]}]}}',
        "154e,80x24,0,0{37x24,0,0,0,42x24,38,0[42x12,38,0,1,42x11,38,13{21x11,38,13,2,20x11,60,13,3}]}",
    ),
    (
        '{"V":2,"L":{"t":"v","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":80,"h":12,"x":0,"y":0,"a":true,"i":0,"I":"%4"},{"t":"p","w":80,"h":11,"x":0,"y":13,"l":0,"i":1,"I":"%5"}]}}',
        "c1a7,80x24,0,0[80x12,0,0,4,80x11,0,13,5]",
    ),
    (
        '{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"a":true,"i":0,"I":"%0"}}',
        "b25d,80x24,0,0,0",
    ),
]

FLOATING = [
    (
        '{"V":2,"L":{"t":"v","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":80,"h":24,"x":0,"y":0,"a":true,"i":0,"I":"%0"},{"t":"p","w":18,"h":4,"x":6,"y":5,"i":1,"z":0,"I":"%1"}]}}',
        "b25d,80x24,0,0,0",
    ),
    (
        '{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"l":0,"i":0,"I":"%2"},{"t":"p","w":39,"h":24,"x":41,"y":0,"a":true,"i":1,"I":"%3"},{"t":"p","w":8,"h":3,"x":51,"y":3,"i":2,"z":0,"I":"%4"}]}}',
        "820e,80x24,0,0{40x24,0,0,2,39x24,41,0,3}",
    ),
    (
        '{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"l":0,"i":0,"I":"%0"},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":24,"x":41,"y":0,"a":true,"i":1,"I":"%2"},{"t":"p","w":8,"h":3,"x":51,"y":16,"i":2,"z":0,"I":"%3"}]}]}}',
        "0206,80x24,0,0{40x24,0,0,0,39x24,41,0,2}",
    ),
    (
        '{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"l":1,"i":0,"I":"%4"},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":12,"x":41,"y":0,"l":0,"i":1,"I":"%5"},{"t":"p","w":8,"h":3,"x":46,"y":3,"i":3,"z":1,"I":"%7"},{"t":"p","w":39,"h":11,"x":41,"y":13,"a":true,"i":2,"I":"%6"},{"t":"p","w":8,"h":3,"x":46,"y":16,"i":4,"z":0,"I":"%8"}]}]}}',
        "da83,80x24,0,0{40x24,0,0,4,39x24,41,0[39x12,41,0,5,39x11,41,13,6]}",
    ),
]


class LayoutV1Test(unittest.TestCase):
    def test_tiled_layouts_match_the_old_pin_v1_output(self):
        for layout, expected in TILED:
            with self.subTest(expected=expected):
                self.assertEqual(layout_v1.v1(layout), expected)

    def test_floating_cells_drop_and_singleton_parents_collapse_like_tmux(self):
        for layout, expected in FLOATING:
            with self.subTest(expected=expected):
                self.assertEqual(layout_v1.v1(layout), expected)

    def test_v1_strings_pass_through_and_an_all_floating_tree_dumps_nothing(self):
        self.assertEqual(layout_v1.v1("b25d,80x24,0,0,0"), "b25d,80x24,0,0,0")
        self.assertEqual(
            layout_v1.v1('{"V":2,"L":{"t":"p","w":8,"h":3,"x":4,"y":2,"i":0,"z":0,"I":"%1"}}'),
            "",
        )


if __name__ == "__main__":
    unittest.main()
