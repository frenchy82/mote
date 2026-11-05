# Releasing

What a Mote release page says, and which tags stay. The build and the package
steps are in [BUILDING.md](BUILDING.md).

## The release page

A release page is the version's section of
[RELEASE_NOTES.md](RELEASE_NOTES.md) and a compare link, nothing else.
`tools/release-notes.sh` writes both:

```sh
tools/release-notes.sh 1.3.0 > temp/release/1.3.0-github.md
```

The link compares from a tag that stays, so it keeps working: a beta from the
last stable, a stable from the last tag of the series before its own.

## Tags

A beta's tag and release page go once the next beta supersedes them, after its
release page and notes are up. Stable tags stay.

A dropped tag kills every compare link naming it. After dropping any tag, run
`tools/check-release-links.sh --fix`, which finds the pages still naming a tag
that is gone and points them at the tag `tools/release-notes.sh` picks.
