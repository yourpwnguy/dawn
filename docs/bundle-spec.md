# DSB-2 bundle specification

Status: current. The engine in this repository implements revision 2. Revision 1
remains readable through the compatibility and migration rules in section 6.

This document is normative. The keywords MUST, MUST NOT, SHOULD, and MAY are
used in the sense of RFC 2119. Where this document and the code disagree, this
document is the bug report. The implementation lives in `src/theme/spec.rs`.

A dawn theme bundle is an immutable directory tree. The tree is versioned,
credited, addressed by a stable id, and installed as a unit. The engine reads
the manifest, validates the tree, stages a copy, validates the staged copy
again, and renames it into a store. Nothing renders straight from an
unvalidated directory.

## 1. Layout

A revision 2 bundle MUST contain exactly these four artifacts at its root. The
four are required; additional files are allowed only inside `assets/`.

```text
<id>/
  theme.toml      # manifest
  config.toml     # complete run config
  preview.webp    # preview still
  assets/         # asset root; MAY be empty
```

* The directory name MUST equal `bundle.id`.
* `config.toml` MUST be a complete dawn configuration. It is not a patch over
  the machine config.
* `preview.webp` MUST be a valid WebP image no larger than 1600 by 1080 pixels
  and no larger than 2 MiB.
* `assets/` MUST exist and MUST be a directory. It MAY contain any number of
  files, including files no config key references. Keeping build leftovers or a
  `.gitkeep` in this directory is valid.
* A regular file at the bundle root other than `theme.toml`, `config.toml`, and
  `preview.webp` is the revision 1 layout. Revision 2 rejects it. Use
  `dawn add --migrate` to move such files into `assets/`.

## 2. Manifest

`theme.toml` MUST parse as TOML and MUST match this shape:

```toml
[spec]
revision = 2

[bundle]
id = "midnight"
name = "Midnight"
version = "1.0.0"
description = "Frosted glass over a video wall."
preview = "preview.webp"

[bundle.engine]
min = "0.5.0"

[[author]]
name = "Ada Grey"
role = "layout, palette"
contact = "ada@example.com"

[[credit]]
name = "Ilya Kuvshinov"
for = "Illustration"
via = "https://example.com/source"
```

Field rules:

* `spec.revision` MUST be `2` for this revision. An absent revision means
  revision 1.
* `bundle.id` MUST match `[a-z0-9][a-z0-9_-]*` and be 1 to 64 characters. It
  MUST be unique within each store.
* `bundle.name` MUST be a non-empty string of at most 128 characters.
* `bundle.version` MUST be semantic versioning, for example `1.0.0`.
* `bundle.description` MUST be a non-empty string of at most 512 characters.
* `bundle.preview` MUST be `preview.webp`.
* `bundle.engine.min` MUST be semantic versioning. The engine MUST be greater
  than or equal to this version.
* At least one `[[author]]` entry is required. Each author MUST have a
  non-empty `name` and `role`. `contact` is optional.
* `[[credit]]` entries are optional. Each credit MUST have a non-empty `name`
  and `for`. `via` is optional.
* License fields are not part of this format. A `[license]` table, or a
  `license` key on a credit, makes the manifest invalid. Licensing stays
  outside the bundle format.
* Unknown keys make the manifest invalid. A typo MUST NOT silently become a
  field that nothing reads.

## 3. Config asset references

Every file path in `config.toml` that names a bundle asset MUST resolve inside
`assets/`.

* The only asset-bearing keys in the current config vocabulary are `image`
  values (for example `[background].image`, `[[login.decor]].image`, and
  per-monitor overrides) and entries in `[plugins].dirs`.
* `[sessions].dirs` names system desktop-entry directories, not bundle assets,
  and is outside this rule.
* References MUST be relative. Absolute paths, `~` expansion, and `..`
  components are invalid.
* Migration patches exactly two path-valued keys when it moves a root-level
  file: `image` and `art`. Matching is exact, so unrelated values are never
  rewritten.

## 4. Validation rules

The engine enforces the following rules before an install. Each rejection is
reported as one line in this form:

```text
rule <n>: <file>: <what>
```

`<file>` is relative to the bundle root when possible. A rule number refers to
this section.

1. The four required artifacts MUST be present and valid. `theme.toml` MUST
   exist, `config.toml` MUST exist, `preview.webp` MUST pass the image checks,
   `assets/` MUST be a directory, and the directory name MUST equal
   `bundle.id`.
2. Every asset reference in `config.toml` MUST resolve inside `assets/`. A
   reference to a root-level file is invalid. Root-level media files are the
   revision 1 layout and are handled only by migration.
3. References MUST be relative, MUST NOT contain `..`, and MUST NOT leave the
   bundle. Symbolic links are not allowed anywhere in the tree. The engine
   rejects all of them, which is stricter than "must not cross the boundary"
   and keeps staged copies identical to their source.
4. `config.toml` MUST pass the complete config validation used by
   `dawn --check-config`, with one bundle-specific addition: `layout.save` MUST
   be `false`. A validation failure is a bundle rejection, and the report
   includes the config issue text.
5. `theme.toml` MUST parse as a revision the engine knows. This engine knows
   revisions 1 and 2, and it MUST refuse an unknown revision rather than render
   a best-effort guess. The engine version MUST also satisfy
   `bundle.engine.min`.
6. `bundle.id` MUST match the pattern in section 2 and MUST be unique per
   store. A second install of the same id without `--force` is rejected.
7. At least one `[[author]]` entry MUST be present.
8. Installs MUST be staged. The engine copies the tree into a temporary sibling
   directory, validates the staged copy again, and renames it into place only
   after validation succeeds. A failed install MUST leave a previously
   installed bundle untouched.

## 5. Store and install behaviour

The canonical store is `/usr/share/dawn/themes`. `DAWN_THEME_DIRS` adds
read-only system stores. The per-user XDG store remains a deprecated read
fallback. Lookup checks the system stores first. A bundle found only in the
user store still loads, and the engine prints one line naming the corrective
`dawn add PATH --force` command.

`dawn add` writes to the system store by default. When the store is not
writable it re-runs itself through `sudo` if the process has a terminal, and
prints the exact command when it does not. `DAWN_NO_SUDO=1` always prints the
command instead of escalating.

Atomicity is part of the format, not an implementation detail an admin is
expected to trust blindly: a bundle either appears under its id with every
validated file or the previous state remains.

## 6. Revision 1 compatibility and migration

A tree that declares `[spec] revision = 1`, uses the older flat `schema = 1`
keys, or declares no revision at all is a revision 1 tree. The engine accepts
it with exactly one warning line that names the migration command. The known
revision 1 looseness applies: authors are optional, the preview has no
dimension or size ceiling, and root-level media is allowed. This is the
backward compatibility path and it does not change in this release.

`dawn add PATH --migrate` performs the mechanical upgrade:

1. Read the tree and copy it to a sibling staging directory.
2. Move every root-level file except `theme.toml`, `config.toml`, and
   `preview.webp` into `assets/`.
3. Patch `image` and `art` values that exactly match a moved file name.
4. Generate `preview.webp` when it is missing.
5. Rewrite the manifest at revision 2. Missing required fields receive visible
   TODO comments. A missing author becomes the placeholder `Your Name` with the
   role `author`.
6. Validate the staged tree against the rules in section 4.

Without `--force` the original tree is left untouched and the validated staged
copy is installed. With `--force` the staged tree replaces the original only
after validation succeeds, and the migrated tree is installed. Migration never
guesses at an asset reference it does not understand: an unresolvable tree
fails validation and the source stays as it was.

## 7. Command surface

* `dawn check PATH` runs the full validation on an uninstalled tree. Zero issues
  exit 0. Any issue exits 1 and prints the numbered report.
* `dawn new NAME [--id ID] [--parent DIR]` creates `DIR/ID/` with the four
  required artifacts, a commented manifest, a minimal commented config, a
  generated 640 by 400 WebP preview, and an `assets/` directory. The id
  defaults to the name in lowercase with dashes. It prints exactly three next
  steps: check, add, and switch.
* `dawn add PATH [--migrate] [--force]` installs one bundle or a directory of
  bundles. `--migrate` upgrades revision 1 trees as described in section 6.
* `dawn use ID` writes `[theme] package = "ID"` into the machine config without
  touching the rest of the file. `dawn list` marks the active theme.
* `dawn list` and `dawn remove` operate on installed bundles by id.

The six example bundles in `examples/themes/` remain revision 1 trees. They are
the compatibility demonstration, and `dawn check` labels each one revision 1
and passes it with the migration warning.
