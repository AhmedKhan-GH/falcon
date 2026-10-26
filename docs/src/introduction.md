# Introduction

Falcon is a rebuild of the [Eagle](https://eagle.cool) asset manager in Rust, written to learn how Eagle works.

Each chapter in the first part describes one piece of Eagle. It covers what Eagle does, with every claim marked **observed** or **inferred**, and then how falcon implements it.

- **Observed** claims come from Eagle's own files: a real library on disk, or REA analysis of `app.asar`.
- **Inferred** claims are conclusions drawn from those files that haven't been checked directly.

Falcon is written from that behaviour. It doesn't copy Eagle's code.

## Running the docs

```sh
scripts/docs.sh
```

This builds the API docs with `cargo doc` and serves the book at <http://localhost:3000>, reloading on changes.

## Workspace

| Crate | Purpose |
|---|---|
| `falcon-library` | Read and write `.library` folders |
| `falcon` | Command-line tool |
