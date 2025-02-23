//! `PyClass` and related traits.
use crate::{
    class::impl_::{fallback_new, tp_dealloc, PyClassImpl},
    ffi,
    impl_::pyclass::{PyClassDict, PyClassWeakRef},
    PyCell, PyErr, PyMethodDefType, PyNativeType, PyResult, PyTypeInfo, Python,
};
use std::{
    convert::TryInto,
    ffi::{CStr, CString},
    os::raw::{c_char, c_int, c_uint, c_void},
    ptr,
};

/// If `PyClass` is implemented for a Rust type `T`, then we can use `T` in the Python
/// world, via `PyCell`.
///
/// The `#[pyclass]` attribute automatically implements this trait for your Rust struct,
/// so you normally don't have to use this trait directly.
pub trait PyClass:
    PyTypeInfo<AsRefTarget = PyCell<Self>> + PyClassImpl<Layout = PyCell<Self>>
{
    /// Specify this class has `#[pyclass(dict)]` or not.
    type Dict: PyClassDict;
    /// Specify this class has `#[pyclass(weakref)]` or not.
    type WeakRef: PyClassWeakRef;
    /// The closest native ancestor. This is `PyAny` by default, and when you declare
    /// `#[pyclass(extends=PyDict)]`, it's `PyDict`.
    type BaseNativeType: PyTypeInfo + PyNativeType;
}

fn into_raw<T>(vec: Vec<T>) -> *mut c_void {
    Box::into_raw(vec.into_boxed_slice()) as _
}

pub(crate) fn create_type_object<T>(py: Python) -> *mut ffi::PyTypeObject
where
    T: PyClass,
{
    match unsafe {
        create_type_object_impl(
            py,
            T::DOC,
            T::MODULE,
            T::NAME,
            T::BaseType::type_object_raw(py),
            std::mem::size_of::<T::Layout>(),
            T::get_new(),
            tp_dealloc::<T>,
            T::get_alloc(),
            T::get_free(),
            T::dict_offset(),
            T::weaklist_offset(),
            &T::for_each_method_def,
            &T::for_each_proto_slot,
            T::IS_GC,
            T::IS_BASETYPE,
        )
    } {
        Ok(type_object) => type_object,
        Err(e) => type_object_creation_failed(py, e, T::NAME),
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_type_object_impl(
    py: Python,
    tp_doc: &str,
    module_name: Option<&str>,
    name: &str,
    base_type_object: *mut ffi::PyTypeObject,
    basicsize: usize,
    tp_new: Option<ffi::newfunc>,
    tp_dealloc: ffi::destructor,
    tp_alloc: Option<ffi::allocfunc>,
    tp_free: Option<ffi::freefunc>,
    dict_offset: Option<ffi::Py_ssize_t>,
    weaklist_offset: Option<ffi::Py_ssize_t>,
    for_each_method_def: &dyn Fn(&mut dyn FnMut(&[PyMethodDefType])),
    for_each_proto_slot: &dyn Fn(&mut dyn FnMut(&[ffi::PyType_Slot])),
    is_gc: bool,
    is_basetype: bool,
) -> PyResult<*mut ffi::PyTypeObject> {
    let mut slots = Vec::new();

    fn push_slot(slots: &mut Vec<ffi::PyType_Slot>, slot: c_int, pfunc: *mut c_void) {
        slots.push(ffi::PyType_Slot { slot, pfunc });
    }

    push_slot(&mut slots, ffi::Py_tp_base, base_type_object as _);
    if let Some(doc) = py_class_doc(tp_doc) {
        push_slot(&mut slots, ffi::Py_tp_doc, doc as _);
    }

    push_slot(
        &mut slots,
        ffi::Py_tp_new,
        tp_new.unwrap_or(fallback_new) as _,
    );
    push_slot(&mut slots, ffi::Py_tp_dealloc, tp_dealloc as _);

    if let Some(alloc) = tp_alloc {
        push_slot(&mut slots, ffi::Py_tp_alloc, alloc as _);
    }
    if let Some(free) = tp_free {
        push_slot(&mut slots, ffi::Py_tp_free, free as _);
    }

    #[cfg(Py_3_9)]
    {
        let members = py_class_members(dict_offset, weaklist_offset);
        if !members.is_empty() {
            push_slot(&mut slots, ffi::Py_tp_members, into_raw(members))
        }
    }

    // normal methods
    let methods = py_class_method_defs(for_each_method_def);
    if !methods.is_empty() {
        push_slot(&mut slots, ffi::Py_tp_methods, into_raw(methods));
    }

    // properties
    let props = py_class_properties(dict_offset.is_none(), for_each_method_def);
    if !props.is_empty() {
        push_slot(&mut slots, ffi::Py_tp_getset, into_raw(props));
    }

    // protocol methods
    let mut has_gc_methods = false;
    // Before Python 3.9, need to patch in buffer methods manually (they don't work in slots)
    #[cfg(all(not(Py_3_9), not(Py_LIMITED_API)))]
    let mut buffer_procs: ffi::PyBufferProcs = Default::default();

    for_each_proto_slot(&mut |proto_slots| {
        for slot in proto_slots {
            has_gc_methods |= slot.slot == ffi::Py_tp_clear || slot.slot == ffi::Py_tp_traverse;

            #[cfg(all(not(Py_3_9), not(Py_LIMITED_API)))]
            if slot.slot == ffi::Py_bf_getbuffer {
                // Safety: slot.pfunc is a valid function pointer
                buffer_procs.bf_getbuffer = Some(std::mem::transmute(slot.pfunc));
            }

            #[cfg(all(not(Py_3_9), not(Py_LIMITED_API)))]
            if slot.slot == ffi::Py_bf_releasebuffer {
                // Safety: slot.pfunc is a valid function pointer
                buffer_procs.bf_releasebuffer = Some(std::mem::transmute(slot.pfunc));
            }
        }
        slots.extend_from_slice(proto_slots);
    });

    push_slot(&mut slots, 0, ptr::null_mut());
    let mut spec = ffi::PyType_Spec {
        name: py_class_qualified_name(module_name, name)?,
        basicsize: basicsize as c_int,
        itemsize: 0,
        flags: py_class_flags(has_gc_methods, is_gc, is_basetype),
        slots: slots.as_mut_ptr(),
    };

    let type_object = ffi::PyType_FromSpec(&mut spec);
    if type_object.is_null() {
        Err(PyErr::fetch(py))
    } else {
        tp_init_additional(
            type_object as _,
            tp_doc,
            #[cfg(all(not(Py_3_9), not(Py_LIMITED_API)))]
            &buffer_procs,
            #[cfg(not(Py_3_9))]
            dict_offset,
            #[cfg(not(Py_3_9))]
            weaklist_offset,
        );
        Ok(type_object as _)
    }
}

#[cold]
fn type_object_creation_failed(py: Python, e: PyErr, name: &'static str) -> ! {
    e.print(py);
    panic!("An error occurred while initializing class {}", name)
}

/// Additional type initializations necessary before Python 3.10
#[cfg(all(not(Py_LIMITED_API), not(Py_3_10)))]
unsafe fn tp_init_additional(
    type_object: *mut ffi::PyTypeObject,
    _tp_doc: &str,
    #[cfg(not(Py_3_9))] buffer_procs: &ffi::PyBufferProcs,
    #[cfg(not(Py_3_9))] dict_offset: Option<ffi::Py_ssize_t>,
    #[cfg(not(Py_3_9))] weaklist_offset: Option<ffi::Py_ssize_t>,
) {
    // Just patch the type objects for the things there's no
    // PyType_FromSpec API for... there's no reason this should work,
    // except for that it does and we have tests.

    // Running this causes PyPy to segfault.
    #[cfg(all(not(PyPy), not(Py_3_10)))]
    {
        if _tp_doc != "\0" {
            // Until CPython 3.10, tp_doc was treated specially for
            // heap-types, and it removed the text_signature value from it.
            // We go in after the fact and replace tp_doc with something
            // that _does_ include the text_signature value!
            ffi::PyObject_Free((*type_object).tp_doc as _);
            let data = ffi::PyObject_Malloc(_tp_doc.len());
            data.copy_from(_tp_doc.as_ptr() as _, _tp_doc.len());
            (*type_object).tp_doc = data as _;
        }
    }

    // Setting buffer protocols, tp_dictoffset and tp_weaklistoffset via slots doesn't work until
    // Python 3.9, so on older versions we must manually fixup the type object.
    #[cfg(not(Py_3_9))]
    {
        (*(*type_object).tp_as_buffer).bf_getbuffer = buffer_procs.bf_getbuffer;
        (*(*type_object).tp_as_buffer).bf_releasebuffer = buffer_procs.bf_releasebuffer;

        if let Some(dict_offset) = dict_offset {
            (*type_object).tp_dictoffset = dict_offset;
        }

        if let Some(weaklist_offset) = weaklist_offset {
            (*type_object).tp_weaklistoffset = weaklist_offset;
        }
    }
}

#[cfg(any(Py_LIMITED_API, Py_3_10))]
fn tp_init_additional(
    _type_object: *mut ffi::PyTypeObject,
    _tp_doc: &str,
    #[cfg(all(not(Py_3_9), not(Py_LIMITED_API)))] _buffer_procs: &ffi::PyBufferProcs,
    #[cfg(not(Py_3_9))] _dict_offset: Option<ffi::Py_ssize_t>,
    #[cfg(not(Py_3_9))] _weaklist_offset: Option<ffi::Py_ssize_t>,
) {
}

fn py_class_doc(class_doc: &str) -> Option<*mut c_char> {
    match class_doc {
        "\0" => None,
        s => {
            // To pass *mut pointer to python safely, leak a CString in whichever case
            let cstring = if s.as_bytes().last() == Some(&0) {
                CStr::from_bytes_with_nul(s.as_bytes())
                    .unwrap_or_else(|e| panic!("doc contains interior nul byte: {:?} in {}", e, s))
                    .to_owned()
            } else {
                CString::new(s)
                    .unwrap_or_else(|e| panic!("doc contains interior nul byte: {:?} in {}", e, s))
            };
            Some(cstring.into_raw())
        }
    }
}

fn py_class_qualified_name(module_name: Option<&str>, class_name: &str) -> PyResult<*mut c_char> {
    Ok(CString::new(format!(
        "{}.{}",
        module_name.unwrap_or("builtins"),
        class_name
    ))?
    .into_raw())
}

fn py_class_flags(has_gc_methods: bool, is_gc: bool, is_basetype: bool) -> c_uint {
    let mut flags = if has_gc_methods || is_gc {
        ffi::Py_TPFLAGS_DEFAULT | ffi::Py_TPFLAGS_HAVE_GC
    } else {
        ffi::Py_TPFLAGS_DEFAULT
    };
    if is_basetype {
        flags |= ffi::Py_TPFLAGS_BASETYPE;
    }

    // `c_ulong` and `c_uint` have the same size
    // on some platforms (like windows)
    #[allow(clippy::useless_conversion)]
    flags.try_into().unwrap()
}

fn py_class_method_defs(
    for_each_method_def: &dyn Fn(&mut dyn FnMut(&[PyMethodDefType])),
) -> Vec<ffi::PyMethodDef> {
    let mut defs = Vec::new();

    for_each_method_def(&mut |method_defs| {
        defs.extend(method_defs.iter().filter_map(|def| match def {
            PyMethodDefType::Method(def)
            | PyMethodDefType::Class(def)
            | PyMethodDefType::Static(def) => Some(def.as_method_def().unwrap()),
            _ => None,
        }));
    });

    if !defs.is_empty() {
        // Safety: Python expects a zeroed entry to mark the end of the defs
        defs.push(unsafe { std::mem::zeroed() });
    }

    defs
}

/// Generates the __dictoffset__ and __weaklistoffset__ members, to set tp_dictoffset and
/// tp_weaklistoffset.
///
/// Only works on Python 3.9 and up.
#[cfg(Py_3_9)]
fn py_class_members(
    dict_offset: Option<isize>,
    weaklist_offset: Option<isize>,
) -> Vec<ffi::structmember::PyMemberDef> {
    #[inline(always)]
    fn offset_def(name: &'static str, offset: ffi::Py_ssize_t) -> ffi::structmember::PyMemberDef {
        ffi::structmember::PyMemberDef {
            name: name.as_ptr() as _,
            type_code: ffi::structmember::T_PYSSIZET,
            offset,
            flags: ffi::structmember::READONLY,
            doc: std::ptr::null_mut(),
        }
    }

    let mut members = Vec::new();

    // __dict__ support
    if let Some(dict_offset) = dict_offset {
        members.push(offset_def("__dictoffset__\0", dict_offset));
    }

    // weakref support
    if let Some(weaklist_offset) = weaklist_offset {
        members.push(offset_def("__weaklistoffset__\0", weaklist_offset));
    }

    if !members.is_empty() {
        // Safety: Python expects a zeroed entry to mark the end of the defs
        members.push(unsafe { std::mem::zeroed() });
    }

    members
}

const PY_GET_SET_DEF_INIT: ffi::PyGetSetDef = ffi::PyGetSetDef {
    name: ptr::null_mut(),
    get: None,
    set: None,
    doc: ptr::null_mut(),
    closure: ptr::null_mut(),
};

fn py_class_properties(
    is_dummy: bool,
    for_each_method_def: &dyn Fn(&mut dyn FnMut(&[PyMethodDefType])),
) -> Vec<ffi::PyGetSetDef> {
    let mut defs = std::collections::HashMap::new();

    for_each_method_def(&mut |method_defs| {
        for def in method_defs {
            match def {
                PyMethodDefType::Getter(getter) => {
                    getter.copy_to(defs.entry(getter.name).or_insert(PY_GET_SET_DEF_INIT));
                }
                PyMethodDefType::Setter(setter) => {
                    setter.copy_to(defs.entry(setter.name).or_insert(PY_GET_SET_DEF_INIT));
                }
                _ => (),
            }
        }
    });

    let mut props: Vec<_> = defs.values().cloned().collect();

    // PyPy doesn't automatically adds __dict__ getter / setter.
    // PyObject_GenericGetDict not in the limited API until Python 3.10.
    push_dict_getset(&mut props, is_dummy);

    if !props.is_empty() {
        // Safety: Python expects a zeroed entry to mark the end of the defs
        props.push(unsafe { std::mem::zeroed() });
    }
    props
}

#[cfg(not(any(PyPy, all(Py_LIMITED_API, not(Py_3_10)))))]
fn push_dict_getset(props: &mut Vec<ffi::PyGetSetDef>, is_dummy: bool) {
    if !is_dummy {
        props.push(ffi::PyGetSetDef {
            name: "__dict__\0".as_ptr() as *mut c_char,
            get: Some(ffi::PyObject_GenericGetDict),
            set: Some(ffi::PyObject_GenericSetDict),
            doc: ptr::null_mut(),
            closure: ptr::null_mut(),
        });
    }
}

#[cfg(any(PyPy, all(Py_LIMITED_API, not(Py_3_10))))]
fn push_dict_getset(_: &mut Vec<ffi::PyGetSetDef>, _is_dummy: bool) {}
