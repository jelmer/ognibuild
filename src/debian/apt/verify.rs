//! Verification of APT Release signatures.
//!
//! APT repositories ship a `Release` file whose authenticity is asserted by a
//! detached or inline PGP signature (`InRelease` is clearsigned, `Release` is
//! accompanied by a separate `Release.gpg`). This module verifies either form
//! against the keys a repository is trusted with via its `Signed-By` field (or
//! APT's default trusted keyrings), returning the verified payload.

use apt_sources::signature::Signature;
use sequoia_openpgp::anyhow;
use sequoia_openpgp::cert::CertParser;
use sequoia_openpgp::parse::stream::{
    MessageLayer, MessageStructure, VerificationHelper, VerifierBuilder,
};
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::{Cert, KeyHandle};
use std::io::Read;
use std::path::Path;

/// Error verifying an APT Release signature.
#[derive(Debug)]
pub enum Error {
    /// No trusted key was available to verify the repository against.
    NoTrustedKeys,
    /// A keyring file referenced by `Signed-By` could not be read.
    KeyringRead(std::io::Error),
    /// A trusted key could not be parsed.
    KeyParse(anyhow::Error),
    /// The signature did not verify against any trusted key.
    Verification(anyhow::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::NoTrustedKeys => write!(f, "no trusted keys available to verify Release"),
            Error::KeyringRead(e) => write!(f, "unable to read keyring: {}", e),
            Error::KeyParse(e) => write!(f, "unable to parse trusted key: {}", e),
            Error::Verification(e) => write!(f, "Release signature did not verify: {}", e),
        }
    }
}

impl std::error::Error for Error {}

/// Default locations APT trusts when a source has no explicit `Signed-By`.
///
/// Paths are resolved through the caller-provided `read_file` closure so they
/// can be redirected into a session's filesystem.
const DEFAULT_TRUSTED_KEYRING: &str = "/etc/apt/trusted.gpg";
const DEFAULT_TRUSTED_KEYRING_DIR: &str = "/etc/apt/trusted.gpg.d";

fn parse_certs(bytes: &[u8]) -> Result<Vec<Cert>, Error> {
    CertParser::from_bytes(bytes)
        .map_err(Error::KeyParse)?
        .collect::<sequoia_openpgp::Result<Vec<Cert>>>()
        .map_err(Error::KeyParse)
}

/// Resolve the certificates a repository is trusted against.
///
/// `read_file` reads a path (which may be redirected into a session), and
/// `list_dir` lists the entries of a directory. When the repository has an
/// explicit `Signed-By`, only those keys are trusted; otherwise APT's default
/// trusted keyrings are used.
pub fn trusted_certs(
    signature: Option<&Signature>,
    read_file: impl Fn(&Path) -> std::io::Result<Vec<u8>>,
    list_dir: impl Fn(&Path) -> std::io::Result<Vec<std::path::PathBuf>>,
) -> Result<Vec<Cert>, Error> {
    let mut certs = Vec::new();
    match signature {
        Some(Signature::KeyBlock(block)) => {
            certs.extend(parse_certs(block.as_bytes())?);
        }
        Some(Signature::KeyPath(path)) => {
            let bytes = read_file(path).map_err(Error::KeyringRead)?;
            certs.extend(parse_certs(&bytes)?);
        }
        None => {
            // APT trusts /etc/apt/trusted.gpg and every keyring under
            // /etc/apt/trusted.gpg.d when a source omits Signed-By.
            if let Ok(bytes) = read_file(Path::new(DEFAULT_TRUSTED_KEYRING)) {
                certs.extend(parse_certs(&bytes)?);
            }
            if let Ok(entries) = list_dir(Path::new(DEFAULT_TRUSTED_KEYRING_DIR)) {
                for entry in entries {
                    match entry.extension().and_then(|e| e.to_str()) {
                        Some("gpg") | Some("asc") => {}
                        _ => continue,
                    }
                    let bytes = read_file(&entry).map_err(Error::KeyringRead)?;
                    certs.extend(parse_certs(&bytes)?);
                }
            }
        }
    }
    if certs.is_empty() {
        return Err(Error::NoTrustedKeys);
    }
    Ok(certs)
}

/// Resolve trusted certificates from the host filesystem.
///
/// Reads keyrings directly via [`std::fs`], suitable when APT's configuration
/// lives on the local machine (as opposed to inside a session).
pub fn trusted_certs_host(signature: Option<&Signature>) -> Result<Vec<Cert>, Error> {
    trusted_certs(
        signature,
        |path| std::fs::read(path),
        |dir| {
            std::fs::read_dir(dir)?
                .map(|entry| entry.map(|e| e.path()))
                .collect()
        },
    )
}

struct Helper {
    certs: Vec<Cert>,
}

impl VerificationHelper for Helper {
    fn get_certs(&mut self, _ids: &[KeyHandle]) -> sequoia_openpgp::Result<Vec<Cert>> {
        Ok(self.certs.clone())
    }

    fn check(&mut self, structure: MessageStructure) -> sequoia_openpgp::Result<()> {
        for layer in structure {
            if let MessageLayer::SignatureGroup { results } = layer {
                if results.iter().any(|r| r.is_ok()) {
                    return Ok(());
                }
                // VerificationError borrows from the message structure, so
                // render it to an owned message rather than propagating it.
                if let Some(Err(e)) = results.into_iter().next() {
                    return Err(anyhow::anyhow!("{}", e));
                }
            }
        }
        Err(anyhow::anyhow!("no valid signature found"))
    }
}

/// Verify a clearsigned `InRelease` message against trusted certificates.
///
/// Returns the verified payload (the Release file body). The returned bytes are
/// the signed payload as recovered by the verifier, which is what must be
/// parsed -- never the raw input.
pub fn verify_clearsigned(signed: &[u8], certs: Vec<Cert>) -> Result<Vec<u8>, Error> {
    let policy = StandardPolicy::new();
    let helper = Helper { certs };
    let mut verifier = VerifierBuilder::from_bytes(signed)
        .and_then(|b| b.with_policy(&policy, None, helper))
        .map_err(Error::Verification)?;
    let mut payload = Vec::new();
    verifier
        .read_to_end(&mut payload)
        .map_err(|e| Error::Verification(anyhow::Error::from(e)))?;
    Ok(payload)
}

/// Verify a `Release` file against its detached `Release.gpg` signature.
///
/// Unlike the clearsigned `InRelease`, a plain `Release` is the payload itself
/// and is accompanied by a separate (possibly armored) signature. The `release`
/// bytes are the payload to parse; this returns `Ok(())` once the signature is
/// confirmed.
pub fn verify_detached(release: &[u8], signature: &[u8], certs: Vec<Cert>) -> Result<(), Error> {
    use sequoia_openpgp::parse::stream::DetachedVerifierBuilder;
    let policy = StandardPolicy::new();
    let helper = Helper { certs };
    let mut verifier = DetachedVerifierBuilder::from_bytes(signature)
        .and_then(|b| b.with_policy(&policy, None, helper))
        .map_err(Error::Verification)?;
    verifier
        .verify_bytes(release)
        .map_err(Error::Verification)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sequoia_openpgp::cert::CertBuilder;
    use sequoia_openpgp::serialize::stream::{Message, Signer};
    use sequoia_openpgp::serialize::Serialize;
    use std::io::Write;

    fn clearsign(cert: &Cert, payload: &[u8]) -> Vec<u8> {
        let policy = StandardPolicy::new();
        let keypair = cert
            .keys()
            .with_policy(&policy, None)
            .secret()
            .for_signing()
            .next()
            .unwrap()
            .key()
            .clone()
            .into_keypair()
            .unwrap();
        let mut sink = Vec::new();
        {
            // The cleartext signer produces its own armor framing.
            let message = Message::new(&mut sink);
            let mut signer = Signer::new(message, keypair)
                .unwrap()
                .cleartext()
                .build()
                .unwrap();
            signer.write_all(payload).unwrap();
            signer.finalize().unwrap();
        }
        sink
    }

    fn armored_cert(cert: &Cert) -> Vec<u8> {
        let mut buf = Vec::new();
        cert.armored().serialize(&mut buf).unwrap();
        buf
    }

    fn detached_signature(cert: &Cert, payload: &[u8]) -> Vec<u8> {
        use sequoia_openpgp::serialize::stream::Armorer;
        let policy = StandardPolicy::new();
        let keypair = cert
            .keys()
            .with_policy(&policy, None)
            .secret()
            .for_signing()
            .next()
            .unwrap()
            .key()
            .clone()
            .into_keypair()
            .unwrap();
        let mut sink = Vec::new();
        {
            let message = Message::new(&mut sink);
            let message = Armorer::new(message)
                .kind(sequoia_openpgp::armor::Kind::Signature)
                .build()
                .unwrap();
            let mut signer = Signer::new(message, keypair)
                .unwrap()
                .detached()
                .build()
                .unwrap();
            signer.write_all(payload).unwrap();
            signer.finalize().unwrap();
        }
        sink
    }

    #[test]
    fn test_verify_detached_roundtrip() {
        let (cert, _) = CertBuilder::new().add_signing_subkey().generate().unwrap();
        let release = b"Origin: Debian\nSuite: trixie\n";
        let signature = detached_signature(&cert, release);
        verify_detached(release, &signature, vec![cert]).unwrap();
    }

    #[test]
    fn test_verify_detached_wrong_key_fails() {
        let (signing_cert, _) = CertBuilder::new().add_signing_subkey().generate().unwrap();
        let (other_cert, _) = CertBuilder::new().add_signing_subkey().generate().unwrap();
        let release = b"Origin: Debian\n";
        let signature = detached_signature(&signing_cert, release);

        let err = verify_detached(release, &signature, vec![other_cert]).unwrap_err();
        assert!(matches!(err, Error::Verification(_)), "got {:?}", err);
    }

    #[test]
    fn test_verify_detached_tampered_payload_fails() {
        let (cert, _) = CertBuilder::new().add_signing_subkey().generate().unwrap();
        let signature = detached_signature(&cert, b"Origin: Debian\n");

        let err = verify_detached(b"Origin: Evil\n", &signature, vec![cert]).unwrap_err();
        assert!(matches!(err, Error::Verification(_)), "got {:?}", err);
    }

    #[test]
    fn test_verify_clearsigned_roundtrip() {
        let (cert, _) = CertBuilder::new().add_signing_subkey().generate().unwrap();
        let payload = b"Origin: Debian\nSuite: trixie\n";
        let signed = clearsign(&cert, payload);

        let verified = verify_clearsigned(&signed, vec![cert]).unwrap();
        assert_eq!(verified, payload);
    }

    #[test]
    fn test_verify_clearsigned_wrong_key_fails() {
        let (signing_cert, _) = CertBuilder::new().add_signing_subkey().generate().unwrap();
        let (other_cert, _) = CertBuilder::new().add_signing_subkey().generate().unwrap();
        let signed = clearsign(&signing_cert, b"Origin: Debian\n");

        let err = verify_clearsigned(&signed, vec![other_cert]).unwrap_err();
        assert!(matches!(err, Error::Verification(_)), "got {:?}", err);
    }

    #[test]
    fn test_trusted_certs_key_block() {
        let (cert, _) = CertBuilder::new().add_signing_subkey().generate().unwrap();
        let block = String::from_utf8(armored_cert(&cert)).unwrap();
        let signature = Signature::KeyBlock(block);
        let certs = trusted_certs(
            Some(&signature),
            |_| panic!("should not read files for a key block"),
            |_| panic!("should not list dirs for a key block"),
        )
        .unwrap();
        assert_eq!(certs.len(), 1);
        assert_eq!(certs[0].fingerprint(), cert.fingerprint());
    }

    #[test]
    fn test_trusted_certs_key_path() {
        let (cert, _) = CertBuilder::new().add_signing_subkey().generate().unwrap();
        let bytes = armored_cert(&cert);
        let signature = Signature::KeyPath("/usr/share/keyrings/test.gpg".into());
        let certs = trusted_certs(
            Some(&signature),
            |p| {
                assert_eq!(p, Path::new("/usr/share/keyrings/test.gpg"));
                Ok(bytes.clone())
            },
            |_| panic!("should not list dirs for a key path"),
        )
        .unwrap();
        assert_eq!(certs.len(), 1);
        assert_eq!(certs[0].fingerprint(), cert.fingerprint());
    }

    #[test]
    fn test_trusted_certs_none_available() {
        let err = trusted_certs(
            None,
            |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        )
        .unwrap_err();
        assert!(matches!(err, Error::NoTrustedKeys), "got {:?}", err);
    }
}
