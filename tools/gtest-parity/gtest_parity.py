"""Parity between the C++ gtests (../maplibre-native/test/**) and their Rust ports.

Every Rust test that ports a C++ gtest carries a marker comment directly above
its `#[test]` (or above the attributes in front of it):

    // gtest: Group.Name
    #[test]
    fn name() { ... }

`Group.Name` is the C++ test suite and test name as in
`TEST(Group, Name)` / `TEST_F(Group, Name)` / `TEST_P(Group, Name)`
(macro-wrapped names such as `TEST_REQUIRES_SERVER(Name)` count as `Name`).
One Rust test may carry several markers; a C++ test counts as ported when at
least one of its markers is on a test without `#[ignore]`, and as ignored when
all of its markers are on `#[ignore]`d tests. A test the C++ CI does not run
either (`DISABLED_*`, `TEST_REQUIRES_ACCURATE_TIMING(...)`,
`TEST_DISABLED_ON_CI(...)`) counts as ported when its port is `#[ignore]`d.

Usage (from anywhere; `just gtest-parity [options]` runs it too):
    python3 -I tools/gtest-parity/gtest_parity.py              # table per test/ dir
    python3 -I tools/gtest-parity/gtest_parity.py --missing util
    python3 -I tools/gtest-parity/gtest_parity.py --ignored util  # with reasons
    python3 -I tools/gtest-parity/gtest_parity.py --ported util   # marker locations
Options: --cpp <path to maplibre-native> (default: ../maplibre-native next to
this repo), --rust <path to this repo>.
"""
import argparse
import os
import re
import sys
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))

CPP_TEST_RE = re.compile(
    r'^\s*(?:TEST|TEST_F|TEST_P)\s*\(\s*(\w+)\s*,\s*(?:(\w+)\s*\(\s*(\w+)\s*\)|(\w+))\s*\)')
# Wrappers that make the test DISABLED_ on the C++ CI (test/include/mln/test/util.hpp).
CI_DISABLED_WRAPPERS = {'TEST_REQUIRES_ACCURATE_TIMING', 'TEST_DISABLED_ON_CI'}
MARKER_RE = re.compile(r'^\s*//\s*gtest:\s*(\w+)\.(\w+)\s*$')
IGNORE_RE = re.compile(r'#\[ignore(?:\s*=\s*"((?:[^"\\]|\\.)*)")?\s*\]')
# Directories under the repo that never hold sources (Bazel convenience links).
SKIP_DIRS = {'.git', 'target', 'node_modules'}


CI_DISABLED = set()  # (group, name) of tests the C++ CI does not run


def cpp_tests(cpp_root):
    """Return {dir: {(group, name): 'file:line'}} for test/<dir>/**."""
    test_root = os.path.join(cpp_root, 'test')
    result = defaultdict(dict)
    for dirpath, dirnames, files in os.walk(test_root):
        dirnames.sort()
        for f in sorted(files):
            if not f.endswith(('.cpp', '.mm', '.hpp')):
                continue
            path = os.path.join(dirpath, f)
            rel = os.path.relpath(path, test_root)
            top = rel.split(os.sep)[0] if os.sep in rel else '(top)'
            with open(path, encoding='utf-8', errors='replace') as fh:
                for lineno, line in enumerate(fh, 1):
                    m = CPP_TEST_RE.match(line)
                    if m:
                        name = m.group(3) or m.group(4)
                        result[top].setdefault((m.group(1), name), f'test/{rel}:{lineno}')
                        if m.group(2) in CI_DISABLED_WRAPPERS or name.startswith('DISABLED_'):
                            CI_DISABLED.add((m.group(1), name))
    return result


def rust_markers(rust_root):
    """Return {(group, name): [(path:line, ignore_reason or None)]}.

    ignore_reason is '' for a bare #[ignore]."""
    markers = defaultdict(list)
    for dirpath, dirnames, files in os.walk(rust_root):
        dirnames[:] = sorted(d for d in dirnames
                             if d not in SKIP_DIRS and not d.startswith('bazel-'))
        for f in sorted(files):
            if not f.endswith('.rs'):
                continue
            path = os.path.join(dirpath, f)
            with open(path, encoding='utf-8', errors='replace') as fh:
                lines = fh.readlines()
            for i, line in enumerate(lines):
                m = MARKER_RE.match(line)
                if not m:
                    continue
                # Scan the attribute/comment block below the marker up to the fn.
                ignore = None
                is_test = False
                j = i + 1
                while j < len(lines):
                    s = lines[j].strip()
                    if s.startswith('#['):
                        # Attributes may span lines (#[ignore = "..."]).
                        attr = s
                        while attr.count('[') > attr.count(']') and j + 1 < len(lines):
                            j += 1
                            attr += ' ' + lines[j].strip()
                        if re.match(r'#\[(?:\w+::)*test\b', attr):
                            is_test = True
                        im = IGNORE_RE.search(attr)
                        if im:
                            ignore = im.group(1) if im.group(1) is not None else ''
                    elif s.startswith('//') or s == '':
                        pass
                    else:
                        break
                    j += 1
                rel = os.path.relpath(path, rust_root)
                markers[(m.group(1), m.group(2))].append(
                    (f'{rel}:{i + 1}', ignore, is_test))
    return markers


def classify(key, markers):
    entries = markers.get(key)
    if not entries:
        return 'missing'
    # Tests the C++ CI doesn't run either (`DISABLED_*`,
    # TEST_REQUIRES_ACCURATE_TIMING, TEST_DISABLED_ON_CI): an #[ignore]d port
    # is a complete port.
    if key in CI_DISABLED or any(ign is None for _, ign, _ in entries):
        return 'ported'
    return 'ignored'


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--cpp', default=os.path.join(os.path.dirname(REPO), 'maplibre-native'))
    ap.add_argument('--rust', default=REPO)
    ap.add_argument('--missing', metavar='DIR', help='list the missing tests of test/DIR')
    ap.add_argument('--ignored', metavar='DIR', help='list the ignored tests of test/DIR')
    ap.add_argument('--ported', metavar='DIR', help='list the ported tests of test/DIR')
    args = ap.parse_args()

    tests = cpp_tests(args.cpp)
    markers = rust_markers(args.rust)

    listing = args.missing or args.ignored or args.ported
    if listing:
        if listing not in tests:
            sys.exit(f'no C++ tests under test/{listing}; dirs: {", ".join(sorted(tests))}')
        want = 'missing' if args.missing else 'ignored' if args.ignored else 'ported'
        for key, loc in tests[listing].items():
            if classify(key, markers) != want:
                continue
            line = f'{key[0]}.{key[1]}  ({loc})'
            if want == 'ignored':
                reasons = sorted({ign for _, ign, _ in markers[key]})
                line += '  ' + ' | '.join(r or '#[ignore]' for r in reasons)
            elif want == 'ported':
                line += '  ' + ', '.join(p for p, _, _ in markers[key])
            print(line)
        return

    rows = []
    totals = [0, 0, 0, 0]
    for d in sorted(tests):
        counts = {'ported': 0, 'ignored': 0, 'missing': 0}
        for key in tests[d]:
            counts[classify(key, markers)] += 1
        row = [len(tests[d]), counts['ported'], counts['ignored'], counts['missing']]
        rows.append((d, row))
        totals = [a + b for a, b in zip(totals, row)]
    print(f'{"dir":<12}{"total":>7}{"ported":>8}{"ignored":>9}{"missing":>9}')
    for d, (t, p, i, m) in rows + [('ALL', totals)]:
        print(f'{d:<12}{t:>7}{p:>8}{i:>9}{m:>9}')

    known = {key for d in tests.values() for key in d}
    unknown = sorted(k for k in markers if k not in known)
    not_tests = sorted((k, p) for k, entries in markers.items()
                       for p, _, is_test in entries if not is_test)
    if unknown:
        print('\nmarkers without a C++ test (typo or renamed?):')
        for k in unknown:
            print(f'  {k[0]}.{k[1]}  ({", ".join(p for p, _, _ in markers[k])})')
    if not_tests:
        print('\nmarkers not directly above a #[test]:')
        for k, p in not_tests:
            print(f'  {k[0]}.{k[1]}  ({p})')


if __name__ == '__main__':
    main()
