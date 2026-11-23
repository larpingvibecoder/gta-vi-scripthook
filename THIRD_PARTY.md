# Third-party material

| What | Where | Licence |
|---|---|---|
| Inter (Inter-SemiBold.ttf, InterDisplay-Bold.ttf) by The Inter Project Authors | `assets/fonts/` (licence: `assets/fonts/Inter-LICENSE.txt`) | SIL Open Font License 1.1 |

Inter SemiBold is embedded in `scripthook.dll` and the SDK (the overlay's and the previews' text,
rasterized by the repository's own TrueType reader). The README title (`assets/title-*.svg`) is
Inter Display Bold converted to outlines by `tools/title`; the font itself is not modified or
renamed. Under the OFL the fonts may be bundled with software; they must keep their licence and
may not be sold on their own.

The icons in `assets/icons/` are simple monochrome drawings of the Discord and GitHub marks, used
only to link to those services. The marks are trademarks of their owners. The GitHub icon's path
is the `mark-github` icon of GitHub's Octicons (https://github.com/primer/octicons, MIT licence,
Copyright (c) GitHub Inc.).

Nothing else in this repository comes from another project. No external Rust crates are used.
