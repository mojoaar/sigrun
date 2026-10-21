# AUR package

`sigrun-bin` repackages the Linux release artifacts: the desktop app as the
released AppImage (installed under `/opt/sigrun`, run through
`/usr/bin/sigrun-desktop`) and the CLI as `/usr/bin/sigrun`. It builds nothing,
so the two binaries are byte-identical to what GitHub serves.

## Bump on a release

1. Set `pkgver` in `PKGBUILD`.
2. Replace the four `sha256sums` with the new assets:

   ```sh
   gh release download "v${pkgver}" -R mojoaar/sigrun \
     -p "Sigrun_${pkgver}_amd64.AppImage" -p 'sigrun-x86_64-unknown-linux-gnu'
   sha256sum Sigrun_*.AppImage sigrun-x86_64-unknown-linux-gnu
   ```

   The last two sums stay as they are unless `sigrun.png` or
   `sigrun.desktop` changes in this directory.
3. Regenerate the metadata: `makepkg --printsrcinfo > .SRCINFO`.
4. `namcap PKGBUILD` and, on Arch, `makepkg -si`.

## Publishing

The AUR is a separate git remote (`ssh://aur@aur.archlinux.org/sigrun-bin.git`).
Pushing needs an AUR account with the maintainer's SSH key; this repository
holds the source of truth, and the AUR copy is a push of `PKGBUILD`,
`.SRCINFO`, `sigrun.png` and `sigrun.desktop`.

## Why not a native package

A `makepkg` from source would build the Tauri app and the CLI for every user
(about ten minutes, a full webkit2gtk toolchain, and npm's network). The
AppImage is already built by CI on every tag, so repackaging it keeps the AUR
entry to a checksum bump.

`fuse2` is a dependency because AppImages mount themselves at start; the
alternative (`APPIMAGE_EXTRACT_AND_RUN=1`) unpacks 85 MB on every launch.
