# SeWriter Update System

SeWriter uses GitHub Pages for the Sparkle appcast and GitHub Releases for DMG assets.

## Stable URLs

- Appcast feed: `https://sego443.github.io/SeWriter/appcast.xml`
- Release DMG pattern: `https://github.com/sego443/SeWriter/releases/download/vX.Y.Z/SeWriter.dmg`

## GitHub Pages Setup

Configure the repository once:

1. Open `Settings -> Pages`.
2. Set source to `Deploy from a branch`.
3. Select branch `main`.
4. Select folder `/docs`.
5. Confirm `https://sego443.github.io/SeWriter/appcast.xml` is reachable.

## Sparkle Key Setup

Download Sparkle 2.9.1 and run:

```bash
./bin/generate_keys
```

Store the private key safely. The current public key is:

```text
b9K4seR/8gKh4rk/mKM1j4ioM89zS1F2l1ebt/oPvSA=
```

## Building With Sparkle

Set both variables before running the build:

```bash
SPARKLE_FRAMEWORK_PATH=/path/to/Sparkle.framework \
./build-mac.sh
```

If `SPARKLE_FRAMEWORK_PATH` is missing, `build-mac.sh` builds a normal non-Sparkle app. The public key defaults to the generated SeWriter key above.

## Release Sequence

1. Build the DMG.
2. Upload `dist/SeWriter.dmg` to a GitHub Release.
3. Sign the uploaded DMG with Sparkle tooling.
4. Add a new item to `docs/appcast.xml` with the GitHub Release asset URL.
5. Commit and push `docs/appcast.xml`.
