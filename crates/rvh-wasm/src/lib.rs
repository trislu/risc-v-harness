//! C ABI over [`rvh_session::Session`].
//!
//! The wasm target exports these functions directly, so the browser and the
//! extension host call them through `WebAssembly.Instance` and read results from
//! linear memory. `wasm-bindgen` is deliberately not used: it is an external
//! dependency, and the build must work offline. Nothing here is wasm-specific,
//! so the same functions are callable natively and tested that way.
//!
//! Protocol.
//!
//! * A call that returns data stores it in the session's result buffer and
//!   returns the length.
//! * The host then reads `rvh_result_ptr` and the returned length.
//! * Hot data (registers, RAM) has its own pointer, so a host can read it
//!   without a JSON round trip.

#![allow(unsafe_code)]

use std::ptr;
use std::slice;

use rvh_isa::Target;
use rvh_session::Session;

#[derive(Debug)]
pub struct FfiSession {
    session: Session,
    result: Vec<u8>,
}

impl FfiSession {
    fn publish(&mut self, text: String) -> usize {
        self.result = text.into_bytes();
        self.result.len()
    }
}

/// Allocate `len` bytes in linear memory, so the host can write a string there.
///
/// The host must pass the same `len` to [`rvh_dealloc`].
#[no_mangle]
pub extern "C" fn rvh_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return ptr::null_mut();
    }
    let Ok(layout) = std::alloc::Layout::from_size_align(len, 1) else {
        return ptr::null_mut();
    };
    unsafe { std::alloc::alloc(layout) }
}

/// Free a buffer obtained from [`rvh_alloc`].
///
/// # Safety
///
/// `pointer` and `len` must come from the same [`rvh_alloc`] call.
#[no_mangle]
pub unsafe extern "C" fn rvh_dealloc(pointer: *mut u8, len: usize) {
    if pointer.is_null() || len == 0 {
        return;
    }
    let Ok(layout) = std::alloc::Layout::from_size_align(len, 1) else {
        return;
    };
    unsafe { std::alloc::dealloc(pointer, layout) };
}

/// Create a session for a target string such as `rv32i_zicsr`.
///
/// Returns null when the target is not known.
///
/// # Safety
///
/// `target` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn rvh_new(target: *const u8, len: usize) -> *mut FfiSession {
    let Some(bytes) = (unsafe { readable(target, len) }) else {
        return ptr::null_mut();
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return ptr::null_mut();
    };
    let Ok(target) = Target::parse(text) else {
        return ptr::null_mut();
    };
    Box::into_raw(Box::new(FfiSession {
        session: Session::new(target),
        result: Vec::new(),
    }))
}

/// Destroy a session created by [`rvh_new`].
///
/// # Safety
///
/// `session` must come from [`rvh_new`] and must not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn rvh_free(session: *mut FfiSession) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// Replace the document. Returns the length of the diagnostics JSON.
///
/// # Safety
///
/// `source` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn rvh_set_source(
    session: *mut FfiSession,
    source: *const u8,
    len: usize,
) -> usize {
    let Some(session) = (unsafe { session_mut(session) }) else {
        return 0;
    };
    let Some(bytes) = (unsafe { readable(source, len) }) else {
        return 0;
    };
    session.session.set_source(&String::from_utf8_lossy(bytes));
    let json = session.session.diagnostics_json();
    session.publish(json)
}

/// Execute up to `count` instructions. Returns the length of the state JSON.
///
/// # Safety
///
/// `session` must come from [`rvh_new`].
#[no_mangle]
pub unsafe extern "C" fn rvh_step(session: *mut FfiSession, count: u32) -> usize {
    let Some(session) = (unsafe { session_mut(session) }) else {
        return 0;
    };
    session.session.step(count);
    let json = session.session.state_json();
    session.publish(json)
}

/// Undo up to `count` instructions. Returns the length of the state JSON.
///
/// # Safety
///
/// `session` must come from [`rvh_new`].
#[no_mangle]
pub unsafe extern "C" fn rvh_undo(session: *mut FfiSession, count: u32) -> usize {
    let Some(session) = (unsafe { session_mut(session) }) else {
        return 0;
    };
    session.session.undo(count);
    let json = session.session.state_json();
    session.publish(json)
}

/// Run to a halt or to `max_steps`. Returns the length of the state JSON.
///
/// `max_steps` is a `u32` on purpose: an `i64` parameter forces the JavaScript
/// host to pass a `BigInt`, which is a trap for every caller.
///
/// # Safety
///
/// `session` must come from [`rvh_new`].
#[no_mangle]
pub unsafe extern "C" fn rvh_run(session: *mut FfiSession, max_steps: u32) -> usize {
    let Some(session) = (unsafe { session_mut(session) }) else {
        return 0;
    };
    session.session.run(u64::from(max_steps));
    let json = session.session.state_json();
    session.publish(json)
}

/// Reload the program from the start. Returns the length of the state JSON.
///
/// # Safety
///
/// `session` must come from [`rvh_new`].
#[no_mangle]
pub unsafe extern "C" fn rvh_reset(session: *mut FfiSession) -> usize {
    let Some(session) = (unsafe { session_mut(session) }) else {
        return 0;
    };
    session.session.reset();
    let json = session.session.state_json();
    session.publish(json)
}

/// Current state as JSON. Returns the length of the JSON.
///
/// # Safety
///
/// `session` must come from [`rvh_new`].
#[no_mangle]
pub unsafe extern "C" fn rvh_state(session: *mut FfiSession) -> usize {
    let Some(session) = (unsafe { session_mut(session) }) else {
        return 0;
    };
    let json = session.session.state_json();
    session.publish(json)
}

/// Set the content locale, such as `zh-CN`. Returns the length of the state JSON.
///
/// # Safety
///
/// `session` must come from [`rvh_new`]; `locale` must point to `len` readable
/// bytes.
#[no_mangle]
pub unsafe extern "C" fn rvh_set_locale(
    session: *mut FfiSession,
    locale: *const u8,
    len: usize,
) -> usize {
    let Some(session) = (unsafe { session_mut(session) }) else {
        return 0;
    };
    let Some(bytes) = (unsafe { readable(locale, len) }) else {
        return 0;
    };
    session.session.set_locale(&String::from_utf8_lossy(bytes));
    let json = session.session.state_json();
    session.publish(json)
}

/// Hover at a byte offset. Returns the length of the hover JSON.
///
/// # Safety
///
/// `session` must come from [`rvh_new`].
#[no_mangle]
pub unsafe extern "C" fn rvh_hover(session: *mut FfiSession, offset: usize) -> usize {
    let Some(session) = (unsafe { session_mut(session) }) else {
        return 0;
    };
    let json = session.session.hover_json(offset);
    session.publish(json)
}

/// Pointer to the result buffer written by the last call.
///
/// # Safety
///
/// `session` must come from [`rvh_new`]. The pointer is invalidated by the next
/// call that writes a result.
#[no_mangle]
pub unsafe extern "C" fn rvh_result_ptr(session: *const FfiSession) -> *const u8 {
    match unsafe { session_ref(session) } {
        Some(session) => session.result.as_ptr(),
        None => ptr::null(),
    }
}

/// Backing RAM, for a zero-copy memory view.
///
/// # Safety
///
/// `session` must come from [`rvh_new`]; the pointer is valid until the document
/// changes.
#[no_mangle]
pub unsafe extern "C" fn rvh_mem_ptr(session: *const FfiSession) -> *const u8 {
    match unsafe { session_ref(session) }.and_then(|session| session.session.machine()) {
        Some(machine) => machine.ram().as_ptr(),
        None => ptr::null(),
    }
}

/// Length of the RAM view.
///
/// # Safety
///
/// `session` must come from [`rvh_new`].
#[no_mangle]
pub unsafe extern "C" fn rvh_mem_len(session: *const FfiSession) -> usize {
    match unsafe { session_ref(session) }.and_then(|session| session.session.machine()) {
        Some(machine) => machine.ram().len(),
        None => 0,
    }
}

/// Registers, for a zero-copy view of 32 `u32` values.
///
/// # Safety
///
/// `session` must come from [`rvh_new`]; the pointer is valid until the machine
/// is reset.
#[no_mangle]
pub unsafe extern "C" fn rvh_regs_ptr(session: *const FfiSession) -> *const u32 {
    match unsafe { session_ref(session) }.and_then(|session| session.session.machine()) {
        Some(machine) => machine.regs().as_ptr(),
        None => ptr::null(),
    }
}

unsafe fn readable<'a>(data: *const u8, len: usize) -> Option<&'a [u8]> {
    // A zero-length slice is legitimate: the document can be empty.
    if len == 0 {
        return Some(&[]);
    }
    if data.is_null() {
        return None;
    }
    Some(unsafe { slice::from_raw_parts(data, len) })
}

unsafe fn session_mut<'a>(session: *mut FfiSession) -> Option<&'a mut FfiSession> {
    if session.is_null() {
        return None;
    }
    Some(unsafe { &mut *session })
}

unsafe fn session_ref<'a>(session: *const FfiSession) -> Option<&'a FfiSession> {
    if session.is_null() {
        return None;
    }
    Some(unsafe { &*session })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(session: *const FfiSession, len: usize) -> String {
        let pointer = unsafe { rvh_result_ptr(session) };
        assert!(!pointer.is_null());
        let bytes = unsafe { slice::from_raw_parts(pointer, len) };
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[test]
    fn a_full_round_trip_through_the_c_abi() {
        let target = b"rv32i_zicsr";
        let session = unsafe { rvh_new(target.as_ptr(), target.len()) };
        assert!(!session.is_null());

        let source = b"li a0, 5\nebreak\n";
        let len = unsafe { rvh_set_source(session, source.as_ptr(), source.len()) };
        let diagnostics = result(session, len);
        assert!(diagnostics.contains("\"ok\":true"), "{diagnostics}");

        let len = unsafe { rvh_step(session, 1) };
        let state = result(session, len);
        assert!(state.contains("\"new\":5"), "{state}");

        let len = unsafe { rvh_run(session, 10) };
        let state = result(session, len);
        assert!(state.contains("\"halt\":\"ebreak\""), "{state}");

        let registers = unsafe { rvh_regs_ptr(session) };
        assert!(!registers.is_null());
        let registers = unsafe { slice::from_raw_parts(registers, 32) };
        assert_eq!(registers[10], 5);

        let memory = unsafe { rvh_mem_ptr(session) };
        assert!(!memory.is_null());
        assert!(unsafe { rvh_mem_len(session) } >= 0x4_0000);

        unsafe { rvh_free(session) };
    }

    #[test]
    fn hover_travels_through_the_c_abi() {
        let target = b"rv32i_zicsr";
        let session = unsafe { rvh_new(target.as_ptr(), target.len()) };
        let source = b"addi a0, a0, 1\n";
        unsafe { rvh_set_source(session, source.as_ptr(), source.len()) };

        let len = unsafe { rvh_hover(session, 1) };
        let json = result(session, len);
        assert!(json.contains("\"kind\":\"instruction\""), "{json}");
        assert!(json.contains("format `I`"), "{json}");
        unsafe { rvh_free(session) };
    }

    #[test]
    fn the_host_can_allocate_a_string_buffer() {
        let pointer = rvh_alloc(8);
        assert!(!pointer.is_null());
        unsafe {
            std::slice::from_raw_parts_mut(pointer, 8).copy_from_slice(b"rv32i_zi");
            rvh_dealloc(pointer, 8);
        }
        assert!(rvh_alloc(0).is_null());
    }

    #[test]
    fn an_empty_document_is_valid() {
        let target = b"rv32i_zicsr";
        let session = unsafe { rvh_new(target.as_ptr(), target.len()) };
        let len = unsafe { rvh_set_source(session, std::ptr::null(), 0) };
        let json = result(session, len);
        assert!(json.contains("\"ok\":true"), "{json}");
        unsafe { rvh_free(session) };
    }

    #[test]
    fn a_bad_target_returns_null() {
        let target = b"rv999z";
        let session = unsafe { rvh_new(target.as_ptr(), target.len()) };
        assert!(session.is_null());
    }

    #[test]
    fn diagnostics_travel_as_json() {
        let target = b"rv32i_zicsr";
        let session = unsafe { rvh_new(target.as_ptr(), target.len()) };
        let source = b"adii a0, a1, 1\n";
        let len = unsafe { rvh_set_source(session, source.as_ptr(), source.len()) };
        let json = result(session, len);
        assert!(json.contains("sem.unknown-instruction"), "{json}");
        assert!(json.contains("did you mean `addi`"), "{json}");
        unsafe { rvh_free(session) };
    }
}
