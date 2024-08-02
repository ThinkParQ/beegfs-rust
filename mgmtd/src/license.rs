use anyhow::{anyhow, bail, Result};
use prost::Message;
use protobuf::license::*;
use std::ffi::{CStr, CString};
use std::path::Path;

#[derive(Debug)]
pub enum LicensedFeature {
    Mirroring,
    HA,
    Quota,
    ACL,
    Storagepool,
    Events,
    Watch,
    Flex,
}

impl AsRef<CStr> for LicensedFeature {
    fn as_ref(&self) -> &CStr {
        match self {
            LicensedFeature::Mirroring => c"io.beegfs.mirroring",
            LicensedFeature::HA => c"io.beegfs.ha",
            LicensedFeature::Quota => c"io.beegfs.quota",
            LicensedFeature::ACL => c"io.beegfs.acl",
            LicensedFeature::Storagepool => c"io.beegfs.storagepool",
            LicensedFeature::Events => c"io.beegfs.events",
            LicensedFeature::Watch => c"io.beegfs.watch",
            LicensedFeature::Flex => c"io.beegfs.flex",
        }
    }
}

/// Encapsulates a C string buffer and provides methods for easy access to the data inside the
/// buffer and automatic deallocation
struct ExternalBuf {
    data: *mut ::std::os::raw::c_char,
    free: unsafe extern "C" fn(ptr: *mut ::std::os::raw::c_char),
}

impl AsRef<[u8]> for ExternalBuf {
    fn as_ref(&self) -> &[u8] {
        unsafe { CStr::from_ptr(self.data).to_bytes() }
    }
}

impl Drop for ExternalBuf {
    fn drop(&mut self) {
        unsafe {
            (self.free)(self.data);
        }
    }
}

/// An abstraction for the Go library that encapsulates all unsafe operations
#[derive(Debug)]
struct LoadedLibrary {
    #[allow(dead_code)]
    library: ::libloading::Library,
    init_cert_store: unsafe extern "C" fn() -> core::ffi::c_uchar,
    verify_pem: unsafe extern "C" fn(
        pem: *mut core::ffi::c_char,
        len: core::ffi::c_uint,
    ) -> *mut core::ffi::c_char,
    verify_file: unsafe extern "C" fn(path: *mut core::ffi::c_char) -> *mut core::ffi::c_char,
    get_loaded_cert_data: unsafe extern "C" fn() -> *mut core::ffi::c_char,
    verify_feature: unsafe extern "C" fn(feature: *mut core::ffi::c_char) -> *mut core::ffi::c_char,
    free_returned_buffer: unsafe extern "C" fn(ptr: *mut core::ffi::c_char),
}

impl LoadedLibrary {
    fn new(path: impl AsRef<Path>) -> Result<Self, ::libloading::Error> {
        unsafe {
            let library = ::libloading::Library::new(path.as_ref())?;
            Ok(Self {
                init_cert_store: library.get(b"InitCertStore\0").map(|sym| *sym)?,
                verify_pem: library.get(b"VerifyPEM\0").map(|sym| *sym)?,
                verify_file: library.get(b"VerifyFile\0").map(|sym| *sym)?,
                get_loaded_cert_data: library.get(b"GetLoadedCertData\0").map(|sym| *sym)?,
                verify_feature: library.get(b"VerifyFeature\0").map(|sym| *sym)?,
                free_returned_buffer: library.get(b"FreeReturnedBuffer\0").map(|sym| *sym)?,
                library,
            })
        }
    }

    fn init_cert_store(&self) -> core::ffi::c_uchar {
        unsafe { (self.init_cert_store)() }
    }

    fn verify_pem(&self, pem: &[u8]) -> Result<ExternalBuf> {
        let len = pem.len() as core::ffi::c_uint;
        let pem = pem.as_ptr() as *mut core::ffi::c_char;

        unsafe {
            let data = (self.verify_pem)(pem, len);
            Ok(ExternalBuf {
                data,
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
        let ppath = path.as_ptr() as *mut i8;

        unsafe {
            let data = (self.verify_file)(ppath);
            Ok(ExternalBuf {
                data,
                free: self.free_returned_buffer,
            })
        }
    }

    fn get_loaded_cert_data(&self) -> ExternalBuf {
        unsafe {
            let data = (self.get_loaded_cert_data)();
            ExternalBuf {
                data,
                free: self.free_returned_buffer,
            }
        }
    }

    fn verify_feature(&self, feature: *mut core::ffi::c_char) -> ExternalBuf {
        unsafe {
            let data = (self.verify_feature)(feature);
            ExternalBuf {
                data,
                free: self.free_returned_buffer,
            }
        }
    }
}

#[derive(Debug)]
pub struct LicenseVerifier(Option<LoadedLibrary>);

impl LicenseVerifier {
    pub fn new(path: impl AsRef<Path>) -> Self {
        match LoadedLibrary::new(path) {
            Ok(l) => {
                log::info!("Successfully initialized certificate verification library.");
                Self(Some(l))
            }
            Err(e) => {
                log::warn!(
                    "Failed to load license verification library. Licensed functionality will be unavailable: {e}"
                );
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

        match cert_path.to_str() {
            Some(path) => {
                if path.is_empty() {
                    return Err(anyhow!("No license certificate configured"));
                }
            }
            None => return Err(anyhow!("Configured license certificate path is invalid")),
        }
        library.init_cert_store();
        let pem = tokio::fs::read(cert_path).await?;
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
    /// verified, regardless of verfication success. This is useful for consumers like ctl that
    /// might be interested in certificate for invalid certificates. The `GetCertDataResult` will
    /// also contain the verification status. `Error`s are simply propagated.
    pub fn get_cert_data(&self) -> Result<GetCertDataResult> {
        let Some(ref library) = self.0 else {
            bail!("License verification library not loaded.");
        };

        let cert = GetCertDataResult::decode(library.get_loaded_cert_data().as_ref())?;
        Ok(cert)
    }

    /// Verifies a specific licensed feature
    ///
    /// Returns `Ok(())` in case of verification success and an `Error` that contains the reason for
    /// verification failure otherwise.
    pub fn verify_feature(&self, feature: LicensedFeature) -> Result<()> {
        let Some(ref library) = self.0 else {
            bail!("License verification library not loaded. Feature {feature:?} unavailable.");
        };

        let pfeature = feature.as_ref().as_ptr() as *mut i8;
        let res = VerifyFeatureResult::decode(library.verify_feature(pfeature).as_ref())?;
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
