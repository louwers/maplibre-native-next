"""For selected (src module -> dst module) edges, list which files include
which headers. Usage: python3 -I back_edges.py <repo-root>, or
`just include-back-edges`."""
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


pairs = {
    ('util', 'style'), ('util', 'tile'), ('util', 'map'), ('util', 'platform'), ('util', 'actor'),
    ('util', 'text'), ('util', 'storage'), ('util', 'gl'), ('util', 'style/layers'), ('util', 'geometry'),
    ('util', 'style/expression'),
    ('style/expression', 'style'), ('style/expression', 'tile'), ('style/expression', 'renderer'),
    ('style/expression', 'style/conversion'),
}
hits = defaultdict(lambda: defaultdict(set))  # pair -> header -> {files}
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
                    if m and (src_mod, module_of(m.group(1))) in pairs:
                        hits[(src_mod, module_of(m.group(1)))][m.group(1)].add(base.split('/')[0] + '/' + rel)

for pair in sorted(hits):
    print(f"== {pair[0]} -> {pair[1]}")
    for header, srcs in sorted(hits[pair].items()):
        print(f"   {header}  <=  {', '.join(sorted(s.replace('include/', 'inc:').replace('src/', 'src:') for s in srcs))}")
