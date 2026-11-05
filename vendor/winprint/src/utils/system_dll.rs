//! Optional Windows components, looked up when a job needs them instead of imported.
//!
//! The `windows` crate imports every function it wraps in the executable's import table, so a
//! program calling one of them directly does not start at all where its DLL is missing ("The code
//! execution cannot proceed because X.dll was not found"). The print stack (`windows.data.pdf.dll`,
//! `prntvpt.dll`, `xpsprint.dll`) is absent from some N/KN, LTSC, Server and trimmed-down
//! installations, so these drop-in replacements for the `windows` wrappers load it from System32
//! when called and return the loader's error where it is missing. Only printing fails there.
//! Weave Office addition; `scripts/check-windows-imports.py` keeps such DLLs out of the import table.
#![allow(non_snake_case)]

use windows::core::{Error, Param, Result, BSTR, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Gdi::DEVMODEA;
use windows::Win32::Graphics::Printing::PrintTicket::{
    EDefaultDevmodeType, EPrintTicketScope, HPTPROVIDER,
};
use windows::Win32::Storage::Xps::Printing::{IXpsPrintJob, IXpsPrintJobStream};
use windows::Win32::System::Com::IStream;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};

type Raw = *mut core::ffi::c_void;

/// `name` from the System32 DLL `dll` (never the program's folder or the current directory).
/// The module stays loaded for the life of the process, as a static import would.
pub(crate) unsafe fn proc_address(
    dll: PCWSTR,
    name: PCSTR,
) -> Result<unsafe extern "system" fn() -> isize> {
    let module = LoadLibraryExW(dll, None, LOAD_LIBRARY_SEARCH_SYSTEM32)?;
    GetProcAddress(module, name).ok_or_else(Error::from_thread)
}

/// Looks up `$name` in `$dll` as the function pointer type `$ty`, returning early on failure.
macro_rules! lookup {
    ($dll:literal, $name:literal, $ty:ty) => {
        core::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(
            $crate::utils::system_dll::proc_address(
                windows::core::w!($dll),
                windows::core::s!($name),
            )?,
        )
    };
}

pub(crate) unsafe fn PTOpenProvider<P0: Param<PCWSTR>>(
    printer: P0,
    version: u32,
) -> Result<HPTPROVIDER> {
    let f = lookup!(
        "prntvpt.dll",
        "PTOpenProvider",
        unsafe extern "system" fn(PCWSTR, u32, *mut HPTPROVIDER) -> HRESULT
    );
    let mut provider = HPTPROVIDER::default();
    f(printer.param().abi(), version, &mut provider).map(|| provider)
}

pub(crate) unsafe fn PTCloseProvider(provider: HPTPROVIDER) -> Result<()> {
    let f = lookup!(
        "prntvpt.dll",
        "PTCloseProvider",
        unsafe extern "system" fn(HPTPROVIDER) -> HRESULT
    );
    f(provider).ok()
}

pub(crate) unsafe fn PTReleaseMemory(buffer: *const core::ffi::c_void) -> Result<()> {
    let f = lookup!(
        "prntvpt.dll",
        "PTReleaseMemory",
        unsafe extern "system" fn(*const core::ffi::c_void) -> HRESULT
    );
    f(buffer).ok()
}

pub(crate) unsafe fn PTConvertPrintTicketToDevMode<P1: Param<IStream>>(
    provider: HPTPROVIDER,
    ticket: P1,
    base: EDefaultDevmodeType,
    scope: EPrintTicketScope,
    size: *mut u32,
    dev_mode: *mut *mut DEVMODEA,
    error_message: Option<*mut BSTR>,
) -> Result<()> {
    let f = lookup!(
        "prntvpt.dll",
        "PTConvertPrintTicketToDevMode",
        unsafe extern "system" fn(
            HPTPROVIDER,
            Raw,
            EDefaultDevmodeType,
            EPrintTicketScope,
            *mut u32,
            *mut *mut DEVMODEA,
            *mut Raw,
        ) -> HRESULT
    );
    f(
        provider,
        ticket.param().abi(),
        base,
        scope,
        size,
        dev_mode,
        error_message.map_or(core::ptr::null_mut(), |p| p.cast()),
    )
    .ok()
}

pub(crate) unsafe fn PTGetPrintCapabilities<P1: Param<IStream>, P2: Param<IStream>>(
    provider: HPTPROVIDER,
    ticket: P1,
    capabilities: P2,
    error_message: Option<*mut BSTR>,
) -> Result<()> {
    let f = lookup!(
        "prntvpt.dll",
        "PTGetPrintCapabilities",
        unsafe extern "system" fn(HPTPROVIDER, Raw, Raw, *mut Raw) -> HRESULT
    );
    f(
        provider,
        ticket.param().abi(),
        capabilities.param().abi(),
        error_message.map_or(core::ptr::null_mut(), |p| p.cast()),
    )
    .ok()
}

pub(crate) unsafe fn PTMergeAndValidatePrintTicket<
    P1: Param<IStream>,
    P2: Param<IStream>,
    P4: Param<IStream>,
>(
    provider: HPTPROVIDER,
    base: P1,
    delta: P2,
    scope: EPrintTicketScope,
    result: P4,
    error_message: Option<*mut BSTR>,
) -> Result<()> {
    let f = lookup!(
        "prntvpt.dll",
        "PTMergeAndValidatePrintTicket",
        unsafe extern "system" fn(HPTPROVIDER, Raw, Raw, EPrintTicketScope, Raw, *mut Raw) -> HRESULT
    );
    f(
        provider,
        base.param().abi(),
        delta.param().abi(),
        scope,
        result.param().abi(),
        error_message.map_or(core::ptr::null_mut(), |p| p.cast()),
    )
    .ok()
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn StartXpsPrintJob<P0: Param<PCWSTR>, P1: Param<PCWSTR>, P2: Param<PCWSTR>>(
    printer: P0,
    job: P1,
    output: P2,
    progress: HANDLE,
    completion: HANDLE,
    printable_pages_on: &[u8],
    xps_print_job: *mut Option<IXpsPrintJob>,
    document_stream: *mut Option<IXpsPrintJobStream>,
    print_ticket_stream: *mut Option<IXpsPrintJobStream>,
) -> Result<()> {
    let f = lookup!(
        "xpsprint.dll",
        "StartXpsPrintJob",
        unsafe extern "system" fn(
            PCWSTR,
            PCWSTR,
            PCWSTR,
            HANDLE,
            HANDLE,
            *const u8,
            u32,
            *mut Raw,
            *mut Raw,
            *mut Raw,
        ) -> HRESULT
    );
    f(
        printer.param().abi(),
        job.param().abi(),
        output.param().abi(),
        progress,
        completion,
        printable_pages_on.as_ptr(),
        printable_pages_on.len().try_into().unwrap(),
        xps_print_job.cast(),
        document_stream.cast(),
        print_ticket_stream.cast(),
    )
    .ok()
}
