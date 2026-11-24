#!/usr/bin/env python3
"""Google's Android libraries for the template, pinned, with no Gradle.

`crates/balaur_google/play/libraries.json` names the root library of each
`[android] capabilities` entry (`roots`, edited by hand) and every artifact
their POMs pull in (`artifacts`, written by `resolve`): its version, the
repository it came from, its SHA-256, its licence and the dependencies its POM
declared. A conflict resolves to the highest version asked for, as Gradle's
does.

  python3 -I scripts/play_libraries.py resolve        re-resolve the roots from
                                                      Maven and rewrite the lock
  python3 -I scripts/play_libraries.py fetch DIR      download the lock into DIR,
                                                      each file checked against it
  python3 -I scripts/play_libraries.py stage DIR OUT  build the template's play/
                                                      from DIR (needs the SDK)
  python3 -I scripts/play_libraries.py --check        fail when the lock drifted
                                                      from its roots

`stage` writes OUT/index.json, which `balaur export` reads: per capability,
the artifacts it needs in dependency order, and per artifact its dex files,
its resources and its manifest entries. Every artifact's `classes.jar` (and
any `libs/*.jar`) is dexed on its own with d8, so two capabilities that share
a library share its dex. The libraries' resources are linked once here to fix
their ids (`ids.txt`, which the export links with `--stable-ids`), and the R
classes the libraries' code reads are generated from that link.

Nothing downloaded is run: an archive is unzipped by name, its jars are only
read by javac and d8, and its XML is parsed as data.
"""
from __future__ import annotations

import argparse
import copy
import itertools
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LOCK = ROOT / "crates" / "balaur_google" / "play" / "libraries.json"
SHIMS = ROOT / "crates" / "balaur_google" / "play"
REPOSITORIES = {
    "google": "https://dl.google.com/android/maven2",
    "central": "https://repo1.maven.org/maven2",
}
POM = "{http://maven.apache.org/POM/4.0.0}"
ANDROID = "http://schemas.android.com/apk/res/android"
TOOLS = "http://schemas.android.com/tools"
# The template's floor, `<uses-sdk android:minSdkVersion>` in package_runtime.sh.
MIN_API = 26
SCOPES = {"compile", "runtime", ""}
ET.register_namespace("android", ANDROID)


def fail(message: str) -> None:
    sys.exit(f"play_libraries.py: {message}")


def version_key(version: str) -> tuple:
    """1.10.0 above 1.9.0, and a release above its own -alpha."""
    main, _, suffix = version.partition("-")
    numbers = tuple(int(p) if p.isdigit() else 0 for p in re.split(r"[.]", main))
    return numbers, suffix == "", suffix


def requested(version: str) -> str:
    """The version a POM asks for: `[18.5.0]` is exactly that, a range its floor."""
    version = version.strip()
    if version[:1] in "[(":
        return version.strip("[]()").split(",")[0].strip()
    return version


def fetch_bytes(url: str) -> bytes | None:
    request = urllib.request.Request(url, headers={"User-Agent": "balaur-play-libraries"})
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read()
    except urllib.error.HTTPError as err:
        if err.code == 404:
            return None
        raise


def artifact_path(group: str, name: str, version: str, ext: str) -> str:
    return f"{group.replace('.', '/')}/{name}/{version}/{name}-{version}.{ext}"


def pom(group: str, name: str, version: str) -> tuple[str, ET.Element]:
    """The POM and the repository it is in: Google's first, then Central."""
    for repo, base in REPOSITORIES.items():
        body = fetch_bytes(f"{base}/{artifact_path(group, name, version, 'pom')}")
        if body is not None:
            return repo, ET.fromstring(body)
    fail(f"no POM for {group}:{name}:{version} in Google's Maven or Central")
    raise AssertionError


def text(node: ET.Element | None, tag: str) -> str:
    found = node.find(POM + tag) if node is not None else None
    return (found.text or "").strip() if found is not None else ""


def properties(project: ET.Element) -> dict[str, str]:
    props = {"project.version": text(project, "version")}
    block = project.find(POM + "properties")
    for prop in block if block is not None else []:
        props[prop.tag.removeprefix(POM)] = (prop.text or "").strip()
    return props


def expand(value: str, props: dict[str, str]) -> str:
    return re.sub(r"\$\{([^}]+)\}", lambda m: props.get(m.group(1), m.group(0)), value)


def dependencies(project: ET.Element) -> list[str]:
    """`group:name:version` for each dependency a consumer gets at run time."""
    props = properties(project)
    managed = {}
    block = project.find(f"{POM}dependencyManagement/{POM}dependencies")
    for dep in block if block is not None else []:
        managed[(text(dep, "groupId"), text(dep, "artifactId"))] = text(dep, "version")
    found = []
    block = project.find(POM + "dependencies")
    for dep in block if block is not None else []:
        if text(dep, "optional") == "true" or text(dep, "scope") not in SCOPES:
            continue
        group, name = text(dep, "groupId"), text(dep, "artifactId")
        version = text(dep, "version") or managed.get((group, name), "")
        version = requested(expand(version, props))
        if not version or "${" in version:
            fail(f"{group}:{name} has no version a POM states")
        found.append(f"{group}:{name}:{version}")
    return found


def license_of(project: ET.Element) -> tuple[str, str]:
    """The licence a POM names, and where its text is."""
    block = project.find(f"{POM}licenses/{POM}license")
    return text(block, "name") or "unstated", text(block, "url")


def packaging_of(project: ET.Element) -> str:
    packaging = text(project, "packaging") or "jar"
    return "jar" if packaging == "bundle" else packaging


def digest(body: bytes) -> str:
    return hashlib.sha256(body).hexdigest()


def published_sums(base: str, path: str, body: bytes) -> None:
    """Hold a download to the checksums its repository publishes beside it."""
    checked = 0
    for ext, algorithm in (("sha256", hashlib.sha256), ("sha1", hashlib.sha1)):
        published = fetch_bytes(f"{base}/{path}.{ext}")
        if published is None:
            continue
        expected = published.decode().split()[0].strip().lower()
        if algorithm(body).hexdigest() != expected:
            fail(f"{path} does not match its published {ext}")
        checked += 1
    if checked == 0:
        fail(f"{path} has no published checksum to hold it to")


def resolve(lock: dict) -> dict:
    """Every artifact the roots pull in, highest version winning, and only the
    dependencies of the versions that won: a version that lost brings none."""
    poms: dict[str, tuple[str, ET.Element]] = {}
    roots = [c for capability in lock["roots"].values() for c in capability]
    chosen: dict[str, str] = {}
    while True:
        asked: dict[str, list[str]] = {}
        queue, seen = list(roots), set()
        while queue:
            coordinate = queue.pop()
            key, version = coordinate.rsplit(":", 1)
            asked.setdefault(key, []).append(version)
            # Walk the version that wins so far, not the one asked for.
            version = chosen.get(key, version)
            if f"{key}:{version}" in seen:
                continue
            seen.add(f"{key}:{version}")
            if f"{key}:{version}" not in poms:
                group, name = key.split(":")
                poms[f"{key}:{version}"] = pom(group, name, version)
            queue += dependencies(poms[f"{key}:{version}"][1])
        settled = {key: max(versions, key=version_key) for key, versions in asked.items()}
        if settled == chosen:
            break
        chosen = settled
    artifacts = {}
    for key, version in sorted(chosen.items()):
        repo, project = poms[f"{key}:{version}"]
        group, name = key.split(":")
        packaging = packaging_of(project)
        path = artifact_path(group, name, version, packaging)
        body = fetch_bytes(f"{REPOSITORIES[repo]}/{path}")
        if body is None:
            fail(f"{key}:{version} has a POM but no .{packaging}")
        published_sums(REPOSITORIES[repo], path, body)
        artifacts[key] = {
            "version": version,
            "packaging": packaging,
            "repository": repo,
            "sha256": digest(body),
            "license": license_of(project)[0],
            "license_url": license_of(project)[1],
            "dependencies": sorted(dependencies(project)),
        }
    return {"roots": lock["roots"], "artifacts": artifacts}


def closure(lock: dict, roots: list[str]) -> list[str]:
    """The artifacts `roots` need, each after everything it depends on."""
    artifacts = lock["artifacts"]
    order: list[str] = []
    seen: set[str] = set()

    def visit(key: str) -> None:
        if key in seen:
            return
        seen.add(key)
        for dep in artifacts[key]["dependencies"]:
            visit(dep.rsplit(":", 1)[0])
        order.append(key)

    for root in roots:
        visit(root.rsplit(":", 1)[0])
    return order


def check(lock: dict) -> list[str]:
    """What is wrong with the lock, offline: drift from its roots or itself."""
    problems = []
    artifacts = lock["artifacts"]
    asked: dict[str, list[str]] = {}
    for key, entry in artifacts.items():
        for dep in entry["dependencies"]:
            dep_key, version = dep.rsplit(":", 1)
            if dep_key not in artifacts:
                problems.append(f"{key} needs {dep_key}, which the lock does not hold")
            asked.setdefault(dep_key, []).append(version)
    for capability, roots in lock["roots"].items():
        for root in roots:
            key, version = root.rsplit(":", 1)
            if key not in artifacts:
                problems.append(f"{capability}'s root {key} is not in the lock: run resolve")
                continue
            asked.setdefault(key, []).append(version)
            if artifacts[key]["version"] != version:
                problems.append(
                    f"{capability} pins {root}, the lock holds "
                    f"{artifacts[key]['version']}: run resolve"
                )
    for key, versions in asked.items():
        if key in artifacts and artifacts[key]["version"] != max(versions, key=version_key):
            problems.append(f"{key} is locked at {artifacts[key]['version']}, under {max(versions, key=version_key)}")
    needed = {k for roots in lock["roots"].values() for k in closure(lock, roots)} if not problems else set()
    for key in sorted(set(artifacts) - needed if needed else set()):
        problems.append(f"{key} is locked but no root needs it: run resolve")
    return problems


def fetch(lock: dict, into: Path) -> dict[str, Path]:
    """Each locked file in `into`, downloaded once and held to its SHA-256."""
    files = {}
    for key, entry in lock["artifacts"].items():
        group, name = key.split(":")
        path = artifact_path(group, name, entry["version"], entry["packaging"])
        target = into / path
        if not (target.is_file() and digest(target.read_bytes()) == entry["sha256"]):
            body = fetch_bytes(f"{REPOSITORIES[entry['repository']]}/{path}")
            if body is None:
                fail(f"{key}:{entry['version']} is gone from {entry['repository']}")
            if digest(body) != entry["sha256"]:
                fail(f"{path} does not match the SHA-256 the lock pins")
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(body)
        files[key] = target
    return files


def safe_names(archive: zipfile.ZipFile, path: Path) -> None:
    for name in archive.namelist():
        if name.startswith("/") or ".." in Path(name).parts:
            fail(f"{path.name} holds {name}, outside the archive")


def unpack(key: str, archive_path: Path, into: Path) -> dict:
    """An AAR's jars, resources, manifest and R.txt, or a JAR as its classes."""
    into.mkdir(parents=True)
    if archive_path.suffix == ".jar":
        shutil.copy(archive_path, into / "classes.jar")
        return {"jars": [into / "classes.jar"], "res": None, "manifest": None, "r": None}
    with zipfile.ZipFile(archive_path) as archive:
        safe_names(archive, archive_path)
        names = archive.namelist()
        if any(n.startswith("jni/") and not n.endswith("/") for n in names):
            fail(f"{key} carries native code, which the template cannot merge yet")
        archive.extractall(into)
    jars = [into / "classes.jar"] if (into / "classes.jar").is_file() else []
    jars += sorted((into / "libs").glob("*.jar"))
    res = into / "res"
    has_res = res.is_dir() and any(p.is_file() for p in res.rglob("*"))
    return {
        "jars": jars,
        "res": res if has_res else None,
        "manifest": into / "AndroidManifest.xml",
        "r": into / "R.txt" if (into / "R.txt").is_file() else None,
    }


def strip_tools(element: ET.Element) -> None:
    for node in element.iter():
        for attr in list(node.attrib):
            if attr.startswith("{%s}" % TOOLS):
                if attr.endswith("}node") and node.attrib[attr] not in ("merge", "replace"):
                    fail(f"a manifest asks the merger for tools:node={node.attrib[attr]}")
                del node.attrib[attr]


def serialize(element: ET.Element) -> str:
    element = copy.deepcopy(element)
    for node in element.iter():
        node.tail = None
        if node.text is not None and not node.text.strip():
            node.text = None
    body = ET.tostring(element, encoding="unicode")
    # One prefix the template already declares, and none of its own.
    body = body.replace(f"{{{ANDROID}}}", "android:")
    return re.sub(r'\s+xmlns:\w+="[^"]*"', "", body).strip()


def manifest_entries(key: str, manifest: Path) -> dict:
    """What an AAR's manifest adds: entries beside `<application>`, and in it."""
    root = ET.parse(manifest).getroot()
    strip_tools(root)
    for uses in root.findall("uses-sdk"):
        floor = int(uses.get(f"{{{ANDROID}}}minSdkVersion", "1"))
        if floor > MIN_API:
            fail(f"{key} needs API {floor}, above the template's {MIN_API}")
    top = [c for c in root if c.tag in ("uses-permission", "permission", "queries", "uses-feature")]
    application = root.find("application")
    inside = list(application) if application is not None else []
    return {"package": root.get("package", ""), "manifest": top, "application": inside}


def merged(elements: list[ET.Element]) -> list[ET.Element]:
    """Entries two libraries both declare, merged as Gradle's merger does: one
    element a name, holding both's children; one `<queries>`."""
    out: list[ET.Element] = []
    by_name: dict[tuple, ET.Element] = {}
    for element in elements:
        key = (element.tag, element.get(f"{{{ANDROID}}}name") or (None if element.tag != "queries" else ""))
        if key[1] is None:
            out.append(element)
            continue
        if key not in by_name:
            fresh = ET.Element(element.tag, dict(element.attrib))
            by_name[key] = fresh
            out.append(fresh)
        held = by_name[key]
        for attr, value in element.attrib.items():
            if held.get(attr, value) != value:
                fail(f"two libraries declare {key[1]} with different {attr}")
            held.set(attr, value)
        seen = {serialize(child) for child in held}
        for child in element:
            if serialize(child) not in seen:
                held.append(child)
                seen.add(serialize(child))
    return out


def subset_manifest(staged: dict, artifacts: list[str]) -> dict:
    """The permissions and entries a set of artifacts adds to the manifest."""
    top = merged([e for k in artifacts for e in staged[k]["manifest"]])
    return {
        "permissions": [e.get(f"{{{ANDROID}}}name") for e in top if e.tag == "uses-permission"],
        "manifest": [serialize(e) for e in top if e.tag != "uses-permission"],
        "application": [serialize(e) for e in merged([e for k in artifacts for e in staged[k]["application"]])],
    }


def run(command: list, what: str) -> None:
    done = subprocess.run([str(c) for c in command], capture_output=True, text=True, check=False)
    if done.returncode != 0:
        sys.stderr.write(done.stdout + done.stderr)
        fail(f"{what} failed ({done.returncode})")


def sdk_tools() -> tuple[Path, Path, Path]:
    sdk = Path(os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
               or Path.home() / "Library/Android/sdk")
    jars = sorted(sdk.glob("platforms/android-*/android.jar"), key=lambda p: version_key(p.parent.name.split("-")[1]))
    tools = sorted(sdk.glob("build-tools/*"), key=lambda p: version_key(p.name))
    if not jars or not tools:
        fail(f"no platforms/android-*/android.jar or build-tools under {sdk}")
    return jars[-1], tools[-1] / "d8", tools[-1] / "aapt2"


def dex(jars: list[Path], classpath: list[Path], android_jar: Path, d8: Path, out: Path) -> list[str]:
    out.mkdir(parents=True)
    command = [d8, "--release", "--min-api", MIN_API, "--lib", android_jar, "--output", out]
    for jar in classpath:
        command += ["--classpath", jar]
    run(command + jars, f"d8 {out.name}")
    return sorted(p.name for p in out.glob("classes*.dex"))


def link_ids(staged: dict, aapt2: Path, android_jar: Path, work: Path) -> tuple[Path, Path, list[str]]:
    """Every library's resources linked once: their ids, and the R sources."""
    compiled = []
    for key, entry in staged.items():
        if entry["res"] is None:
            continue
        flat = work / f"{len(compiled):02}.zip"
        run([aapt2, "compile", "--dir", entry["res"], "-o", flat], f"aapt2 compile {key}")
        compiled.append(flat)
    manifest = work / "AndroidManifest.xml"
    manifest.write_text(
        '<manifest xmlns:android="http://schemas.android.com/apk/res/android" '
        'package="org.balaur.template"><application/></manifest>\n'
    )
    packages = sorted({e["package"] for e in staged.values() if e["r"] is not None and e["package"]})
    ids, java = work / "ids.txt", work / "r"
    command = [aapt2, "link", "-I", android_jar, "--manifest", manifest, "-o", work / "linked.apk",
               "--auto-add-overlay", "--emit-ids", ids, "--java", java,
               "--extra-packages", ":".join(packages)]
    for flat in compiled:
        command += ["-R", flat]
    run(command, "aapt2 link (the libraries' ids)")
    return ids, java, packages


def stage(lock: dict, downloads: Path, out: Path) -> None:
    android_jar, d8, aapt2 = sdk_tools()
    files = fetch(lock, downloads)
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    work = Path(tempfile.mkdtemp(prefix="play-stage-"))
    try:
        order = []
        for roots in lock["roots"].values():
            order += [k for k in closure(lock, roots) if k not in order]
        staged = {}
        for n, key in enumerate(order):
            staged[key] = unpack(key, files[key], work / "unpacked" / f"{n:02}")
            if staged[key]["manifest"] is not None:
                staged[key].update(manifest_entries(key, staged[key]["manifest"]))
            else:
                staged[key].update({"package": "", "manifest": [], "application": []})
        every_jar = [j for e in staged.values() for j in e["jars"]]
        ids, r_java, _ = link_ids(staged, aapt2, android_jar, work)
        shutil.copy(ids, out / "ids.txt")
        index = {"artifacts": {}, "capabilities": {}}
        for n, key in enumerate(order):
            entry = staged[key]
            name = f"{n:02}-{key.split(':')[1]}"
            record = {
                "version": lock["artifacts"][key]["version"],
                "dex": [],
                "res": None,
            }
            if entry["jars"]:
                others = [j for j in every_jar if j not in entry["jars"]]
                record["dex"] = [f"{name}/{d}" for d in dex(entry["jars"], others, android_jar, d8, out / name)]
            if entry["res"] is not None:
                shutil.copytree(entry["res"], out / name / "res")
                record["res"] = f"{name}/res"
            index["artifacts"][key] = record
        # The R classes every library's code reads its resource ids from.
        r_classes = work / "r-classes"
        sources = sorted(str(p) for p in r_java.rglob("*.java"))
        run(["javac", "--release", "11", "-nowarn", "-d", r_classes, *sources], "javac (R)")
        r_jar = work / "r.jar"
        with zipfile.ZipFile(r_jar, "w") as jar:
            for path in sorted(r_classes.rglob("*.class")):
                jar.write(path, path.relative_to(r_classes).as_posix())
        index["r"] = [f"r/{d}" for d in dex([r_jar], [], android_jar, d8, out / "r")]
        for capability, roots in lock["roots"].items():
            shim = SHIMS / capability
            sources = sorted(str(p) for p in shim.rglob("*.java"))
            needed = closure(lock, roots)
            classpath = ":".join(str(j) for k in needed for j in staged[k]["jars"])
            record = {"artifacts": needed, "shim": []}
            if sources:
                classes = work / "shims" / capability
                run(["javac", "--release", "11", "-Werror", "-Xlint:all,-classfile,-processing",
                     "-classpath", f"{android_jar}:{classpath}", "-d", classes, *sources],
                    f"javac ({capability})")
                shim_jar = work / f"{capability}.jar"
                with zipfile.ZipFile(shim_jar, "w") as jar:
                    for path in sorted(classes.rglob("*.class")):
                        jar.write(path, path.relative_to(classes).as_posix())
                deps = [j for k in needed for j in staged[k]["jars"]]
                record["shim"] = [f"shim-{capability}/{d}" for d in
                                  dex([shim_jar], deps, android_jar, d8, out / f"shim-{capability}")]
            index["capabilities"][capability] = record
        # Every set of capabilities a game can declare, each merged once.
        names = sorted(lock["roots"])
        index["manifests"] = {}
        for size in range(1, len(names) + 1):
            for chosen in itertools.combinations(names, size):
                needed = [k for k in order if any(k in closure(lock, lock["roots"][c]) for c in chosen)]
                index["manifests"][",".join(chosen)] = subset_manifest(staged, needed)
        (out / "index.json").write_text(json.dumps(index, indent=2, sort_keys=True) + "\n")
    finally:
        shutil.rmtree(work, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--check", action="store_true", help="fail when the lock drifted")
    parser.add_argument("command", nargs="?", choices=["resolve", "fetch", "stage"])
    parser.add_argument("paths", nargs="*", type=Path)
    args = parser.parse_args()
    lock = json.loads(LOCK.read_text())
    if args.check:
        problems = check(lock)
        for problem in problems:
            print(f"{LOCK.relative_to(ROOT)}: {problem}")
        if not problems:
            print(f"{LOCK.relative_to(ROOT)}: {len(lock['artifacts'])} artifacts, current")
        return 1 if problems else 0
    if args.command == "resolve":
        resolved = resolve(lock)
        LOCK.write_text(json.dumps(resolved, indent=2) + "\n")
        print(f"wrote {LOCK.relative_to(ROOT)} ({len(resolved['artifacts'])} artifacts)")
        return 0
    if args.command == "fetch" and len(args.paths) == 1:
        fetch(lock, args.paths[0])
        return 0
    if args.command == "stage" and len(args.paths) == 2:
        stage(lock, args.paths[0], args.paths[1])
        return 0
    parser.print_usage()
    return 2


if __name__ == "__main__":
    sys.exit(main())
