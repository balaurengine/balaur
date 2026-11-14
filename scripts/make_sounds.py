#!/usr/bin/env python3
"""Write the sounds `examples/sound` plays. They are computed tones rather than
recordings so the files stay kilobytes and nothing carries a third-party
licence."""

import math
import pathlib
import struct
import wave

OUT = pathlib.Path(__file__).resolve().parent.parent / "examples" / "sound" / "audio"
RATE_HZ = 22050


def write(name, seconds, sample):
    """`sample(t)` answers -1 to 1 for each instant; the file is 16-bit mono."""
    frames = bytearray()
    for n in range(int(seconds * RATE_HZ)):
        value = max(-1.0, min(1.0, sample(n / RATE_HZ)))
        frames += struct.pack("<h", round(value * 32767))
    path = OUT / f"{name}.wav"
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(RATE_HZ)
        out.writeframes(bytes(frames))
    print(f"{path.name}: {path.stat().st_size} bytes")


def bell(t):
    ring = math.sin(2 * math.pi * 880 * t) + 0.4 * math.sin(2 * math.pi * 2640 * t)
    return 0.5 * ring * math.exp(-6 * t)


def thud(t):
    # The pitch falls as it fades, which is what makes a thump out of a tone.
    return 0.9 * math.sin(2 * math.pi * (110 - 220 * t) * t) * math.exp(-28 * t)


def hum(t):
    # Whole cycles of every partial in two seconds, so the loop has no click.
    chord = sum(math.sin(2 * math.pi * hz * t) for hz in (110.0, 165.0, 220.5))
    return 0.2 * chord * (1.0 + 0.3 * math.sin(2 * math.pi * 0.5 * t))


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    write("bell", 0.8, bell)
    write("thud", 0.2, thud)
    write("hum", 2.0, hum)


if __name__ == "__main__":
    main()
