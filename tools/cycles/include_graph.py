"""Module-level include graph of src/mln + include/mln, with SCCs.

Usage: python3 -I include_graph.py <repo-root>, or `just include-graph`.
A "module" is the first directory under mln/ (style, tile, ...); for style we
also split out style/expression and style/conversion as their own nodes,
since those are candidate crates.
"""
import os
import re
import sys
from collections import defaultdict

root = sys.argv[1]
inc_re = re.compile(r'#include\s*<mln/([^>]+)>')

def module_of(rel):
    parts = rel.split('/')
    if len(parts) == 1:
        return '(top)'
    if parts[0] == 'style' and len(parts) > 2 and parts[1] in ('expression', 'conversion', 'layers', 'sources'):
        return 'style/' + parts[1]
    return parts[0]

edges = defaultdict(lambda: defaultdict(int))
for base in ('src/mln', 'include/mln'):
    for dirpath, _, files in os.walk(os.path.join(root, base)):
        for f in files:
            if not f.endswith(('.hpp', '.cpp', '.h')):
                continue
            path = os.path.join(dirpath, f)
            rel = os.path.relpath(path, os.path.join(root, base))
            src_mod = module_of(rel)
            with open(path, errors='replace') as fh:
                for line in fh:
                    m = inc_re.search(line)
                    if m:
                        dst_mod = module_of(m.group(1))
                        if dst_mod != src_mod:
                            edges[src_mod][dst_mod] += 1

nodes = set(edges) | {d for s in edges for d in edges[s]}

# Tarjan SCC
index = {}
low = {}
stack = []
on = set()
sccs = []
counter = [0]
sys.setrecursionlimit(10000)

def strong(v):
    index[v] = low[v] = counter[0]
    counter[0] += 1
    stack.append(v)
    on.add(v)
    for w in edges.get(v, {}):
        if w not in index:
            strong(w)
            low[v] = min(low[v], low[w])
        elif w in on:
            low[v] = min(low[v], index[w])
    if low[v] == index[v]:
        comp = []
        while True:
            w = stack.pop()
            on.discard(w)
            comp.append(w)
            if w == v:
                break
        sccs.append(sorted(comp))

for v in sorted(nodes):
    if v not in index:
        strong(v)

print("Strongly connected components (size > 1 = include cycle):")
for c in sccs:
    if len(c) > 1:
        print(f"  CYCLE ({len(c)}): {', '.join(c)}")
print("Acyclic singletons:", ', '.join(sorted(c[0] for c in sccs if len(c) == 1)))

def show(a, b):
    print(f"  {a} -> {b}: {edges[a].get(b, 0)} includes;  {b} -> {a}: {edges[b].get(a, 0)} includes")

print("\nSelected back-edges:")
for a, b in [('util', 'style'), ('util', 'tile'), ('util', 'renderer'), ('style/expression', 'style'),
             ('style/expression', 'tile'), ('style/expression', 'style/conversion'), ('style', 'renderer'),
             ('tile', 'renderer'), ('tile', 'style'), ('gfx', 'renderer'), ('gfx', 'style'),
             ('text', 'style'), ('text', 'renderer'), ('storage', 'style'), ('actor', 'util')]:
    show(a, b)

print("\nOut-edges of util (who util depends on):", dict(edges['util']))
print("Out-edges of style/expression:", dict(edges['style/expression']))
print("Out-edges of actor:", dict(edges['actor']))
print("Out-edges of storage:", dict(edges['storage']))
print("Out-edges of gfx:", dict(edges['gfx']))
