# ESCPost Python binding

Python bindings for ESCPost's dot-accurate ESC/POS renderer. The package
exposes `render` and `render_result` from the shared Rust rendering engine.

The binding is currently developed and distributed from the ESCPost source
workspace; it has not yet been published to PyPI.

## Development

From this directory, run the binding suite with:

```bash
just test
```

The command uses the repository's Docker Compose development image, so Rust,
Python, and Maturin do not need to be installed on the host. `just` is only a
task runner; `./test` invokes the same workflow directly.

## License

Licensed under the [Apache License 2.0](LICENSE).
