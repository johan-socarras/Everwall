#!/usr/bin/env python3
"""Writes THIRD-PARTY-LICENSES.txt: the license of every library compiled
into Everwall on Windows and on Linux, the Rust standard library, and
Microsoft's WebView2Loader.dll, which the Windows package ships next to the
exe. The release packages carry the file, as the MIT, BSD, Apache and similar
licenses ask of a binary.

Run from the repository root after Cargo.lock changes or a new Rust release:
    python3 tools/third-party-licenses.py
It reads `cargo metadata` and `cargo tree` (the crates must be in the cargo
cache, `cargo fetch` puts them there), each crate's own license files, and
the notices of the standard library that ship with the toolchain.

The texts that come with no crate are kept in tools/licenses/:
  WebView2.txt                   LICENSE.txt of the Microsoft.Web.WebView2
                                 1.0.3650.58 NuGet package, the SDK whose
                                 loader webview2-com-sys bundles
  COPYING.MinGW-w64-runtime.txt  COPYING.MinGW-w64-runtime/ and
  COPYING.winpthreads.txt        mingw-w64-libraries/winpthreads/COPYING of
                                 github.com/mingw-w64/mingw-w64 (master,
                                 f0a4580, 2026-11-10)
"""
import html
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TAURI = os.path.join(ROOT, 'src-tauri')
OUT = os.path.join(ROOT, 'THIRD-PARTY-LICENSES.txt')
KEPT = os.path.join(ROOT, 'tools', 'licenses')
TARGETS = ['x86_64-pc-windows-gnu', 'x86_64-unknown-linux-gnu']
# for "A OR B" the first of these that is offered
PREFERENCE = ['Apache-2.0', 'MIT', 'BSD-3-Clause', 'ISC', 'Zlib', 'MIT-0', '0BSD', 'Unlicense', 'CC0-1.0']
SHARED = {'Apache-2.0', 'MPL-2.0'}            # one copy of the text is enough
NO_NOTICE = {'MIT-0', '0BSD', 'Unlicense', 'CC0-1.0'}  # ask for no notice at all
SIGNATURES = {
    'MIT': 'permission is hereby granted, free of charge',
    'BSD-3-Clause': 'redistribution and use in source and binary forms',
    'ISC': 'permission to use, copy, modify, and/or distribute',
    'Zlib': "this software is provided 'as-is'",
    'Unicode-3.0': 'unicode',
    'Apache-2.0': 'apache license',
    'MPL-2.0': 'mozilla public license',
}
MIT_TEMPLATE = """Copyright (c) {holders}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."""
BSD3_TEMPLATE = """Copyright (c) {holders}

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE."""
LINE = '-' * 78


def kept(name):
    """One of the texts in tools/licenses/, without trailing spaces."""
    with open(os.path.join(KEPT, name), encoding='utf-8') as f:
        return '\n'.join(line.rstrip() for line in f.read().strip().splitlines())


def cargo(*args):
    return subprocess.run(['cargo', *args], cwd=TAURI, check=True, capture_output=True, text=True).stdout


def crates():
    """Every crate compiled into the program, on any of the targets."""
    found = {}
    for target in TARGETS:
        meta = json.loads(cargo('metadata', '--format-version', '1', '--locked', '--filter-platform', target))
        by_id = {(p['name'], p['version']): p for p in meta['packages']}
        tree = cargo('tree', '--locked', '--target', target, '-e', 'normal', '--prefix', 'none', '--format', '{p}')
        for line in tree.splitlines():
            parts = line.split()
            if len(parts) >= 2 and parts[1].startswith('v'):
                key = (parts[0], parts[1][1:])
                if key in by_id and key[0] != 'everwall':
                    found[key] = by_id[key]
    return [found[k] for k in sorted(found)]


def standard_library():
    """The parts of the Rust standard library that are not under Apache-2.0
    OR MIT, from the toolchain's COPYRIGHT-library.html: on these targets
    only core's Unicode tables. Returns their copyright years."""
    sysroot = subprocess.run(['rustc', '--print', 'sysroot'], check=True, capture_output=True, text=True).stdout.strip()
    path = os.path.join(sysroot, 'share', 'doc', 'rust', 'COPYRIGHT-library.html')
    if not os.path.isfile(path):
        sys.exit(f'{path} is missing: this Rust toolchain does not ship the notices of its standard library')
    page = open(path, encoding='utf-8').read()
    start = re.search(r'<h2[^>]*>\s*In-tree files\s*</h2>', page)
    end = re.search(r'<h2[^>]*>\s*Out-of-tree dependencies\s*</h2>', page)
    text = html.unescape(re.sub(r'<[^>]+>', ' ', page[start.end():end.start()]))
    years = None
    for entry in text.split('File/Directory:')[1:]:
        where = entry.split()[0]
        license_id = re.search(r'License:\s*(.+?)\s+Copyright:', entry, re.S).group(1)
        if license_id == 'Apache-2.0 OR MIT' or 'fuchsia' in where:   # Fuchsia code is not built for these targets
            continue
        if license_id == 'Unicode-3.0' and where == 'library/core/src/unicode':
            years = re.search(r'Copyright:\s*([\d\-]+) Unicode', entry).group(1)
            continue
        sys.exit(f'the standard library now has {where} under {license_id}: add its notice to this script')
    if years is None:
        sys.exit("no copyright for core's Unicode tables in " + path)
    return years


def chosen(expression):
    """The licenses that apply, picking one where the crate offers a choice."""
    expression = expression.replace('/', ' OR ').replace(' WITH LLVM-exception', '')
    licenses = []
    for part in re.split(r'\s+AND\s+', expression):
        options = [o.strip('() ') for o in re.split(r'\s+OR\s+', part.strip('() '))]
        licenses.append(next((p for p in PREFERENCE if p in options), options[0]))
    return licenses


def license_files(package):
    folder = os.path.dirname(package['manifest_path'])
    names = sorted(n for n in os.listdir(folder)
                   if re.match(r'(licen[cs]e|copying|copyright)', n, re.I) and os.path.isfile(os.path.join(folder, n)))
    if package.get('license_file'):
        names.append(package['license_file'])
    return [(n, open(os.path.join(folder, n), encoding='utf-8', errors='replace').read()) for n in names]


def text_for(package, license_id):
    """The crate's own text for that license, or the standard one with its authors."""
    signature = SIGNATURES.get(license_id, license_id.lower())
    for name, text in license_files(package):
        if signature in text.lower() and not (license_id != 'Apache-2.0' and 'apache license' in text.lower()[:400]):
            return text.strip()
    holders = ', '.join(re.sub(r'\s*<[^>]*>', '', a) for a in package.get('authors') or []) or f"the {package['name']} authors"
    if license_id == 'MIT':
        return MIT_TEMPLATE.format(holders=holders)
    if license_id == 'BSD-3-Clause':
        return BSD3_TEMPLATE.format(holders=holders)
    sys.exit(f"no {license_id} text for {package['name']} {package['version']}")


def main():
    packages = crates()
    shared, own, free = {}, {}, []
    for p in packages:
        label = f"{p['name']} {p['version']}"
        for license_id in chosen(p.get('license') or ''):
            if license_id in NO_NOTICE:
                free.append(f'{label} ({license_id})')
            elif license_id in SHARED:
                shared.setdefault(license_id, {'users': [], 'text': None})
                shared[license_id]['users'].append(label)
                if shared[license_id]['text'] is None:
                    for name, text in license_files(p):
                        if SIGNATURES[license_id] in text.lower():
                            shared[license_id]['text'] = text.strip()
                            break
            else:
                text = text_for(p, license_id)
                own.setdefault((license_id, ' '.join(text.split())), {'text': text, 'users': []})['users'].append(label)
    for license_id, entry in shared.items():
        if entry['text'] is None:
            sys.exit(f'no {license_id} text in any crate')

    # The standard library is compiled into the program too: Apache-2.0 OR
    # MIT, and the Unicode license for core's tables, with their own years.
    shared['Apache-2.0']['users'].insert(0, 'the Rust standard library and the crates it is built from')
    # the plainest copy of the license (ICU4X's adds notes of its own)
    unicode = min((e['text'] for (license_id, _), e in own.items() if license_id == 'Unicode-3.0'), key=len, default='')
    unicode, found = re.subn(r'Copyright © [\d\-]+ Unicode, Inc\.', f'Copyright © {standard_library()} Unicode, Inc.', unicode, count=1)
    if not found:
        sys.exit('no Unicode-3.0 text to copy for the standard library')
    own.setdefault(('Unicode-3.0', ' '.join(unicode.split())), {'text': unicode, 'users': []})['users'].append(
        "the Rust standard library (core's Unicode tables)")

    out = ['Third-party software in Everwall', '=' * 32, '',
           'The Everwall packages contain the libraries listed below and the Rust',
           'standard library, compiled into the program. The Windows package also',
           'contains parts of the MinGW-w64 runtime, linked into Everwall.exe, and',
           'ships Microsoft\'s WebView2Loader.dll. Each one is used under the license',
           'shown. Everwall itself is free software under the GNU General Public',
           'License v3.0, in the LICENSE file next to this one.',
           '',
           f'{len(packages)} libraries. This file is generated from Cargo.lock and the',
           'Rust toolchain by tools/third-party-licenses.py.', '', LINE,
           'WebView2Loader.dll (Windows package only), from the Microsoft Edge WebView2',
           'SDK, as bundled by the webview2-com-sys crate', LINE, '', kept('WebView2.txt'), '',
           LINE, 'MinGW-w64 runtime (Windows package only)', LINE, '',
           "Rust's GNU toolchain for Windows links Everwall.exe statically against the",
           'MinGW-w64 runtime and its winpthreads library, whose notices follow, and',
           "against GCC's libgcc, which is under the GCC Runtime Library Exception and",
           'asks for no notice.', '',
           kept('COPYING.MinGW-w64-runtime.txt'), '',
           '===========', 'winpthreads', '===========', '',
           kept('COPYING.winpthreads.txt'), '']
    for license_id in sorted(shared):
        entry = shared[license_id]
        out += [LINE, f'{license_id}, used by:', LINE, *wrap(entry['users']), '']
        if license_id == 'MPL-2.0':
            out += ['The source code of these libraries is published on crates.io:',
                    *[f"  https://crates.io/crates/{u.split()[0]}/{u.split()[1]}" for u in entry['users']], '']
        out += [entry['text'], '']
    for (license_id, _), entry in sorted(own.items(), key=lambda item: (item[0][0], item[1]['users'][0])):
        out += [LINE, f'{license_id}, used by:', LINE, *wrap(entry['users']), '', entry['text'], '']
    if free:
        out += [LINE, 'Under licenses that ask for no notice:', LINE, *wrap(free), '']
    with open(OUT, 'w', encoding='utf-8', newline='\n') as f:
        f.write('\n'.join(out).rstrip() + '\n')
    print(f'{OUT}: {len(packages)} libraries, {len(own)} own license texts, {len(shared)} shared ones')


def wrap(items, width=78):
    lines, line = [], ''
    for item in items:
        piece = item if not line else f', {item}'
        if len(line) + len(piece) > width - 1:   # room for the comma
            lines.append(line + ',')
            line = item
        else:
            line += piece
    return lines + ([line] if line else [])


if __name__ == '__main__':
    main()
