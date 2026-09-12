//! Decryption of SAML EncryptedID elements (XML-Enc, via the `crypto` adapter).
//!
//! eID §9.3: AES-256-CBC for data encryption, RSA-OAEP with a SHA digest for key
//! wrapping (the SHA-1 MGF1 variant SHOULD NOT be used).
//! eID §7.6.3.4.4: Identifiers (NameID) are in EncryptedID elements, XML-encrypted
//! so only the intended recipient(s) can decrypt.
use crate::{
    keys::DecryptionKey,
    saml::{
        constants::{NS_SAML, NS_XENC},
        crypto,
        xml_parser::{
            Document, NodeId, descendants_by_tag, direct_text, find_child, find_descendant,
        },
    },
};
use secrecy::{ExposeSecret, SecretString};
use tracing::{debug, warn};

// eID §9.3: block (data) encryption MUST be AES-256-CBC.
const ALLOWED_DATA_ENCRYPTION: &[&str] = &["http://www.w3.org/2001/04/xmlenc#aes256-cbc"];

// eID §9.3: key transport MUST be RSA-OAEP (with a SHA digest). Both the
// `xmlenc#rsa-oaep-mgf1p` and the `xmlenc11#rsa-oaep` URIs are accepted; RSA
// PKCS#1 v1.5 (`rsa-1_5`) is rejected so the decryptor cannot be downgraded to a
// Bleichenbacher-prone padding scheme.
const ALLOWED_KEY_TRANSPORT: &[&str] = &[
    "http://www.w3.org/2001/04/xmlenc#rsa-oaep-mgf1p",
    "http://www.w3.org/2009/xmlenc11#rsa-oaep",
];

/// A decrypted SAML NameID and its eID §7.6.3.4.4 attributes.
#[derive(Debug, Clone)]
pub struct DecryptedNameId {
    /// The decrypted identifier (BSN / pseudonym, PII). Wrapped so it never
    /// reaches `Debug`/logs and is zeroized on drop.
    pub value: SecretString,
    /// `@Format`: eID §7.6.3.4.4 requires `nameid-format:persistent`.
    pub format: String,
    /// `@NameQualifier`: eID §7.6.3.4.4 requires this to carry the identifier type.
    pub name_qualifier: String,
    /// `@SPNameQualifier`: eID §7.6.3.4.4 requires this to be absent.
    pub sp_name_qualifier: Option<String>,
    /// `@SPProvidedID`: eID §7.6.3.4.4 requires this to be absent.
    pub sp_provided_id: Option<String>,
}

/// Enforce the eID §9.3 encryption-algorithm allow-list on an EncryptedID before
/// it is handed to the crypto backend: the `EncryptedData` block cipher MUST be
/// AES-256-CBC and the `EncryptedKey` transport MUST be RSA-OAEP. This mirrors
/// the §9.1 signature-algorithm allow-list and fails closed on a downgrade (e.g.
/// RSA-1.5 key transport or a 128-bit / 3DES data cipher) rather than relying on
/// whatever the backend would otherwise accept.
///
/// `dv_entity_id` is our own EntityID. eID §7.6.3.4 requires every `EncryptedKey`
/// to name its intended recipient in `@Recipient`, and says keys for other
/// recipients SHOULD be ignored, so only keys addressed to us are algorithm-
/// checked: a key wrapped for a *different* DV with a weak transport is skipped
/// rather than failing the whole message. At least one key must be ours (§7.6.3.4
/// pairs `@Recipient` with the `<Audience>` values, and we already require our
/// EntityID among the audiences).
fn check_encryption_algorithms(
    doc: &Document,
    enc_id: NodeId,
    dv_entity_id: Option<&str>,
) -> Result<(), String> {
    check_data_encryption(doc, enc_id)?;
    check_key_transport(doc, &our_encrypted_keys(doc, enc_id, dv_entity_id)?)
}

// eID §9.3: the `EncryptedData` block cipher MUST be AES-256-CBC.
fn check_data_encryption(doc: &Document, enc_id: NodeId) -> Result<(), String> {
    let enc_data = find_descendant(doc, enc_id, NS_XENC, "EncryptedData")
        .ok_or_else(|| "EncryptedID has no EncryptedData".to_string())?;
    let data_alg = find_child(doc, enc_data, NS_XENC, "EncryptionMethod")
        .and_then(|n| doc.get_attribute(n, "Algorithm"))
        .ok_or_else(|| "EncryptedData has no EncryptionMethod Algorithm".to_string())?;
    if !ALLOWED_DATA_ENCRYPTION.contains(&data_alg) {
        return Err(format!(
            "disallowed data-encryption algorithm (eID §9.3 requires AES-256-CBC): {data_alg}"
        ));
    }
    Ok(())
}

// eID §7.6.3.4: `@Recipient` identifies the intended recipient of each wrapped
// key. When we know our own EntityID, consider only our keys; more than one may
// carry it (§7.6.3.4: one per encryption cert during rollover). At least one
// key must be ours.
fn our_encrypted_keys(
    doc: &Document,
    enc_id: NodeId,
    dv_entity_id: Option<&str>,
) -> Result<Vec<NodeId>, String> {
    let enc_keys = descendants_by_tag(doc, enc_id, NS_XENC, "EncryptedKey");
    if enc_keys.is_empty() {
        return Err("EncryptedID has no EncryptedKey".to_string());
    }

    let ours: Vec<NodeId> = match dv_entity_id {
        Some(dv) => enc_keys
            .iter()
            .copied()
            .filter(|&ek| doc.get_attribute(ek, "Recipient") == Some(dv))
            .collect(),
        None => enc_keys.clone(),
    };
    if ours.is_empty() {
        let recipients: Vec<&str> = enc_keys
            .iter()
            .map(|&ek| doc.get_attribute(ek, "Recipient").unwrap_or("<absent>"))
            .collect();
        return Err(format!(
            "no EncryptedKey addressed to this DV (eID §7.6.3.4 @Recipient); saw {recipients:?}"
        ));
    }
    Ok(ours)
}

// eID §9.3: every `EncryptedKey` addressed to us MUST use RSA-OAEP transport.
fn check_key_transport(doc: &Document, enc_keys: &[NodeId]) -> Result<(), String> {
    for &ek in enc_keys {
        let key_alg = find_child(doc, ek, NS_XENC, "EncryptionMethod")
            .and_then(|n| doc.get_attribute(n, "Algorithm"))
            .ok_or_else(|| "EncryptedKey has no EncryptionMethod Algorithm".to_string())?;
        if !ALLOWED_KEY_TRANSPORT.contains(&key_alg) {
            return Err(format!(
                "disallowed key-transport algorithm (eID §9.3 requires RSA-OAEP): {key_alg}"
            ));
        }
    }
    Ok(())
}

/// Decrypt an EncryptedID element per eID §7.6.3.4.4.
///
/// `private_keys`: the DV decryption keys to try. `dv_entity_id` is our own
/// EntityID, used to select the `EncryptedKey` addressed to us
/// (eID §7.6.3.4 `@Recipient`); `None` skips that binding (tests).
pub fn decrypt_encrypted_id(
    doc: &Document,
    enc_id: NodeId,
    private_keys: &[DecryptionKey<'_>],
    dv_entity_id: Option<&str>,
) -> Option<DecryptedNameId> {
    // eID §9.3: reject weak/disallowed encryption algorithms before decrypting,
    // regardless of what the crypto backend would otherwise accept.
    if let Err(reason) = check_encryption_algorithms(doc, enc_id, dv_entity_id) {
        warn!("[decrypt] Rejecting EncryptedID: {reason}");
        return None;
    }

    // Normally self-contained; when the namespaces are declared on an ancestor,
    // restore the inherited ones. The backend only locates the ciphertext, so this
    // cannot affect what is decrypted.
    let enc_id_xml = doc.self_contained_source(enc_id)?;
    let decrypted_xml = decrypt_ciphertext(&enc_id_xml, private_keys)?;

    // eID §7.6.3.4.4: "An <EncryptedID> MUST contain a SAML <NameID> after
    // decryption". Require exactly that, matched by namespace, so plaintext that
    // decrypts to some other element never becomes an identity.
    let dec_doc = crate::saml::xml_parser::parse(&decrypted_xml).ok()?;
    let Some(name_id_node) = decrypted_name_id_node(&dec_doc) else {
        warn!("[decrypt] Decrypted EncryptedID does not contain a saml:NameID; rejecting");
        return None;
    };

    let result = name_id_fields(&dec_doc, name_id_node);
    // SECURITY: only log non-PII metadata; `value` is the decrypted PII
    // (BSN / pseudonym) and MUST stay out of logs.
    debug!(
        "[decrypt] NameID extracted (format='{}', name_qualifier='{}', value_len={})",
        result.format,
        result.name_qualifier,
        result.value.expose_secret().len()
    );
    Some(result)
}

/// Hand the self-contained EncryptedID XML to the crypto backend, trying each
/// configured DV encryption key.
///
/// RSA key transport is crypto-bound to the keypair, not selected by `<KeyName>`
/// (see `crypto::decrypt`): each key is tried in turn, so a blob wrapped to any
/// configured key (e.g. a rotated cert) decrypts. The backend replaces
/// `<EncryptedData>` with the decrypted plaintext in place, returning the
/// EncryptedID element with the NameID inside.
fn decrypt_ciphertext(enc_id_xml: &str, private_keys: &[DecryptionKey<'_>]) -> Option<String> {
    debug!(
        "[decrypt] Decrypting EncryptedID (xml_len={}, candidate_keys={})",
        enc_id_xml.len(),
        private_keys.len()
    );
    match crypto::decrypt(enc_id_xml, private_keys) {
        Ok(xml) => {
            // SECURITY: do not log the decrypted XML content; it contains the
            // plaintext NameID (PII per eID §7.6.3.4). Log only its length.
            debug!(
                "[decrypt] EncryptedID decryption OK (decrypted_xml_len={})",
                xml.len()
            );
            Some(xml)
        }
        Err(e) => {
            warn!("[decrypt] EncryptedID decryption failed: {e}");
            // The encrypted ciphertext is safe to log: by definition it does
            // not reveal plaintext PII without the private key.
            debug!("[decrypt] EncryptedID XML: {enc_id_xml}");
            None
        }
    }
}

/// The `saml:NameID` element of the decrypted plaintext, matched by namespace;
/// the document element itself might be the NameID.
fn decrypted_name_id_node(dec_doc: &Document) -> Option<NodeId> {
    let dec_root = dec_doc.document_element();
    find_descendant(dec_doc, dec_root, NS_SAML, "NameID")
        .or_else(|| (dec_doc.local_name(dec_root)? == "NameID").then_some(dec_root))
}

/// Lift the NameID text and its eID §7.6.3.4.4 attributes into owned fields.
fn name_id_fields(dec_doc: &Document, name_id_node: NodeId) -> DecryptedNameId {
    DecryptedNameId {
        // `direct_text`: the identifier is the NameID's own text. Element children
        // yield an empty value, which `check_decrypted_name_id` rejects.
        value: SecretString::from(direct_text(dec_doc, name_id_node).unwrap_or_default()),
        format: dec_doc
            .get_attribute(name_id_node, "Format")
            .unwrap_or("")
            .to_string(),
        name_qualifier: dec_doc
            .get_attribute(name_id_node, "NameQualifier")
            .unwrap_or("")
            .to_string(),
        sp_name_qualifier: dec_doc
            .get_attribute(name_id_node, "SPNameQualifier")
            .map(str::to_string),
        sp_provided_id: dec_doc
            .get_attribute(name_id_node, "SPProvidedID")
            .map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        keys::{CertificatePem, PrivateKeyPem},
        saml::{
            constants::{NAMEID_PERSISTENT, NS_SAML},
            xml_parser::parse,
        },
    };
    use bergshamra_enc::{EncContext, encrypt::encrypt};
    use bergshamra_keys::{KeysManager, loader};
    use std::path::PathBuf;

    /// Our own EntityID, the `@Recipient` the RD addresses our wrapped keys to.
    const DV_ENTITY_ID: &str = "urn:nl-eid-gdi:1.0:DV:test:entities:9001";

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    /// Build a self-contained `<saml:EncryptedID>` that XML-encrypts `name_id_xml`
    /// for the recipient `cert_pem` (AES-256-CBC data, RSA-OAEP key wrap per eID
    /// §9.3), matching the recipient by `<ds:KeyName>`.
    fn encrypt_name_id(cert_pem: &str, key_name: &str, name_id_xml: &str) -> String {
        // Template with empty CipherValues; bergshamra fills them in. The
        // EncryptedKey carries @Recipient=DV_ENTITY_ID, as the RD emits (eID
        // §7.6.3.4).
        let template = format!(
            r#"<saml:EncryptedID xmlns:saml="{NS_SAML}" xmlns:xenc="http://www.w3.org/2001/04/xmlenc#" xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><xenc:EncryptedData Type="http://www.w3.org/2001/04/xmlenc#Element"><xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes256-cbc"/><ds:KeyInfo><xenc:EncryptedKey Recipient="{DV_ENTITY_ID}"><xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#rsa-oaep-mgf1p"/><ds:KeyInfo><ds:KeyName>{key_name}</ds:KeyName></ds:KeyInfo><xenc:CipherData><xenc:CipherValue></xenc:CipherValue></xenc:CipherData></xenc:EncryptedKey></ds:KeyInfo><xenc:CipherData><xenc:CipherValue></xenc:CipherValue></xenc:CipherData></xenc:EncryptedData></saml:EncryptedID>"#
        );

        let recipient = loader::load_x509_cert_pem(cert_pem.as_bytes())
            .expect("load recipient cert")
            .with_name(key_name);
        let mut mgr = KeysManager::new();
        mgr.add_key(recipient);
        let ctx = EncContext::new(mgr);

        encrypt(&ctx, &template, name_id_xml.as_bytes()).expect("encryption")
    }

    /// Full XML-Enc round-trip: encrypt a NameID to the DV's public key, then run
    /// the production decryption path and assert the recovered identifier. This
    /// exercises the decrypt + re-parse path (including the parser's namespace
    /// strictness on the decrypted plaintext), which the structural unit tests do
    /// not cover.
    #[test]
    fn decrypts_real_encrypted_id_round_trip() {
        let cert_pem = fixture("dv-encryption-1.pem");
        let key_pem = fixture("dv-encryption-1-key.pem");
        let key_name = CertificatePem::parse(&cert_pem).unwrap().key_name();

        // A self-contained, eID-conformant NameID (persistent, with NameQualifier).
        let name_id_xml = format!(
            r#"<saml:NameID xmlns:saml="{NS_SAML}" Format="{NAMEID_PERSISTENT}" NameQualifier="urn:nl-eid-gdi:1.0:id:legacy-BSN">900070341</saml:NameID>"#
        );

        let encrypted_id_xml = encrypt_name_id(&cert_pem, key_name.as_str(), &name_id_xml);
        // Sanity: encryption actually filled the ciphertext (no plaintext leak).
        assert!(!encrypted_id_xml.contains("900070341"));

        let doc = parse(&encrypted_id_xml).expect("parse EncryptedID");
        let enc_id = doc.document_element();

        let key_pem = PrivateKeyPem::new(key_pem);
        let private_keys = [DecryptionKey {
            key_pem: &key_pem,
            key_name: &key_name,
        }];
        let decrypted = decrypt_encrypted_id(&doc, enc_id, &private_keys, Some(DV_ENTITY_ID))
            .expect("decryption must recover the NameID");

        assert_eq!(decrypted.value.expose_secret(), "900070341");
        assert_eq!(decrypted.format, NAMEID_PERSISTENT);
        assert_eq!(decrypted.name_qualifier, "urn:nl-eid-gdi:1.0:id:legacy-BSN");
        assert!(decrypted.sp_name_qualifier.is_none());
        assert!(decrypted.sp_provided_id.is_none());
    }

    /// A non-matching private key (wrong recipient) must not recover the NameID.
    #[test]
    fn wrong_key_does_not_decrypt() {
        let cert_pem = fixture("dv-encryption-1.pem");
        let key_name = CertificatePem::parse(&cert_pem).unwrap().key_name();
        let name_id_xml = format!(
            r#"<saml:NameID xmlns:saml="{NS_SAML}" Format="{NAMEID_PERSISTENT}" NameQualifier="urn:nl-eid-gdi:1.0:id:legacy-BSN">900070341</saml:NameID>"#
        );
        let encrypted_id_xml = encrypt_name_id(&cert_pem, key_name.as_str(), &name_id_xml);

        let doc = parse(&encrypted_id_xml).expect("parse EncryptedID");
        let enc_id = doc.document_element();

        // Present a different key under the same KeyName: unwrap must fail.
        let other_key = PrivateKeyPem::new(fixture("dv-encryption-2-key.pem"));
        let private_keys = [DecryptionKey {
            key_pem: &other_key,
            key_name: &key_name,
        }];
        assert!(decrypt_encrypted_id(&doc, enc_id, &private_keys, Some(DV_ENTITY_ID)).is_none());
    }

    /// eID §9.3: a fragment wrapped to the *second* configured key (rollover) must
    /// still decrypt even though it is not first in the list (finding H).
    #[test]
    fn decrypts_with_non_first_key_during_rollover() {
        let cert2 = fixture("dv-encryption-2.pem");
        let kn1 = CertificatePem::parse(fixture("dv-encryption-1.pem"))
            .unwrap()
            .key_name();
        let kn2 = CertificatePem::parse(&cert2).unwrap().key_name();
        let key1 = fixture("dv-encryption-1-key.pem");
        let key2 = fixture("dv-encryption-2-key.pem");

        let name_id_xml = format!(
            r#"<saml:NameID xmlns:saml="{NS_SAML}" Format="{NAMEID_PERSISTENT}" NameQualifier="urn:nl-eid-gdi:1.0:id:legacy-BSN">900070341</saml:NameID>"#
        );
        let encrypted_id_xml = encrypt_name_id(&cert2, kn2.as_str(), &name_id_xml);
        let doc = parse(&encrypted_id_xml).expect("parse EncryptedID");
        let enc_id = doc.document_element();

        // key1 (wrong) is listed before key2 (correct); decryption must still work.
        let (key1, key2) = (PrivateKeyPem::new(key1), PrivateKeyPem::new(key2));
        let private_keys = [
            DecryptionKey {
                key_pem: &key1,
                key_name: &kn1,
            },
            DecryptionKey {
                key_pem: &key2,
                key_name: &kn2,
            },
        ];
        let decrypted = decrypt_encrypted_id(&doc, enc_id, &private_keys, Some(DV_ENTITY_ID))
            .expect("rollover: a blob wrapped to the second key must decrypt");
        assert_eq!(decrypted.value.expose_secret(), "900070341");
    }

    // -- eID §9.3 incoming algorithm allow-list (finding F) --

    fn encrypted_id_with_algs(data_alg: &str, key_alg: &str) -> String {
        encrypted_id_for(data_alg, key_alg, DV_ENTITY_ID)
    }

    fn encrypted_id_for(data_alg: &str, key_alg: &str, recipient: &str) -> String {
        format!(
            r#"<saml:EncryptedID xmlns:saml="{NS_SAML}" xmlns:xenc="http://www.w3.org/2001/04/xmlenc#" xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><xenc:EncryptedData><xenc:EncryptionMethod Algorithm="{data_alg}"/><ds:KeyInfo><xenc:EncryptedKey Recipient="{recipient}"><xenc:EncryptionMethod Algorithm="{key_alg}"/></xenc:EncryptedKey></ds:KeyInfo></xenc:EncryptedData></saml:EncryptedID>"#
        )
    }

    #[test]
    fn check_encryption_algorithms_accepts_aes256_and_rsa_oaep() {
        let xml = encrypted_id_with_algs(
            "http://www.w3.org/2001/04/xmlenc#aes256-cbc",
            "http://www.w3.org/2001/04/xmlenc#rsa-oaep-mgf1p",
        );
        let doc = parse(&xml).unwrap();
        assert!(
            check_encryption_algorithms(&doc, doc.document_element(), Some(DV_ENTITY_ID)).is_ok()
        );
    }

    #[test]
    fn check_encryption_algorithms_rejects_rsa_pkcs1_and_weak_cipher() {
        // RSA-1.5 key transport is rejected (Bleichenbacher-prone downgrade).
        let rsa15 = encrypted_id_with_algs(
            "http://www.w3.org/2001/04/xmlenc#aes256-cbc",
            "http://www.w3.org/2001/04/xmlenc#rsa-1_5",
        );
        let doc = parse(&rsa15).unwrap();
        assert!(
            check_encryption_algorithms(&doc, doc.document_element(), Some(DV_ENTITY_ID)).is_err()
        );

        // A weaker data cipher (AES-128-CBC) is rejected.
        let aes128 = encrypted_id_with_algs(
            "http://www.w3.org/2001/04/xmlenc#aes128-cbc",
            "http://www.w3.org/2001/04/xmlenc#rsa-oaep-mgf1p",
        );
        let doc = parse(&aes128).unwrap();
        assert!(
            check_encryption_algorithms(&doc, doc.document_element(), Some(DV_ENTITY_ID)).is_err()
        );
    }

    #[test]
    fn check_encryption_algorithms_requires_a_key_addressed_to_us() {
        // eID §7.6.3.4: @Recipient identifies the intended recipient. An
        // EncryptedID carrying only another DV's key is not ours to decrypt, so
        // it must not be handed to the crypto backend at all.
        let xml = encrypted_id_for(
            "http://www.w3.org/2001/04/xmlenc#aes256-cbc",
            "http://www.w3.org/2001/04/xmlenc#rsa-oaep-mgf1p",
            "urn:nl-eid-gdi:1.0:DV:someone-else:entities:0001",
        );
        let doc = parse(&xml).unwrap();
        let err = check_encryption_algorithms(&doc, doc.document_element(), Some(DV_ENTITY_ID))
            .expect_err("a key for another recipient must be rejected");
        assert!(
            err.contains("no EncryptedKey addressed to this DV"),
            "{err}"
        );
    }

    #[test]
    fn check_encryption_algorithms_ignores_other_recipients_weak_keys() {
        // §7.6.3.4 says keys for other recipients SHOULD be ignored: a foreign
        // key with a weak (rsa-1_5) transport must not fail a message whose own
        // key is sound, but the reverse must still be rejected.
        let ours_ok = format!(
            r#"<saml:EncryptedID xmlns:saml="{NS_SAML}" xmlns:xenc="http://www.w3.org/2001/04/xmlenc#" xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><xenc:EncryptedData><xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes256-cbc"/><ds:KeyInfo><xenc:EncryptedKey Recipient="urn:other"><xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#rsa-1_5"/></xenc:EncryptedKey><xenc:EncryptedKey Recipient="{DV_ENTITY_ID}"><xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#rsa-oaep-mgf1p"/></xenc:EncryptedKey></ds:KeyInfo></xenc:EncryptedData></saml:EncryptedID>"#
        );
        let doc = parse(&ours_ok).unwrap();
        assert!(
            check_encryption_algorithms(&doc, doc.document_element(), Some(DV_ENTITY_ID)).is_ok(),
            "a foreign weak key must not fail a message whose own key is sound"
        );

        let ours_weak = format!(
            r#"<saml:EncryptedID xmlns:saml="{NS_SAML}" xmlns:xenc="http://www.w3.org/2001/04/xmlenc#" xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><xenc:EncryptedData><xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes256-cbc"/><ds:KeyInfo><xenc:EncryptedKey Recipient="urn:other"><xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#rsa-oaep-mgf1p"/></xenc:EncryptedKey><xenc:EncryptedKey Recipient="{DV_ENTITY_ID}"><xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#rsa-1_5"/></xenc:EncryptedKey></ds:KeyInfo></xenc:EncryptedData></saml:EncryptedID>"#
        );
        let doc = parse(&ours_weak).unwrap();
        assert!(
            check_encryption_algorithms(&doc, doc.document_element(), Some(DV_ENTITY_ID)).is_err(),
            "our own key using rsa-1_5 must be rejected"
        );
    }

    #[test]
    fn decrypted_plaintext_that_is_not_a_name_id_is_rejected() {
        // eID §7.6.3.4.4: an EncryptedID MUST decrypt to a saml:NameID. Anything
        // else (here a same-local-name element in the wrong namespace) must not
        // become an identity.
        let cert = fixture("dv-encryption-1.pem");
        let key_name = CertificatePem::parse(&cert).unwrap().key_name();
        let key = fixture("dv-encryption-1-key.pem");
        let not_a_name_id =
            r#"<NameID xmlns="urn:attacker:ns" Format="x">900070341</NameID>"#.to_string();
        let encrypted = encrypt_name_id(&cert, key_name.as_str(), &not_a_name_id);
        let doc = parse(&encrypted).expect("parse EncryptedID");
        let key = PrivateKeyPem::new(key);
        let private_keys = [DecryptionKey {
            key_pem: &key,
            key_name: &key_name,
        }];
        assert!(
            decrypt_encrypted_id(
                &doc,
                doc.document_element(),
                &private_keys,
                Some(DV_ENTITY_ID)
            )
            .is_none(),
            "plaintext without a saml:NameID must be rejected"
        );
    }
}
