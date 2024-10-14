use anyhow::{anyhow, bail, Context, Result};
use core::ffi::{c_char, c_uchar, c_uint};
use prost::Message;
use protobuf::license::*;
use std::ffi::{CStr, CString};
use std::path::Path;

#[derive(Debug)]
pub enum LicensedFeature {
    Mirroring,
    HA,
    Quota,
    Storagepool,
    Watch,
    RST,
    Copy,
    Index,
}

impl LicensedFeature {
    fn as_cstr(&self) -> &CStr {
        match self {
            LicensedFeature::Mirroring => c"io.beegfs.mirroring",
            LicensedFeature::HA => c"io.beegfs.ha",
            LicensedFeature::Quota => c"io.beegfs.quota",
            LicensedFeature::Storagepool => c"io.beegfs.storagepool",
            LicensedFeature::Watch => c"io.beegfs.watch",
            LicensedFeature::RST => c"io.beegfs.rst",
            LicensedFeature::Copy => c"io.beegfs.copy",
            LicensedFeature::Index => c"io.beegfs.index",
        }
    }
}

const NUM_MACHINES_PREFIX: &str = "io.beegfs.numservers.";
const NUM_MACHINES_UNLIMITED: &str = "unlimited";

/// Encapsulates a C string buffer and provides methods for easy access to the data inside the
/// buffer and automatic deallocation.
struct ExternalBuf {
    /// # Safety
    /// See [`CStr::from_ptr()`].
    /// Despite being *mut, might not be mutated while Self is hold.
    data: *mut c_char,
    /// # Safety
    /// must point to a function releasing the C-String behind `data`
    free: unsafe extern "C" fn(ptr: *mut c_char),
}

impl AsRef<[u8]> for ExternalBuf {
    fn as_ref(&self) -> &[u8] {
        // SAFETY:
        // `self.data` fulfills the requirements by the struct definitions contract
        unsafe { CStr::from_ptr(self.data).to_bytes() }
    }
}

impl Drop for ExternalBuf {
    fn drop(&mut self) {
        // SAFETY:
        // `self.free` fulfills the requirements by the struct definitions contract
        unsafe {
            (self.free)(self.data);
        }
    }
}

/// An abstraction for the BeeGFS licensing Go library that encapsulates all the ffi interactions
/// with it.
///
/// # Safety
///
/// * All functions pointers returning a `*mut c_char` must return a valid, nul terminated C-String
///   which must stay valid and may not written to until `free_return_buffer` is called.
/// * `free_return_buffer` must free C-Strings (`*mut c_char`) returned by the other functions.
#[derive(Debug)]
struct LoadedLibrary {
    #[allow(dead_code)]
    library: ::libloading::Library,

    init_cert_store: unsafe extern "C" fn() -> c_uchar,
    verify_pem: unsafe extern "C" fn(pem: *mut c_char, len: c_uint) -> *mut c_char,
    verify_file: unsafe extern "C" fn(path: *mut c_char) -> *mut c_char,
    get_loaded_cert_data: unsafe extern "C" fn() -> *mut c_char,
    verify_feature: unsafe extern "C" fn(feature: *mut c_char) -> *mut c_char,
    free_returned_buffer: unsafe extern "C" fn(ptr: *mut c_char),
}

impl LoadedLibrary {
    /// # Safety
    /// The signatures of the functions loaded from the dynamic library at `path` must match the
    /// function pointers defined in the LoadedLibrary struct.
    unsafe fn new(path: impl AsRef<Path>) -> Result<Self, ::libloading::Error> {
        // SAFETY:
        // Self::new() already requires fulfilling the Library::new() contract
        unsafe {
            let library = ::libloading::Library::new(path.as_ref())?;

            Ok(Self {
                init_cert_store: *library.get(b"InitCertStore\0")?,
                verify_pem: *library.get(b"VerifyPEM\0")?,
                verify_file: *library.get(b"VerifyFile\0")?,
                get_loaded_cert_data: *library.get(b"GetLoadedCertData\0")?,
                verify_feature: *library.get(b"VerifyFeature\0")?,
                free_returned_buffer: *library.get(b"FreeReturnedBuffer\0")?,

                library,
            })
        }
    }

    fn init_cert_store(&self) -> u8 {
        // SAFETY: Being valid fp and correct signatures assured by [`Self::new()`].
        unsafe { (self.init_cert_store)() }
    }

    fn verify_pem(&self, pem: &[u8]) -> Result<ExternalBuf> {
        let len = pem.len() as c_uint;
        let pem = pem.as_ptr() as *mut c_char;

        // SAFETY: Being valid fp and correct signatures assured by [`Self::new()`].
        unsafe {
            Ok(ExternalBuf {
                data: (self.verify_pem)(pem, len),
                free: self.free_returned_buffer,
            })
        }
    }

    #[allow(dead_code)]
    fn verify_file(&self, path: &Path) -> Result<ExternalBuf> {
        let path = CString::new(
            path.to_str()
                .ok_or_else(|| anyhow!("Couldn't convert cert path to C string."))?,
        )?;

        // SAFETY: Being valid fp and correct signatures assured by [`Self::new()`].
        unsafe {
            Ok(ExternalBuf {
                data: (self.verify_file)(path.as_ptr() as *mut c_char),
                free: self.free_returned_buffer,
            })
        }
    }

    fn get_loaded_cert_data(&self) -> ExternalBuf {
        // SAFETY: Being valid fp and correct signatures assured by [`Self::new()`].
        unsafe {
            ExternalBuf {
                data: (self.get_loaded_cert_data)(),
                free: self.free_returned_buffer,
            }
        }
    }

    fn verify_feature(&self, feature: LicensedFeature) -> ExternalBuf {
        // SAFETY: Being valid fp and correct signatures assured by [`Self::new()`].
        unsafe {
            ExternalBuf {
                data: (self.verify_feature)(feature.as_cstr().as_ptr() as *mut c_char),
                free: self.free_returned_buffer,
            }
        }
    }
}

#[derive(Debug)]
pub struct LicenseVerifier(Option<LoadedLibrary>);

impl LicenseVerifier {
    /// # Safety
    /// The signatures of the functions loaded from the dynamic library at `path` must match the
    /// function pointers defined in the LoadedLibrary struct.
    pub unsafe fn new(path: impl AsRef<Path>) -> Self {
        // SAFETY:
        // Self::new() already requires fulfilling the LoadedLibrary::new() contract
        match unsafe { LoadedLibrary::new(path) } {
            Ok(l) => {
                log::info!("Successfully initialized certificate verification library.");
                Self(Some(l))
            }
            Err(e) => {
                log::warn!("Failed to load license verification library: {e}");
                Self(None)
            }
        }
    }

    /// Loads and verifies a certificate from a file
    ///
    /// Checks whether the configured path is empty and if not, relays the path to the library,
    /// which loads and verifies the certificate.
    ///
    /// Returns a `String` that contains the certificate's serial number. That serial number is
    /// not required for subsequent operations on the certificate, because the library caches the
    /// last certificate it was asked to verify, regardless of verification success. In case of
    /// verification failure, returns an `Error` that contains the failure reason.
    pub async fn load_and_verify_cert(&self, cert_path: &Path) -> Result<String> {
        let Some(ref library) = self.0 else {
            bail!("License verification library not loaded.");
        };

        if cert_path.as_os_str().is_empty() {
            bail!("No license certificate configured");
        }

        library.init_cert_store();
        let pem = tokio::fs::read(cert_path)
            .await
            .with_context(|| format!("Reading certificate file {cert_path:?} failed"))?;

        let res = VerifyCertResult::decode(library.verify_pem(&pem)?.as_ref())?;
        let result = res.result();
        let serial = res.serial;
        let message = res.message;

        match result {
            VerifyResult::VerifyValid => {
                log::info!("Successfully loaded license certificate: {serial}");
                Ok(serial)
            }
            VerifyResult::VerifyInvalid => Err(anyhow!(message)),
            VerifyResult::VerifyError => Err(anyhow!(
                "Internal error during certificate verification: {message}"
            )),
            VerifyResult::VerifyUnspecified => Err(anyhow!("Unspecified result.")),
        }
    }

    /// Fetches contents of the certificate that was last verified
    ///
    /// Returns a `GetCertDataResult` that contains the data of the certificate that was last
    /// verified, regardless of verification success. This is useful for consumers like ctl that
    /// might be interested in certificate for invalid certificates. The `GetCertDataResult` will
    /// also contain the verification status. `Error`s are simply propagated.
    pub fn get_cert_data(&self) -> Result<GetCertDataResult> {
        let Some(ref library) = self.0 else {
            bail!("License verification library not loaded.");
        };

        let cert = GetCertDataResult::decode(library.get_loaded_cert_data().as_ref())?;
        Ok(cert)
    }

    /// Fetches the number of machines the license is valid for
    pub fn get_num_machines(&self) -> Result<u32> {
        let cert_data = match self
            .get_cert_data()
            .and_then(|e| e.data.ok_or_else(|| anyhow!("No certificate loaded")))
        {
            Ok(cert_data) => cert_data,
            Err(err) => {
                log::debug!(
                    "Could not obtain certificate data, defaulting to unlimited machines: {err:#}"
                );
                return Ok(u32::MAX);
            }
        };

        for name in cert_data.dns_names {
            if let Some(suffix) = name.strip_prefix(NUM_MACHINES_PREFIX) {
                if suffix == NUM_MACHINES_UNLIMITED {
                    return Ok(u32::MAX);
                } else {
                    return Ok(suffix.parse::<u32>()?);
                }
            }
        }

        bail!("Number of licensed servers not specified in certificate")
    }

    /// Verifies a specific licensed feature
    ///
    /// Returns `Ok(())` in case of verification success and an `Error` that contains the reason for
    /// verification failure otherwise.
    pub fn verify_feature(&self, feature: LicensedFeature) -> Result<()> {
        let Some(ref library) = self.0 else {
            bail!("License verification library not loaded. Feature {feature:?} unavailable.");
        };

        let res = VerifyFeatureResult::decode(library.verify_feature(feature).as_ref())?;
        let result = res.result();
        let message = res.message;

        match result {
            VerifyResult::VerifyValid => Ok(()),
            VerifyResult::VerifyInvalid => Err(anyhow!(message)),
            VerifyResult::VerifyError => Err(anyhow!(
                "Internal error during feature verification: {message}"
            )),
            VerifyResult::VerifyUnspecified => Err(anyhow!("Unspecified result.")),
        }
    }
}
