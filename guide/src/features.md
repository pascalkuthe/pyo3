# Features Reference

PyO3 provides a number of Cargo features to customise functionality. This chapter of the guide provides detail on each of them.

By default, only the `macros` feature is enabled.

## Features for extension module authors

### `extension-module`

This feature is required when building a Python extension module using PyO3.

It tells PyO3's build script to skip linking against `libpython.so` on Unix platforms, where this must not be done.

See the [building and distribution](building_and_distribution.md#linking) section for further detail.

### `abi3`

This feature is used when building Python extension modules to create wheels which are compatible with multiple Python versions.

It restricts PyO3's API to a subset of the full Python API which is guaranteed by [PEP 384](https://www.python.org/dev/peps/pep-0384/) to be forwards-compatible with future Python versions.

See the [building and distribution](building_and_distribution.md#py_limited_apiabi3) section for further detail.

### The `abi3-pyXY` features

(`abi3-py37`, `abi3-py38`, `abi3-py39`, and `abi3-py310`)

These features are extensions of the `abi3` feature to specify the exact minimum Python version which the multiple-version-wheel will support.

See the [building and distribution](building_and_distribution.md#minimum-python-version-for-abi3) section for further detail.

## Features for embedding Python in Rust

### `auto-initialize`

This feature changes [`Python::with_gil`]({{#PYO3_DOCS_URL}}/pyo3/struct.Python.html#method.with_gil) and [`Python::acquire_gil`]({{#PYO3_DOCS_URL}}/pyo3/struct.Python.html#method.acquire_gil) to automatically initialize a Python interpreter (by calling [`prepare_freethreaded_python`]({{#PYO3_DOCS_URL}}/pyo3/fn.prepare_freethreaded_python.html)) if needed.

If you do not enable this feature, you should call `pyo3::prepare_freethreaded_python()` before attempting to call any other Python APIs.

## Advanced Features

### `macros`

This feature enables a dependency on the `pyo3-macros` crate, which provides the procedural macros portion of PyO3's API:

- `#[pymodule]`
- `#[pyfunction]`
- `#[pyclass]`
- `#[pymethods]`
- `#[pyproto]`
- `#[derive(FromPyObject)]`

It also provides the `py_run!` macro.

These macros require a number of dependencies which may not be needed by users who just need PyO3 for Python FFI. Disabling this feature enables faster builds for those users, as these dependencies will not be built if this feature is disabled.

> This feature is enabled by default. To disable it, set `default-features = false` for the `pyo3` entry in your Cargo.toml.

### `multiple-pymethods`

This feature enables a dependency on `inventory`, which enables each `#[pyclass]` to have more than one `#[pymethods]` block.

Most users should only need a single `#[pymethods]` per `#[pyclass]`. In addition, not all platforms (e.g. Wasm) are supported by `inventory`. For this reason this feature is not enabled by default, meaning fewer dependencies and faster compilation for the majority of users.

See [the `#[pyclass]` implementation details](class.md#implementation-details) for more information.

### `nightly`

The `nightly` feature needs the nightly Rust compiler. This allows PyO3 to use Rust's unstable specialization feature to apply the following optimizations:
- `FromPyObject` for `Vec` and `[T;N]` can perform a `memcpy` when the object supports the Python buffer protocol.
- `ToBorrowedObject` can skip a reference count increase when the provided object is a Python native type.

### `resolve-config`

The `resolve-config` feature of the `pyo3-build-config` crate controls whether that crate's
build script automatically resolves a Python interpreter / build configuration. This feature is primarily useful when building PyO3
itself. By default this feature is not enabled, meaning you can freely use `pyo3-build-config` as a standalone library to read or write PyO3 build configuration files or resolve metadata about a Python interpreter.

## Optional Dependencies

These features enable conversions between Python types and types from other Rust crates, enabling easy access to the rest of the Rust ecosystem.

### `anyhow`

Adds a dependency on [anyhow](https://docs.rs/anyhow). Enables a conversion from [anyhow](https://docs.rs/anyhow)’s [`Error`](https://docs.rs/anyhow/latest/anyhow/struct.Error.html) type to [`PyErr`](https://docs.rs/pyo3/latest/pyo3/struct.PyErr.html), for easy error handling.

### `eyre`

Adds a dependency on [eyre](https://docs.rs/eyre). Enables a conversion from [eyre](https://docs.rs/eyre)’s [`Report`](https://docs.rs/eyre/latest/eyre/struct.Report.html) type to [`PyErr`](https://docs.rs/pyo3/latest/pyo3/struct.PyErr.html), for easy error handling.

### `hashbrown`

Adds a dependency on [hashbrown](https://docs.rs/hashbrown) and enables conversions into its [`HashMap`](https://docs.rs/hashbrown/latest/hashbrown/struct.HashMap.html) and [`HashSet`](https://docs.rs/hashbrown/latest/hashbrown/struct.HashSet.html) types.

### `indexmap`

Adds a dependency on [indexmap](https://docs.rs/indexmap) and enables conversions into its [`IndexMap`](https://docs.rs/indexmap/latest/indexmap/map/struct.IndexMap.html) type.

### `num-bigint`

Adds a dependency on [num-bigint](https://docs.rs/num-bigint) and enables conversions into its [`BigInt`](https://docs.rs/num-bigint/latest/num_bigint/struct.BigInt.html) and [`BigUint`](https://docs.rs/num-bigint/latest/num_bigint/struct.BigUInt.html) types.

### `num-complex`

Adds a dependency on [num-complex](https://docs.rs/num-complex) and enables conversions into its [`Complex`](https://docs.rs/num-complex/latest/num_complex/struct.Complex.html) type.

### `serde`

Enables (de)serialization of Py<T> objects via [serde](https://serde.rs/).
This allows to use [`#[derive(Serialize, Deserialize)`](https://serde.rs/derive.html) on structs that hold references to `#[pyclass]` instances

```rust

#[pyclass]
#[derive(Serialize, Deserialize)]
struct Permission {
    name: String
}

#[pyclass]
#[derive(Serialize, Deserialize)]
struct User {
    username: String,
    permissions: Vec<Py<Permission>>
}
```
