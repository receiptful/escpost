//! Python bindings for ESCPost.

use escpost_profiles::PrinterProfile;
use escpost_profiles::resolver::{self, ResolveError};
use escpost_render::{DeviceEvent, RenderResult, RenderWarning, render as render_escpos};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

#[derive(Debug)]
enum BindingError {
    UnknownProfile(String),
    LoadProfiles(String),
    Render(String),
}

#[pyfunction]
#[pyo3(signature = (data, *, profile))]
fn render<'py>(py: Python<'py>, data: &[u8], profile: &str) -> PyResult<Vec<Bound<'py, PyBytes>>> {
    let data = data.to_vec();
    let profile = profile.to_owned();
    let rendered = py
        .detach(move || render_with_profile(&data, &profile))
        .map_err(BindingError::into_py_err)?;

    Ok(rendered
        .sheets
        .iter()
        .map(|sheet| PyBytes::new(py, &sheet.png))
        .collect())
}

#[pyfunction]
#[pyo3(signature = (data, *, profile))]
fn render_result<'py>(py: Python<'py>, data: &[u8], profile: &str) -> PyResult<Bound<'py, PyDict>> {
    let data = data.to_vec();
    let profile = profile.to_owned();
    let rendered = py
        .detach(move || render_with_profile(&data, &profile))
        .map_err(BindingError::into_py_err)?;

    render_result_to_python(py, &rendered)
}

fn render_with_profile(data: &[u8], profile: &str) -> Result<RenderResult, BindingError> {
    let profile = load_profile(profile)?;
    let rendered =
        render_escpos(data, profile).map_err(|error| BindingError::Render(error.to_string()))?;

    Ok(rendered)
}

fn load_profile(profile_id: &str) -> Result<&'static PrinterProfile, BindingError> {
    resolver::resolve(profile_id).map_err(|error| match error {
        ResolveError::UnknownProfile(id) => BindingError::UnknownProfile(id),
        ResolveError::LoadPack(message) => BindingError::LoadProfiles(message),
    })
}

fn render_result_to_python<'py>(
    py: Python<'py>,
    rendered: &RenderResult,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    let sheets = rendered
        .sheets
        .iter()
        .map(|sheet| PyBytes::new(py, &sheet.png));
    result.set_item("sheets", PyList::new(py, sheets)?)?;
    result.set_item(
        "device_events",
        device_events_to_python(py, &rendered.device_events)?,
    )?;
    result.set_item("warnings", warnings_to_python(py, &rendered.warnings)?)?;
    result.set_item("metadata", metadata_to_python(py, rendered)?)?;
    Ok(result)
}

fn warnings_to_python<'py>(
    py: Python<'py>,
    warnings: &[RenderWarning],
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for warning in warnings {
        let item = PyDict::new(py);
        match warning {
            RenderWarning::UncuttableCut {
                command,
                profile,
                offset,
            } => {
                item.set_item("type", "uncuttable_cut")?;
                item.set_item("command", command)?;
                item.set_item("profile", profile)?;
                item.set_item("offset", offset)?;
            }
        }
        item.set_item("message", warning.to_string())?;
        result.append(item)?;
    }
    Ok(result)
}

fn device_events_to_python<'py>(
    py: Python<'py>,
    events: &[DeviceEvent],
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for event in events {
        let item = PyDict::new(py);
        match event {
            DeviceEvent::CashDrawerPulse {
                connector,
                on_time_units,
                off_time_units,
            } => {
                item.set_item("type", "cash_drawer_pulse")?;
                item.set_item("connector", connector)?;
                item.set_item("on_time_units", on_time_units)?;
                item.set_item("off_time_units", off_time_units)?;
            }
        }
        result.append(item)?;
    }
    Ok(result)
}

fn metadata_to_python<'py>(
    py: Python<'py>,
    rendered: &RenderResult,
) -> PyResult<Bound<'py, PyDict>> {
    let metadata = PyDict::new(py);
    metadata.set_item("renderer_version", rendered.metadata.renderer_version)?;
    metadata.set_item("profile_id", &rendered.metadata.profile_id)?;
    metadata.set_item(
        "canonical_profile_sha256",
        &rendered.metadata.canonical_profile_sha256,
    )?;
    Ok(metadata)
}

impl BindingError {
    fn into_py_err(self) -> PyErr {
        match self {
            Self::UnknownProfile(profile) => {
                PyValueError::new_err(format!("unknown printer profile {profile:?}"))
            }
            Self::LoadProfiles(message) => {
                PyRuntimeError::new_err(format!("could not load canonical profile pack: {message}"))
            }
            Self::Render(message) => PyRuntimeError::new_err(message),
        }
    }
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(render, module)?)?;
    module.add_function(wrap_pyfunction!(render_result, module)?)?;
    Ok(())
}
