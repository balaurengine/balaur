#!/usr/bin/env python3
"""Tests for pkg.py, the Mac App Store's flat package writer.

    python3 -I docker/pkg_test.py

Each package is read back by hand (the xar, its checksums, the Bom's tree,
the cpio payload) and, where macOS's own tools are on PATH, by them: lsbom
reads the Bom, pkgutil expands the package and lists its payload, installer
reads it as a product. Off a Mac those checks are skipped.
"""

import gzip
import hashlib
import importlib.util
import os
import plistlib
import shutil
import struct
import subprocess
import tempfile
import unittest
import zlib
from xml.etree import ElementTree

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location("pkg", os.path.join(HERE, "pkg.py"))
pkg = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pkg)

IDENTIFIER = "com.studio.t"
# A universal Mach-O header: two slices, x86_64 and arm64.
UNIVERSAL = struct.pack(">II", pkg.FAT_MAGIC, 2) + struct.pack(">IIIII", 0x01000007, 3, 0, 0, 0) + struct.pack(
    ">IIIII", 0x0100000C, 0, 0, 0, 0
)


def make_app(root, name="T", files=0, links=False):
    app = os.path.join(root, f"{name}.app")
    os.makedirs(os.path.join(app, "Contents", "MacOS"))
    os.makedirs(os.path.join(app, "Contents", "Resources"))
    with open(os.path.join(app, "Contents", "Info.plist"), "wb") as out:
        plistlib.dump(
            {
                "CFBundleIdentifier": IDENTIFIER,
                "CFBundleExecutable": name,
                "CFBundleName": name,
                "CFBundleShortVersionString": "1.2.3",
                "CFBundleVersion": "7",
                "LSMinimumSystemVersion": "11.0",
            },
            out,
        )
    executable = os.path.join(app, "Contents", "MacOS", name)
    with open(executable, "wb") as out:
        out.write(UNIVERSAL + b"\0" * 4000)
    os.chmod(executable, 0o700)
    with open(os.path.join(app, "Contents", "Resources", "game.bpak"), "wb") as out:
        out.write(b"pack")
    os.chmod(os.path.join(app, "Contents", "Resources", "game.bpak"), 0o600)
    if files:
        many = os.path.join(app, "Contents", "Resources", "many")
        os.makedirs(many)
        for i in range(files):
            with open(os.path.join(many, f"f{i:04}"), "w") as out:
                out.write(str(i))
    if links:
        versions = os.path.join(app, "Contents", "Frameworks", "X.framework", "Versions")
        os.makedirs(os.path.join(versions, "A"))
        with open(os.path.join(versions, "A", "X"), "wb") as out:
            out.write(b"lib")
        os.symlink("A", os.path.join(versions, "Current"))
        os.symlink("Versions/Current/X", os.path.join(versions, "..", "X"))
    return app


def read_xar(path):
    """{name: bytes} of every file, checking each checksum on the way."""
    with open(path, "rb") as archive:
        blob = archive.read()
    magic, size, version, packed, plain, algorithm = pkg.XAR_HEADER.unpack(blob[: pkg.XAR_HEADER.size])
    assert (magic, size, version, algorithm) == (b"xar!", 28, 1, 1), (magic, size, version, algorithm)
    toc_bytes = zlib.decompress(blob[size : size + packed])
    assert len(toc_bytes) == plain
    heap = size + packed
    assert blob[heap : heap + 20] == hashlib.sha1(blob[size : size + packed]).digest(), "TOC checksum"
    toc = ElementTree.fromstring(toc_bytes)
    files = {}

    def visit(element, prefix):
        for entry in element.findall("file"):
            name = prefix + entry.findtext("name")
            data = entry.find("data")
            if data is not None:
                at = heap + int(data.findtext("offset"))
                raw = blob[at : at + int(data.findtext("length"))]
                assert hashlib.sha1(raw).hexdigest() == data.findtext("archived-checksum"), name
                style = data.find("encoding").get("style")
                body = zlib.decompress(raw) if style == "application/x-gzip" else raw
                assert len(body) == int(data.findtext("size")), name
                assert hashlib.sha1(body).hexdigest() == data.findtext("extracted-checksum"), name
                files[name] = body
            visit(entry, name + "/")

    visit(toc.find("toc"), "")
    return files


def read_bom(blob):
    """[(id, parent id, name, type, mode, size, checksum, link)] in the tree's order."""
    assert blob[:8] == b"BOMStore"
    _version, blocks, index_offset, _index_length, vars_offset, _vars_length = struct.unpack(
        ">IIIIII", blob[8:32]
    )
    (count,) = struct.unpack(">I", blob[index_offset : index_offset + 4])
    pointers = [struct.unpack(">II", blob[index_offset + 4 + 8 * i : index_offset + 12 + 8 * i]) for i in range(count)]
    assert blocks == count - 1

    def block(index):
        address, length = pointers[index]
        return blob[address : address + length]

    (nvars,) = struct.unpack(">I", blob[vars_offset : vars_offset + 4])
    at = vars_offset + 4
    names = {}
    for _ in range(nvars):
        index, length = struct.unpack(">IB", blob[at : at + 5])
        names[blob[at + 5 : at + 5 + length].decode()] = index
        at += 5 + length
    assert set(names) == {"BomInfo", "Paths", "HLIndex", "VIndex", "Size64"}, names
    tree = block(names["Paths"])
    assert tree[:4] == b"tree"
    _v, child, block_size, path_count, _u = struct.unpack(">IIIIB", tree[4:21])
    assert block_size == 4096

    def leaves(index):
        node = block(index)
        leaf, n, _forward, _backward = struct.unpack(">HHII", node[:12])
        pairs = [struct.unpack(">II", node[12 + 8 * i : 20 + 8 * i]) for i in range(n)]
        if leaf:
            yield from pairs
        else:
            for child_index, _last in pairs:
                yield from leaves(child_index)

    found = []
    for info1, file in leaves(child):
        number, info2 = struct.unpack(">II", block(info1))
        raw = block(info2)
        kind, _u0, _arch, mode, _uid, _gid, _mtime, size, _u1, checksum, link_length = struct.unpack(
            ">BBHHIIIIBII", raw[:31]
        )
        link = raw[31 : 31 + link_length].rstrip(b"\0").decode()
        name = block(file)
        (parent,) = struct.unpack(">I", name[:4])
        found.append((number, parent, name[4:].rstrip(b"\0").decode(), kind, mode, size, checksum, link))
    assert len(found) == path_count
    return found


def read_cpio(blob):
    """{path: (mode, data)} of an odc cpio."""
    entries = {}
    at = 0
    while True:
        header = blob[at : at + 76].decode()
        assert header[:6] == "070707", header[:6]
        mode = int(header[18:24], 8)
        namesize = int(header[59:65], 8)
        filesize = int(header[65:76], 8)
        name = blob[at + 76 : at + 76 + namesize - 1].decode()
        data = blob[at + 76 + namesize : at + 76 + namesize + filesize]
        at += 76 + namesize + filesize
        if name == "TRAILER!!!":
            return entries
        entries[name] = (mode, data)


def tool(name):
    return shutil.which(name)


class PackageTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.dir)

    def build(self, **kwargs):
        app = make_app(self.dir, **kwargs)
        out = os.path.join(self.dir, "T.pkg")
        pkg.build(app, out)
        return app, out

    def test_a_product_archive_of_one_component_into_applications(self):
        _app, out = self.build()
        files = read_xar(out)
        component = f"{IDENTIFIER}.pkg"
        self.assertEqual(
            sorted(files),
            sorted(["Distribution", f"{component}/Bom", f"{component}/Payload", f"{component}/PackageInfo"]),
        )
        distribution = ElementTree.fromstring(files["Distribution"])
        bundle = distribution.find("pkg-ref/bundle-version/bundle")
        self.assertEqual(
            (bundle.get("id"), bundle.get("CFBundleShortVersionString"), bundle.get("CFBundleVersion"), bundle.get("path")),
            (IDENTIFIER, "1.2.3", "7", "T.app"),
        )
        self.assertEqual(distribution.find("options").get("hostArchitectures"), "arm64,x86_64")
        self.assertEqual(distribution.find("volume-check/allowed-os-versions/os-version").get("min"), "11.0")
        self.assertEqual(distribution.find("choice[@id='com.studio.t']").get("customLocation"), "/Applications")
        self.assertEqual([r.text for r in distribution.findall("pkg-ref") if (r.text or "").strip()], [f"#{component}"])
        info = ElementTree.fromstring(files[f"{component}/PackageInfo"])
        self.assertEqual(info.get("install-location"), "/Applications")
        self.assertEqual((info.get("identifier"), info.get("version")), (IDENTIFIER, "1.2.3"))
        self.assertEqual(info.find("payload").get("numberOfFiles"), "7")
        self.assertEqual(info.find("bundle").get("path"), "./T.app")

    def test_the_payload_is_the_bundle_rooted_at_dot_world_readable(self):
        app, out = self.build()
        payload = read_cpio(gzip.decompress(read_xar(out)[f"{IDENTIFIER}.pkg/Payload"]))
        self.assertEqual(
            sorted(payload),
            [
                ".",
                "./T.app",
                "./T.app/Contents",
                "./T.app/Contents/Info.plist",
                "./T.app/Contents/MacOS",
                "./T.app/Contents/MacOS/T",
                "./T.app/Contents/Resources",
                "./T.app/Contents/Resources/game.bpak",
            ],
        )
        self.assertEqual(payload["./T.app/Contents/MacOS/T"][0], 0o100755)
        self.assertEqual(payload["./T.app/Contents/Resources/game.bpak"], (0o100644, b"pack"))
        self.assertEqual(payload["./T.app"][0], 0o40755)
        with open(os.path.join(app, "Contents", "Info.plist"), "rb") as plist:
            self.assertEqual(payload["./T.app/Contents/Info.plist"][1], plist.read())

    def test_the_bom_lists_every_path_by_parent_with_its_cksum(self):
        app, out = self.build()
        rows = read_bom(read_xar(out)[f"{IDENTIFIER}.pkg/Bom"])
        names = {number: (parent, name) for number, parent, name, *_ in rows}

        def path(number):
            parent, name = names[number]
            return name if parent == 0 else path(parent) + "/" + name

        by_path = {path(number): tuple(rest) for number, _parent, _name, *rest in rows}
        self.assertEqual(len(by_path), 8)
        kind, mode, size, checksum, _link = by_path["./T.app/Contents/Resources/game.bpak"]
        self.assertEqual((kind, mode, size, checksum), (1, 0o100644, 4, pkg.cksum(b"pack")))
        self.assertEqual(by_path["./T.app/Contents"][:2], (2, 0o40755))
        self.assertEqual(by_path["."][1], 0)
        with open(os.path.join(app, "Contents", "MacOS", "T"), "rb") as binary:
            self.assertEqual(by_path["./T.app/Contents/MacOS/T"][3], pkg.cksum(binary.read()))

    def test_cksum_is_posix_cksum(self):
        # `printf abc | cksum` and the empty input, as every cksum prints them.
        self.assertEqual(pkg.cksum(b""), 4294967295)
        self.assertEqual(pkg.cksum(b"abc"), 1219131554)
        if tool("cksum"):
            data = os.urandom(70000)
            said = subprocess.run(["cksum"], input=data, capture_output=True, check=True).stdout
            self.assertEqual(int(said.split()[0]), pkg.cksum(data))

    def test_many_paths_take_a_branch_and_links_stay_links(self):
        _app, out = self.build(files=600, links=True)
        files = read_xar(out)
        rows = read_bom(files[f"{IDENTIFIER}.pkg/Bom"])
        self.assertEqual(len(rows), 8 + 1 + 600 + 7)
        self.assertEqual([r[0] for r in rows], list(range(1, len(rows) + 1)))
        links = {r[2]: r[7] for r in rows if r[3] == 3}
        self.assertEqual(links, {"Current": "A", "X": "Versions/Current/X"})
        payload = read_cpio(gzip.decompress(files[f"{IDENTIFIER}.pkg/Payload"]))
        self.assertEqual(payload["./T.app/Contents/Frameworks/X.framework/Versions/Current"], (0o120755, b"A"))

    def test_macos_reads_it(self):
        if not (tool("lsbom") and tool("pkgutil") and tool("installer")):
            self.skipTest("lsbom, pkgutil and installer are macOS's")
        _app, out = self.build(files=300, links=True)
        expanded = os.path.join(self.dir, "expanded")
        subprocess.run(["pkgutil", "--expand", out, expanded], check=True)
        listed = subprocess.run(
            ["lsbom", os.path.join(expanded, f"{IDENTIFIER}.pkg", "Bom")], capture_output=True, text=True, check=True
        ).stdout.splitlines()
        self.assertEqual(len(listed), 8 + 1 + 300 + 7)
        self.assertIn(f"./T.app/Contents/Resources/game.bpak\t100644\t0/0\t4\t{pkg.cksum(b'pack')}", listed)
        self.assertIn("./T.app/Contents/Frameworks/X.framework/Versions/Current\t120755\t0/0\t1\t"
                      f"{pkg.cksum(b'A')}\tA", listed)
        payload = subprocess.run(["pkgutil", "--payload-files", out], capture_output=True, text=True, check=True)
        self.assertIn("./T.app/Contents/MacOS/T", payload.stdout.splitlines())
        unsigned = subprocess.run(["pkgutil", "--check-signature", out], capture_output=True, text=True)
        self.assertIn("no signature", unsigned.stdout)
        info = subprocess.run(["installer", "-pkginfo", "-pkg", out], capture_output=True, text=True, check=True)
        self.assertEqual(info.stdout.split(), ["T", "T"])


if __name__ == "__main__":
    unittest.main()
