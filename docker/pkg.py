#!/usr/bin/env python3
"""Wrap a signed macOS .app as the flat installer package the Mac App Store takes.

    balaur-pkg <Game.app> <out.pkg>

What `productbuild --component <app> /Applications <pkg>` writes, without a
Mac: a xar archive holding a Distribution and one component package
(`<bundle id>.pkg/`) of PackageInfo, Bom and Payload, the payload a gzip'd
odc cpio of the bundle rooted at `.`, installed into /Applications. The
bundle id, both version strings, the title and the minimum macOS come from
the .app's Info.plist, the architectures from its executable. The package
is unsigned; `rcodesign sign` with a Mac Installer identity signs it.

The standard library alone, so the signing image needs nothing compiled for
it: Python is already there for the profiles. Every file is owned by
root:wheel, a directory is 0755 and a file 0644, or 0755 when anything may
run it, since the store refuses a file not every user can read. A symlink is
kept as one. Exit status is the verdict.
"""

import gzip
import hashlib
import math
import os
import plistlib
import shutil
import stat
import struct
import sys
import tempfile
import time
import zlib
from xml.sax.saxutils import quoteattr, escape

INSTALL_LOCATION = "/Applications"

# Mach-O cpu types (mach/machine.h), as Distribution's hostArchitectures names them.
CPU_NAMES = {0x01000007: "x86_64", 0x0100000C: "arm64"}
FAT_MAGIC = 0xCAFEBABE
THIN_MAGICS = (0xFEEDFACF, 0xCFFAEDFE)


def fail(message):
    print(f"error: {message}", file=sys.stderr)
    sys.exit(1)


class Entry:
    """One path of the payload, as cpio and the Bom both describe it."""

    def __init__(self, path, source, kind, mode, mtime, size=0, link=""):
        self.path = path  # "." or "./Game.app/…"
        self.source = source
        self.kind = kind  # "dir", "file" or "link"
        self.mode = mode
        self.mtime = mtime
        self.size = size
        self.link = link
        self.checksum = 0


def walk(app):
    """The bundle and all under it, the bundle as `./<name>.app`, in name order."""
    root = Entry(".", None, "dir", stat.S_IFDIR | 0o755, int(os.stat(app).st_mtime))
    entries = [root]

    def visit(source, path):
        info = os.lstat(source)
        mtime = int(info.st_mtime)
        if stat.S_ISLNK(info.st_mode):
            target = os.readlink(source)
            entry = Entry(path, source, "link", stat.S_IFLNK | 0o755, mtime, len(target.encode()), target)
            entries.append(entry)
        elif stat.S_ISDIR(info.st_mode):
            entries.append(Entry(path, source, "dir", stat.S_IFDIR | 0o755, mtime))
            for name in sorted(os.listdir(source)):
                visit(os.path.join(source, name), f"{path}/{name}")
        elif stat.S_ISREG(info.st_mode):
            executable = info.st_mode & 0o111
            mode = stat.S_IFREG | (0o755 if executable else 0o644)
            if info.st_size >= 1 << 32:
                fail(f"{path} is {info.st_size} bytes; a Bom records a file under 4 GiB")
            entries.append(Entry(path, source, "file", mode, mtime, info.st_size))
        else:
            fail(f"{path} is neither a file, a directory nor a symlink")

    visit(app, "./" + os.path.basename(os.path.normpath(app)))
    return entries


# POSIX cksum's CRC, which the Bom records: zlib's CRC-32 run over each byte
# bit-reversed, since cksum reads bits most significant first.
REVERSED = bytes(int(f"{i:08b}"[::-1], 2) for i in range(256))


class Cksum:
    def __init__(self):
        self.crc = 0xFFFFFFFF
        self.length = 0

    def update(self, data):
        self.crc = zlib.crc32(data.translate(REVERSED), self.crc)
        self.length += len(data)

    def value(self):
        length = self.length
        tail = bytearray()
        while length:
            tail.append(length & 0xFF)
            length >>= 8
        register = zlib.crc32(bytes(tail).translate(REVERSED), self.crc) ^ 0xFFFFFFFF
        return ~int(f"{register:032b}"[::-1], 2) & 0xFFFFFFFF


def cksum(data):
    total = Cksum()
    total.update(data)
    return total.value()


# The payload: odc cpio ("070707", every number octal), gzip'd, each file's
# checksum taken on the way through for the Bom.
def write_payload(entries, out):
    with gzip.GzipFile(filename="", fileobj=out, mode="wb", compresslevel=6, mtime=0) as archive:
        for inode, entry in enumerate(entries, start=1):
            if entry.kind == "link":
                body = entry.link.encode()
                entry.checksum = cksum(body)
            else:
                body = b""
            name = entry.path.encode() + b"\0"
            nlink = 2 if entry.kind == "dir" else 1
            archive.write(cpio_header(inode, entry.mode, nlink, entry.mtime, len(name), entry.size))
            archive.write(name)
            if entry.kind == "file":
                total = Cksum()
                with open(entry.source, "rb") as source:
                    while chunk := source.read(1 << 20):
                        total.update(chunk)
                        archive.write(chunk)
                entry.checksum = total.value()
            else:
                archive.write(body)
        trailer = b"TRAILER!!!\0"
        archive.write(cpio_header(0, 0, 1, 0, len(trailer), 0))
        archive.write(trailer)


def cpio_header(inode, mode, nlink, mtime, namesize, filesize):
    return (
        "070707"
        f"{0:06o}{inode:06o}{mode:06o}{0:06o}{0:06o}{nlink:06o}{0:06o}"
        f"{mtime:011o}{namesize:06o}{filesize:011o}"
    ).encode()


# The Bom: a "BOMStore" of numbered blocks and named variables, laid out as
# bomutils' mkbom writes it (the format is Apple's and undocumented): Paths,
# a tree of 256-path leaves, then the empty HLIndex, VIndex and Size64.
class BomStore:
    HEADER = 512

    def __init__(self):
        self.blocks = [None]  # block 0 is always the null block
        self.vars = []

    def add(self, data):
        self.blocks.append(bytearray(data))
        return len(self.blocks) - 1

    def var(self, name, data):
        self.vars.append((name, self.add(data)))

    def encode(self):
        names = struct.pack(">I", len(self.vars)) + b"".join(
            struct.pack(">IB", index, len(name)) + name.encode() for name, index in self.vars
        )
        heap = bytearray()
        pointers = [(0, 0)]
        start = self.HEADER + len(names)
        for block in self.blocks[1:]:
            pointers.append((start + len(heap), len(block)))
            heap += block
        table = struct.pack(">I", len(pointers)) + b"".join(struct.pack(">II", a, n) for a, n in pointers)
        free = struct.pack(">I", 0) + bytes(16)  # mkbom's two empty free-list slots
        index_offset = start + len(heap)
        header = struct.pack(
            ">8sIIIIII",
            b"BOMStore",
            1,
            len(self.blocks) - 1,
            index_offset,
            len(table) + len(free),
            self.HEADER,
            len(names),
        )
        return header.ljust(self.HEADER, b"\0") + names + bytes(heap) + table + free


def tree(child, count, block_size=4096):
    return b"tree" + struct.pack(">IIIIB", 1, child, block_size, count, 0)


def paths_node(leaf, indices, forward=0, backward=0):
    return struct.pack(">HHII", 1 if leaf else 0, len(indices), forward, backward) + b"".join(
        struct.pack(">II", a, b) for a, b in indices
    )


BOM_TYPES = {"file": 1, "dir": 2, "link": 3}
LEAF_PATHS = 256


def write_bom(entries):
    bom = BomStore()
    count = len(entries)
    bom.var("BomInfo", struct.pack(">III", 1, count + 1, 1) + bytes(16))

    # Breadth first, each directory's children in name order: ids rise with
    # (parent, name), the key the tree is searched by.
    children = {}
    for entry in entries[1:]:
        parent = entry.path.rsplit("/", 1)[0]
        children.setdefault(parent, []).append(entry)
    order = [(0, entries[0])]
    cursor = 0
    while cursor < len(order):
        entry = order[cursor][1]
        cursor += 1
        order.extend((cursor, child) for child in children.get(entry.path, []))

    indices = []
    for number, (parent, entry) in enumerate(order, start=1):
        link = entry.link.encode() + b"\0" if entry.kind == "link" else b""
        # The root has no mode of its own in the Bom, as pkgbuild writes it.
        mode = 0 if entry.path == "." else entry.mode
        info2 = struct.pack(
            ">BBHHIIIIBII",
            BOM_TYPES[entry.kind],
            1,
            3,
            mode & 0xFFFF,
            0,
            0,
            entry.mtime & 0xFFFFFFFF,
            entry.size,
            1,
            entry.checksum,
            len(link),
        ) + link
        info1 = struct.pack(">II", number, bom.add(info2))
        name = entry.path.rsplit("/", 1)[-1] if entry.path != "." else "."
        file = struct.pack(">I", parent) + name.encode() + b"\0"
        indices.append((bom.add(info1), bom.add(file)))

    leaves = [indices[i : i + LEAF_PATHS] for i in range(0, len(indices), LEAF_PATHS)]
    if len(leaves) == 1:
        child = bom.add(paths_node(True, leaves[0]))
    else:
        blocks = [bom.add(paths_node(True, leaf)) for leaf in leaves]
        for i, block in enumerate(blocks):
            forward = blocks[i + 1] if i + 1 < len(blocks) else 0
            backward = blocks[i - 1] if i else 0
            bom.blocks[block] = bytearray(paths_node(True, leaves[i], forward, backward))
        # A branch names each leaf by its last path.
        child = bom.add(paths_node(False, [(block, leaf[-1][1]) for block, leaf in zip(blocks, leaves)]))
    bom.var("Paths", tree(child, count))

    empty = paths_node(True, [])
    bom.var("HLIndex", tree(bom.add(empty), 0))
    vtree = bom.add(tree(bom.add(empty), 0, block_size=128))
    bom.var("VIndex", struct.pack(">IIIB", 1, vtree, 0, 0))
    bom.var("Size64", tree(bom.add(empty), 0))
    return bom.encode()


def architectures(executable):
    """The cpu types a Mach-O holds, as Distribution names them, or []."""
    try:
        with open(executable, "rb") as binary:
            head = binary.read(4096)
    except OSError:
        return []
    if len(head) < 8:
        return []
    (magic,) = struct.unpack(">I", head[:4])
    found = []
    if magic == FAT_MAGIC:
        (count,) = struct.unpack(">I", head[4:8])
        for i in range(min(count, 32)):
            at = 8 + i * 20
            if len(head) < at + 4:
                break
            (cpu,) = struct.unpack(">I", head[at : at + 4])
            found.append(cpu)
    elif magic in THIN_MAGICS:
        order = ">" if magic == 0xFEEDFACF else "<"
        (cpu,) = struct.unpack(order + "I", head[4:8])
        found.append(cpu)
    return sorted({CPU_NAMES[cpu] for cpu in found if cpu in CPU_NAMES})


def attrs(**values):
    return " ".join(f"{key.replace('_', '-')}={quoteattr(str(value))}" for key, value in values.items())


def package_info(identifier, version, build, bundle, files, kbytes):
    bundle_id = attrs(id=identifier)
    return f"""<?xml version="1.0" encoding="utf-8"?>
<pkg-info {attrs(overwrite_permissions="true", relocatable="false", identifier=identifier,
                 postinstall_action="none", version=version, format_version="2",
                 generator_version="balaur-pkg", install_location=INSTALL_LOCATION, auth="root")}>
    <payload {attrs(numberOfFiles=files, installKBytes=kbytes)}/>
    <bundle path={quoteattr("./" + bundle)} {bundle_id} {attrs(CFBundleShortVersionString=version, CFBundleVersion=build)}/>
    <bundle-version>
        <bundle {bundle_id}/>
    </bundle-version>
    <upgrade-bundle>
        <bundle {bundle_id}/>
    </upgrade-bundle>
    <update-bundle/>
    <atomic-update-bundle/>
    <strict-identifier>
        <bundle {bundle_id}/>
    </strict-identifier>
    <relocate/>
</pkg-info>
""".encode()


def distribution(identifier, version, build, bundle, title, minimum, arches, kbytes, component):
    ref = attrs(id=identifier)
    options = attrs(customize="never", require_scripts="false")
    if arches:
        options += " " + attrs(hostArchitectures=",".join(arches))
    volume = ""
    if minimum:
        volume = f"""
    <volume-check>
        <allowed-os-versions>
            <os-version {attrs(min=minimum)}/>
        </allowed-os-versions>
    </volume-check>"""
    return f"""<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <pkg-ref {ref}>
        <bundle-version>
            <bundle {attrs(CFBundleShortVersionString=version, CFBundleVersion=build, id=identifier, path=bundle)}/>
        </bundle-version>
    </pkg-ref>
    <product {attrs(id=identifier, version=version)}/>
    <title>{escape(title)}</title>
    <options {options}/>{volume}
    <choices-outline>
        <line choice="default">
            <line {attrs(choice=identifier)}/>
        </line>
    </choices-outline>
    <choice {attrs(id="default", title=title, versStr=version)}/>
    <choice {attrs(id=identifier, title=title, visible="false", customLocation=INSTALL_LOCATION)}>
        <pkg-ref {ref}/>
    </choice>
    <pkg-ref {attrs(id=identifier, version=version, onConclusion="none", installKBytes=kbytes, updateKBytes="0")}>#{escape(component)}</pkg-ref>
</installer-gui-script>
""".encode()


# The xar around it: a 28-byte header, the zlib'd XML table of contents, then
# the heap, which opens with the TOC's SHA-1 as every reader checks it.
XAR_HEADER = struct.Struct(">4sHHQQI")
XAR_SHA1 = 1


def xar_file(file_id, name, data, mtime):
    """A file of the TOC; `data` is (offset, length, size, archived sha1, extracted sha1, style)."""
    offset, length, size, archived, extracted, style = data
    return f"""<file id="{file_id}">
<name>{escape(name)}</name>
<type>file</type>
<mode>0644</mode>
<uid>0</uid>
<user>root</user>
<gid>0</gid>
<group>wheel</group>
<mtime>{mtime}</mtime>
<data>
<length>{length}</length>
<offset>{offset}</offset>
<size>{size}</size>
<encoding style="{style}"/>
<archived-checksum style="sha1">{archived}</archived-checksum>
<extracted-checksum style="sha1">{extracted}</extracted-checksum>
</data>
</file>
"""


def write_xar(out, component, small, payload):
    """`small` is [(name, bytes)] of the component's files, zlib'd; `payload` a file stored as is."""
    heap = []
    offset = hashlib.sha1().digest_size
    stamp = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    entries = {}
    for name, data in small:
        packed = zlib.compress(data, 9)
        entries[name] = (
            offset,
            len(packed),
            len(data),
            hashlib.sha1(packed).hexdigest(),
            hashlib.sha1(data).hexdigest(),
            "application/x-gzip",
        )
        heap.append(packed)
        offset += len(packed)
    payload_sha = hashlib.sha1()
    payload_size = 0
    with open(payload, "rb") as source:
        while chunk := source.read(1 << 20):
            payload_sha.update(chunk)
            payload_size += len(chunk)
    digest = payload_sha.hexdigest()
    entries["Payload"] = (offset, payload_size, payload_size, digest, digest, "application/octet-stream")

    inner = "".join(
        xar_file(i, name, entries[name], stamp)
        for i, name in enumerate(["Bom", "Payload", "PackageInfo"], start=2)
    )
    toc = f"""<?xml version="1.0" encoding="UTF-8"?>
<xar>
<toc>
<checksum style="sha1">
<offset>0</offset>
<size>20</size>
</checksum>
<creation-time>{stamp[:-1]}</creation-time>
<file id="1">
<name>{escape(component)}</name>
<type>directory</type>
<mode>0755</mode>
<uid>0</uid>
<user>root</user>
<gid>0</gid>
<group>wheel</group>
<mtime>{stamp}</mtime>
{inner}</file>
{xar_file(5, "Distribution", entries["Distribution"], stamp)}</toc>
</xar>
""".encode()
    packed_toc = zlib.compress(toc, 9)
    out.write(XAR_HEADER.pack(b"xar!", XAR_HEADER.size, 1, len(packed_toc), len(toc), XAR_SHA1))
    out.write(packed_toc)
    out.write(hashlib.sha1(packed_toc).digest())
    for packed in heap:
        out.write(packed)
    with open(payload, "rb") as source:
        shutil.copyfileobj(source, out, 1 << 20)


def build(app, pkg):
    app = os.path.normpath(app)
    bundle = os.path.basename(app)
    if not bundle.endswith(".app") or not os.path.isdir(app):
        fail(f"{app} is not a .app bundle")
    try:
        with open(os.path.join(app, "Contents", "Info.plist"), "rb") as plist:
            info = plistlib.load(plist)
    except (OSError, plistlib.InvalidFileException):
        fail(f"{bundle} has no readable Contents/Info.plist")
    try:
        identifier = info["CFBundleIdentifier"]
        version = info["CFBundleShortVersionString"]
        build_number = info["CFBundleVersion"]
    except KeyError as missing:
        fail(f"{bundle}'s Info.plist has no {missing.args[0]}")
    title = info.get("CFBundleDisplayName") or info.get("CFBundleName") or bundle[: -len(".app")]
    minimum = info.get("LSMinimumSystemVersion", "")
    executable = os.path.join(app, "Contents", "MacOS", info.get("CFBundleExecutable", ""))
    arches = architectures(executable)
    component = f"{identifier}.pkg"

    entries = walk(app)
    with tempfile.TemporaryDirectory(dir=os.path.dirname(os.path.abspath(pkg))) as scratch:
        payload = os.path.join(scratch, "Payload")
        with open(payload, "wb") as out:
            write_payload(entries, out)
        files = len(entries) - 1
        kbytes = math.ceil(sum(e.size for e in entries if e.kind == "file") / 1024)
        small = [
            ("Bom", write_bom(entries)),
            ("PackageInfo", package_info(identifier, version, build_number, bundle, files, kbytes)),
            ("Distribution", distribution(identifier, version, build_number, bundle, title, minimum,
                                          arches, kbytes, component)),
        ]
        partial = os.path.join(scratch, "out.pkg")
        with open(partial, "wb") as out:
            write_xar(out, component, small, payload)
        os.replace(partial, pkg)
    print(f"==> packaged {bundle} as {os.path.basename(pkg)}: {identifier} {version} ({build_number}), "
          f"{files} files into {INSTALL_LOCATION}")


def main(argv):
    if len(argv) != 3:
        fail("usage: balaur-pkg <Game.app> <out.pkg>")
    build(argv[1], argv[2])


if __name__ == "__main__":
    main(sys.argv)
