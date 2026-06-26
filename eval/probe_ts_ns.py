#!/usr/bin/env python3
"""Reproduce the tree-sitter namespace handling on a mat.hpp-shaped snippet.

We want to know: when `namespace cv` and the opening `{` are on separate lines
(as in opencv's mat.hpp:59-60), does tree-sitter still expose the namespace's
`name` field correctly, or does the parser bail somehow?
"""
from __future__ import annotations

import sys

try:
    import tree_sitter_cpp as ts_cpp
    from tree_sitter import Language, Parser
except ImportError:
    sys.exit("Install: py -3 -m pip install tree-sitter tree-sitter-cpp")

CPP_LANGUAGE = Language(ts_cpp.language())
parser = Parser(CPP_LANGUAGE)


def dump(name: str, source: str) -> None:
    tree = parser.parse(source.encode("utf-8"))
    print(f"\n=== {name} ===")
    walk(tree.root_node, source.encode("utf-8"), 0)


def walk(node, src: bytes, depth: int):
    label = ""
    if node.type == "namespace_definition":
        name_node = node.child_by_field_name("name")
        body_node = node.child_by_field_name("body")
        nm = src[name_node.start_byte : name_node.end_byte].decode("utf-8") if name_node else "<NONE>"
        label = f"  name={nm!r} body={'YES' if body_node else 'NO'}"
    elif node.type in ("class_specifier", "struct_specifier"):
        name_node = node.child_by_field_name("name")
        nm = src[name_node.start_byte : name_node.end_byte].decode("utf-8") if name_node else "<NONE>"
        label = f"  name={nm!r}"
    if node.is_named:
        print("  " * depth + f"{node.type}{label} [{node.start_point}-{node.end_point}]")
    if depth < 6:
        for child in node.children:
            walk(child, src, depth + 1)


SAMPLE_INLINE = """\
namespace cv {
class CV_EXPORTS Mat { public: Mat(); };
}
"""

SAMPLE_SPLIT = """\
namespace cv
{
class CV_EXPORTS Mat { public: Mat(); };
}
"""

SAMPLE_MAT_HPP_LIKE = """\
namespace cv
{
enum AccessFlag { READ=1, WRITE=2 };

class CV_EXPORTS _InputArray
{
public:
    _InputArray();
};

template<typename _Tp> class Mat_
{
public:
    Mat_();
};

class CV_EXPORTS Mat
{
public:
    Mat();
};
}
"""

if __name__ == "__main__":
    dump("inline brace", SAMPLE_INLINE)
    dump("split brace", SAMPLE_SPLIT)
    dump("mat.hpp-like", SAMPLE_MAT_HPP_LIKE)
