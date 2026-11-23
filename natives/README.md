# natives

The natives database **format** and its tool, `shk-natives` (`gen/`). See
[docs/natives-database.md](../docs/natives-database.md).

- `examples/test-title.tsv` — the natives of Sonoma Runtime's synthetic test title (not game data;
  used by the optional integration test and as an example of the format). The repository's own
  test target builds its database in `tests/src/target.rs`.
- `local/` — your database, built with `tools/build-natives-db.ps1` (git-ignored; never commit it).
