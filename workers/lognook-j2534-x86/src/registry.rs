#[cfg(windows)]
use crate::j2534::J2534Failure;
#[cfg(windows)]
use lognook_j2534_protocol::WorkerErrorKind;
use lognook_j2534_protocol::{J2534Provider, J2534ProviderDiscovery};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegisteredProvider {
    pub provider: J2534Provider,
    pub dll_path: PathBuf,
}

pub(crate) fn is_target_provider_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["tactrix", "openport", "opta"]
        .iter()
        .any(|needle| name.contains(needle))
}

pub(crate) fn make_registered_provider(
    id: String,
    name: String,
    registered_path: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> Option<RegisteredProvider> {
    if !is_target_provider_name(&name) {
        return None;
    }
    let expanded = expand_environment_variables(registered_path, lookup)?;
    let dll_path = PathBuf::from(expanded);
    dll_path.is_file().then_some(RegisteredProvider {
        provider: J2534Provider { id, name },
        dll_path,
    })
}

fn expand_environment_variables(
    value: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let mut result = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        result.push_str(&rest[..start]);
        let variable = &rest[start + 1..];
        let end = variable.find('%')?;
        if end == 0 {
            return None;
        }
        result.push_str(&lookup(&variable[..end])?);
        rest = &variable[end + 1..];
    }
    result.push_str(rest);
    Some(result)
}

#[cfg(windows)]
pub(crate) fn list_openport2_providers() -> Result<J2534ProviderDiscovery, J2534Failure> {
    enumerate_registered_providers()
}

fn ignored_provider_diagnostic(id: &str, name: &str) -> String {
    format!(
        "Ignored registered J2534 provider '{name}' ({id}): LogNook supports Tactrix/OpenPort/OPTA only."
    )
}

#[cfg(windows)]
pub(crate) fn resolve_openport2_provider(
    provider_id: &str,
) -> Result<RegisteredProvider, J2534Failure> {
    if provider_id.is_empty() || provider_id.contains(['\\', '\0']) {
        return Err(J2534Failure::new(
            WorkerErrorKind::ProviderNotFound,
            "The selected J2534 provider is not registered",
        ));
    }
    let key = open_j2534_registry()?.ok_or_else(|| {
        J2534Failure::new(
            WorkerErrorKind::ProviderNotFound,
            "No 32-bit J2534 04.04 providers are registered",
        )
    })?;
    let name =
        read_registry_string(key.0, provider_id, "Name")?.unwrap_or_else(|| provider_id.into());
    if !is_target_provider_name(&name) {
        return Err(J2534Failure::new(
            WorkerErrorKind::ProviderNotFound,
            "The selected provider is not an OpenPort/Tactrix/OPTA adapter",
        ));
    }
    let path = read_registry_string(key.0, provider_id, "FunctionLibrary")?.ok_or_else(|| {
        J2534Failure::new(
            WorkerErrorKind::DllNotFound,
            "The registered provider has no FunctionLibrary path",
        )
    })?;
    let expanded = expand_environment_variables(&path, |variable| std::env::var(variable).ok())
        .ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::DllNotFound,
                "The registered provider DLL path contains an unknown environment variable",
            )
        })?;
    let dll_path = PathBuf::from(expanded);
    if !dll_path.is_file() {
        return Err(J2534Failure::new(
            WorkerErrorKind::DllNotFound,
            format!("Registered provider DLL is missing: {}", dll_path.display()),
        ));
    }
    Ok(RegisteredProvider {
        provider: J2534Provider {
            id: provider_id.into(),
            name,
        },
        dll_path,
    })
}

#[cfg(windows)]
fn enumerate_registered_providers() -> Result<J2534ProviderDiscovery, J2534Failure> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::ERROR_NO_MORE_ITEMS,
        System::Registry::{HKEY, RegEnumKeyExW},
    };

    let Some(key) = open_j2534_registry()? else {
        return Ok(J2534ProviderDiscovery {
            providers: Vec::new(),
            diagnostics: vec!["No 32-bit J2534 04.04 registry entries were found.".into()],
        });
    };
    let root: HKEY = key.0;

    let mut providers = Vec::new();
    let mut diagnostics = Vec::new();
    for index in 0..u32::MAX {
        let mut subkey = [0u16; 512];
        let mut length = subkey.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                root,
                index,
                subkey.as_mut_ptr(),
                &mut length,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status != 0 {
            return Err(J2534Failure::new(
                WorkerErrorKind::IpcFailure,
                format!("Could not enumerate J2534 providers: 0x{status:08X}"),
            ));
        }
        let id = String::from_utf16_lossy(&subkey[..length as usize]);
        let name = read_registry_string(root, &id, "Name")?.unwrap_or_else(|| id.clone());
        if !is_target_provider_name(&name) {
            diagnostics.push(ignored_provider_diagnostic(&id, &name));
            continue;
        }
        let Some(path) = read_registry_string(root, &id, "FunctionLibrary")? else {
            diagnostics.push(format!(
                "Ignored registered provider '{name}': it has no FunctionLibrary path."
            ));
            continue;
        };
        if let Some(provider) = make_registered_provider(id, name.clone(), &path, |variable| {
            std::env::var(variable).ok()
        }) {
            providers.push(provider.provider);
        } else {
            diagnostics.push(format!(
                "Ignored registered provider '{name}': its FunctionLibrary path could not be resolved or the DLL is missing."
            ));
        }
    }
    if providers.is_empty() && diagnostics.is_empty() {
        diagnostics
            .push("No registered J2534 provider is eligible for OpenPort 2 connection.".into());
    }
    Ok(J2534ProviderDiscovery {
        providers,
        diagnostics,
    })
}

#[cfg(windows)]
fn read_registry_string(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
    value_name: &str,
) -> Result<Option<String>, J2534Failure> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::ERROR_FILE_NOT_FOUND,
        System::Registry::{RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW},
    };

    let subkey = wide(subkey);
    let value_name = wide(value_name);
    let flags = RRF_NOEXPAND | RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut value_type = 0;
    let mut byte_length = 0;
    let status = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value_name.as_ptr(),
            flags,
            &mut value_type,
            ptr::null_mut(),
            &mut byte_length,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != 0 || byte_length == 0 || byte_length > 8192 || byte_length % 2 != 0 {
        return Err(J2534Failure::new(
            WorkerErrorKind::IpcFailure,
            format!("Could not read J2534 registry value '{value_name:?}': 0x{status:08X}"),
        ));
    }
    let mut data = vec![0u16; byte_length as usize / 2];
    let status = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value_name.as_ptr(),
            flags,
            &mut value_type,
            data.as_mut_ptr().cast(),
            &mut byte_length,
        )
    };
    if status != 0 {
        return Err(J2534Failure::new(
            WorkerErrorKind::IpcFailure,
            format!("Could not read J2534 registry value: 0x{status:08X}"),
        ));
    }
    let length = data
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(data.len());
    String::from_utf16(&data[..length])
        .map(Some)
        .map_err(|error| {
            J2534Failure::new(
                WorkerErrorKind::IpcFailure,
                format!("J2534 registry string is invalid UTF-16: {error}"),
            )
        })
}

#[cfg(windows)]
struct RegistryKey(windows_sys::Win32::System::Registry::HKEY);

#[cfg(windows)]
impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Registry::RegCloseKey(self.0);
        }
    }
}

#[cfg(windows)]
fn open_j2534_registry() -> Result<Option<RegistryKey>, J2534Failure> {
    use windows_sys::Win32::{
        Foundation::ERROR_FILE_NOT_FOUND,
        System::Registry::{HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, RegOpenKeyExW},
    };
    let path = wide("SOFTWARE\\PassThruSupport.04.04");
    let mut key: HKEY = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            0,
            KEY_READ | KEY_WOW64_32KEY,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != 0 {
        return Err(J2534Failure::new(
            WorkerErrorKind::IpcFailure,
            format!("Could not open 32-bit J2534 registry: 0x{status:08X}"),
        ));
    }
    Ok(Some(RegistryKey(key)))
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        RegisteredProvider, ignored_provider_diagnostic, is_target_provider_name,
        list_openport2_providers, make_registered_provider, resolve_openport2_provider,
    };
    use lognook_j2534_protocol::WorkerErrorKind;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn only_tactrix_openport_and_opta_names_are_eligible() {
        assert!(is_target_provider_name("Tactrix Inc. OpenPort 2.0"));
        assert!(is_target_provider_name("OPTA Tactrix J2534"));
        assert!(!is_target_provider_name("DENSO CORPORATION - DST-010"));
        assert!(!is_target_provider_name("Generic PassThru Device"));
    }

    #[test]
    fn unsupported_registered_provider_diagnostic_explains_the_filter() {
        let diagnostic = ignored_provider_diagnostic("DENSO CORPORATION - DST-010", "DST-010");

        assert!(diagnostic.contains("DENSO CORPORATION - DST-010"));
        assert!(diagnostic.contains("DST-010"));
        assert!(diagnostic.contains("Tactrix/OpenPort/OPTA"));
    }

    #[test]
    fn registry_environment_path_is_expanded_and_missing_dll_is_rejected() {
        let root = std::env::temp_dir().join(format!(
            "lognook-j2534-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let dll = root.join("opta_j2534.dll");
        fs::write(&dll, b"fixture").unwrap();
        let root_string = root.to_string_lossy().into_owned();
        let lookup = |key: &str| (key == "LN_TEST_ROOT").then(|| root_string.clone());

        let provider: RegisteredProvider = make_registered_provider(
            "opta".into(),
            "OPTA Tactrix OpenPort 2.0".into(),
            "%LN_TEST_ROOT%\\opta_j2534.dll",
            lookup,
        )
        .unwrap();
        assert_eq!(provider.provider.id, "opta");
        assert_eq!(provider.dll_path, dll);

        assert!(
            make_registered_provider(
                "missing".into(),
                "Tactrix OpenPort".into(),
                "%LN_TEST_ROOT%\\missing.dll",
                |key| (key == "LN_TEST_ROOT").then(|| root_string.clone()),
            )
            .is_none()
        );
        assert!(
            make_registered_provider(
                "denso".into(),
                "DENSO CORPORATION - DST-010".into(),
                "%LN_TEST_ROOT%\\opta_j2534.dll",
                |key| (key == "LN_TEST_ROOT").then(|| root_string.clone()),
            )
            .is_none()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_registry_listing_filters_names_and_rejects_unknown_provider_keys() {
        let providers = list_openport2_providers().unwrap();
        assert!(
            providers
                .providers
                .iter()
                .all(|provider| is_target_provider_name(&provider.name))
        );
        assert!(!providers.diagnostics.is_empty() || !providers.providers.is_empty());
        let error = resolve_openport2_provider("__lognook_unregistered_provider__").unwrap_err();
        assert_eq!(error.kind, WorkerErrorKind::ProviderNotFound);
    }

    #[cfg(windows)]
    #[test]
    fn resolves_registered_provider_ids_with_forward_slashes() {
        let providers = list_openport2_providers().unwrap();
        let Some(provider) = providers
            .providers
            .iter()
            .find(|provider| provider.id.contains('/'))
        else {
            return;
        };

        let resolved = resolve_openport2_provider(&provider.id)
            .expect("a listed provider ID with forward slashes should resolve");
        assert_eq!(resolved.provider, *provider);
    }
}
