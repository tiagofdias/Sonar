//! Native Windows WebAuthn assertion. Origin and relying-party validation is
//! essential because a native app supplies clientDataJSON to Windows itself.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use url::Url;

fn validate_origin(base: &Url, origin: &str, rp_id: &str) -> Result<(), String> {
    let origin_url = Url::parse(origin).map_err(|_| "Invalid passkey origin.")?;
    if !origin_url.username().is_empty() || origin_url.password().is_some() || origin_url.query().is_some() || origin_url.fragment().is_some() || origin_url.path() != "/" || origin_url.origin().ascii_serialization() != origin {
        return Err("Invalid passkey origin.".into());
    }
    let local_development = base.scheme() == "http" && matches!(base.host_str(), Some("127.0.0.1" | "localhost")) && origin == "http://localhost:5173";
    if !local_development && (base.scheme() != "https" || base.origin() != origin_url.origin()) {
        return Err("The passkey origin does not match this Sonar server.".into());
    }
    if origin_url.host_str() != Some(rp_id) {
        return Err("The passkey request does not match the expected origin.".into());
    }
    Ok(())
}
fn client_data(origin: &str, challenge: &str, kind: &str) -> Result<Vec<u8>, String> {
    let raw = URL_SAFE_NO_PAD.decode(challenge).map_err(|_| "Invalid passkey request.")?;
    if raw.len() < 16 || raw.len() > 256 || URL_SAFE_NO_PAD.encode(&raw) != challenge { return Err("Invalid passkey request.".into()); }
    serde_json::to_vec(&json!({ "type": kind, "challenge": challenge, "origin": origin, "crossOrigin": false })).map_err(|_| "Invalid passkey request.".into())
}
pub fn validate_request(base: &Url, origin: &str, options: &Value) -> Result<(String, Vec<u8>), String> {
    let rp_id = options["rpId"].as_str().ok_or("Invalid passkey request.")?;
    validate_origin(base, origin, rp_id)?;
    if options["userVerification"].as_str() != Some("required") { return Err("The passkey request must require user verification.".into()); }
    if options.get("allowCredentials").is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty)) {
        return Err("Credential list not supported.".into());
    }
    let challenge = options["challenge"].as_str().ok_or("Invalid passkey request.")?;
    let client_data = client_data(origin, challenge, "webauthn.get")?;
    Ok((rp_id.to_owned(), client_data))
}

pub struct Registration {
    rp_id: String, rp_name: String, user_id: Vec<u8>, user_name: String, display_name: String,
    client_data: Vec<u8>, algorithms: Vec<i32>, exclude: Vec<Vec<u8>>,
}
pub fn validate_registration(base: &Url, origin: &str, options: &Value, account: &str) -> Result<Registration, String> {
    let rp_id = options["rp"]["id"].as_str().ok_or("Invalid passkey request.")?;
    validate_origin(base, origin, rp_id)?;
    if options["authenticatorSelection"]["residentKey"].as_str() != Some("required") || options["authenticatorSelection"]["userVerification"].as_str() != Some("required") || options["attestation"].as_str() != Some("none") {
        return Err("The passkey request does not meet the security requirements.".into());
    }
    let user_id = URL_SAFE_NO_PAD.decode(options["user"]["id"].as_str().ok_or("Invalid account.")?).map_err(|_| "Invalid account.")?;
    if user_id != account.as_bytes() || account.len() != 24 || !account.bytes().all(|c| c.is_ascii_hexdigit()) { return Err("The passkey request does not match this account.".into()); }
    fn safe_text(value: &Value, max: usize) -> Result<String, String> {
        let text = value.as_str().filter(|s| !s.is_empty() && s.len() <= max && !s.contains('\0')).ok_or("Invalid passkey request.")?;
        Ok(text.to_owned())
    }
    let rp_name = safe_text(&options["rp"]["name"], 128)?;
    let user_name = safe_text(&options["user"]["name"], 254)?;
    let display_name = safe_text(&options["user"]["displayName"], 320)?;
    let challenge = options["challenge"].as_str().ok_or("Invalid passkey request.")?;
    let data = client_data(origin, challenge, "webauthn.create")?;
    let params = options["pubKeyCredParams"].as_array().ok_or("Invalid passkey algorithms.")?;
    let algorithms: Vec<i32> = params.iter().filter(|p| p["type"] == "public-key").filter_map(|p| p["alg"].as_i64()).filter(|alg| *alg == -7 || *alg == -257).map(|alg| alg as i32).collect();
    if algorithms.is_empty() || algorithms.len() > 10 { return Err("Invalid passkey algorithms.".into()); }
    let mut exclude = Vec::new();
    if let Some(list) = options.get("excludeCredentials") {
        let list = list.as_array().ok_or("Invalid passkey list.")?;
        if list.len() > 10 { return Err("Invalid passkey list.".into()); }
        for item in list {
            if item["type"] != "public-key" { return Err("Invalid passkey list.".into()); }
            let text = item["id"].as_str().ok_or("Invalid passkey list.")?;
            let id = URL_SAFE_NO_PAD.decode(text).map_err(|_| "Invalid passkey list.")?;
            if id.is_empty() || id.len() > 1024 || URL_SAFE_NO_PAD.encode(&id) != text { return Err("Invalid passkey list.".into()); }
            exclude.push(id);
        }
    }
    Ok(Registration { rp_id: rp_id.to_owned(), rp_name, user_id, user_name, display_name, client_data: data, algorithms, exclude })
}

#[cfg(windows)]
pub fn active_window() -> isize {
    unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow() as isize }
}

#[cfg(windows)]
pub fn register(window: isize, mut request: Registration) -> Result<Value, String> {
    use std::ptr;
    use windows_sys::Win32::Networking::WindowsWebServices::*;
    if window == 0 { return Err("Bring Sonar to the front before creating a passkey.".into()); }
    let wide = |value: &str| value.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let rp_id = wide(&request.rp_id); let rp_name = wide(&request.rp_name);
    let user_name = wide(&request.user_name); let display_name = wide(&request.display_name);
    let rp = WEBAUTHN_RP_ENTITY_INFORMATION { dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION, pwszId: rp_id.as_ptr(), pwszName: rp_name.as_ptr(), pwszIcon: ptr::null() };
    let user = WEBAUTHN_USER_ENTITY_INFORMATION { dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION, cbId: request.user_id.len() as u32, pbId: request.user_id.as_mut_ptr(), pwszName: user_name.as_ptr(), pwszIcon: ptr::null(), pwszDisplayName: display_name.as_ptr() };
    let mut algorithms: Vec<WEBAUTHN_COSE_CREDENTIAL_PARAMETER> = request.algorithms.iter().map(|alg| WEBAUTHN_COSE_CREDENTIAL_PARAMETER { dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION, pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY, lAlg: *alg }).collect();
    let params = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS { cCredentialParameters: algorithms.len() as u32, pCredentialParameters: algorithms.as_mut_ptr() };
    let data = WEBAUTHN_CLIENT_DATA { dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION, cbClientDataJSON: request.client_data.len() as u32, pbClientDataJSON: request.client_data.as_mut_ptr(), pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256 };
    let mut exclude: Vec<WEBAUTHN_CREDENTIAL> = request.exclude.iter_mut().map(|id| WEBAUTHN_CREDENTIAL { dwVersion: 1, cbId: id.len() as u32, pbId: id.as_mut_ptr(), pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY }).collect();
    let mut options = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS::default();
    options.dwVersion = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_1;
    options.dwTimeoutMilliseconds = 120000;
    options.bRequireResidentKey = 1;
    options.dwUserVerificationRequirement = WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED;
    options.dwAttestationConveyancePreference = WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE;
    options.CredentialList = WEBAUTHN_CREDENTIALS { cCredentials: exclude.len() as u32, pCredentials: if exclude.is_empty() { ptr::null_mut() } else { exclude.as_mut_ptr() } };
    let mut output = ptr::null_mut();
    let result = unsafe { WebAuthNAuthenticatorMakeCredential(window as _, &rp, &user, &params, &data, &options, &mut output) };
    if result < 0 || output.is_null() { return Err("Passkey creation was cancelled or is unavailable.".into()); }
    struct Attestation(*mut WEBAUTHN_CREDENTIAL_ATTESTATION);
    impl Drop for Attestation { fn drop(&mut self) { unsafe { WebAuthNFreeCredentialAttestation(self.0); } } }
    let output = Attestation(output);
    let value = output.0;
    unsafe fn bytes(ptr: *const u8, len: u32) -> Result<Vec<u8>, String> {
        if ptr.is_null() || len == 0 || len > 65536 { return Err("Invalid passkey response.".into()); }
        Ok(std::slice::from_raw_parts(ptr, len as usize).to_vec())
    }
    // Access only v1 fields: older Windows versions may return a shorter struct.
    let id = URL_SAFE_NO_PAD.encode(unsafe { bytes(ptr::addr_of!((*value).pbCredentialId).read(), ptr::addr_of!((*value).cbCredentialId).read())? });
    let attestation = URL_SAFE_NO_PAD.encode(unsafe { bytes(ptr::addr_of!((*value).pbAttestationObject).read(), ptr::addr_of!((*value).cbAttestationObject).read())? });
    Ok(json!({ "id": id, "rawId": id, "type": "public-key", "response": { "attestationObject": attestation, "clientDataJSON": URL_SAFE_NO_PAD.encode(request.client_data) }, "clientExtensionResults": {} }))
}

#[cfg(windows)]
pub fn authenticate(window: isize, rp_id: String, mut client_data: Vec<u8>) -> Result<Value, String> {
    use std::ptr;
    use windows_sys::Win32::Networking::WindowsWebServices::*;
    if window == 0 { return Err("Could not open Windows Hello. Try again with Sonar in front.".into()); }
    let mut rp: Vec<u16> = rp_id.encode_utf16().collect(); rp.push(0);
    let data = WEBAUTHN_CLIENT_DATA { dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION, cbClientDataJSON: client_data.len() as u32, pbClientDataJSON: client_data.as_mut_ptr(), pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256 };
    let mut options = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS::default();
    options.dwVersion = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_1;
    options.dwTimeoutMilliseconds = 120000;
    options.dwUserVerificationRequirement = WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED;
    let mut assertion = ptr::null_mut();
    let result = unsafe { WebAuthNAuthenticatorGetAssertion(window as _, rp.as_ptr(), &data, &options, &mut assertion) };
    if result < 0 || assertion.is_null() { return Err("The passkey prompt was cancelled or is unavailable. Try again.".into()); }
    struct Assertion(*mut WEBAUTHN_ASSERTION);
    impl Drop for Assertion { fn drop(&mut self) { unsafe { WebAuthNFreeAssertion(self.0); } } }
    let assertion = Assertion(assertion);
    let value = assertion.0;
    unsafe fn bytes(ptr: *const u8, len: u32) -> Result<Vec<u8>, String> {
        if ptr.is_null() || len == 0 || len > 65536 { return Err("Invalid passkey response.".into()); }
        Ok(std::slice::from_raw_parts(ptr, len as usize).to_vec())
    }
    // Access only v1 fields: older Windows versions may return a shorter struct.
    let credential = URL_SAFE_NO_PAD.encode(unsafe { bytes(ptr::addr_of!((*value).Credential.pbId).read(), ptr::addr_of!((*value).Credential.cbId).read())? });
    let auth_data = URL_SAFE_NO_PAD.encode(unsafe { bytes(ptr::addr_of!((*value).pbAuthenticatorData).read(), ptr::addr_of!((*value).cbAuthenticatorData).read())? });
    let signature = URL_SAFE_NO_PAD.encode(unsafe { bytes(ptr::addr_of!((*value).pbSignature).read(), ptr::addr_of!((*value).cbSignature).read())? });
    let user = URL_SAFE_NO_PAD.encode(unsafe { bytes(ptr::addr_of!((*value).pbUserId).read(), ptr::addr_of!((*value).cbUserId).read())? });
    Ok(json!({ "id": credential, "rawId": credential, "type": "public-key", "response": { "authenticatorData": auth_data, "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data), "signature": signature, "userHandle": user }, "clientExtensionResults": {} }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_cross_origin_or_invalid_authentication_requests() {
        let base = Url::parse("https://sonar.example/").unwrap();
        let options = json!({"rpId":"sonar.example", "challenge": URL_SAFE_NO_PAD.encode([1u8; 32]), "userVerification":"required", "allowCredentials":[]});
        let (_, data) = validate_request(&base, "https://sonar.example", &options).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&data).unwrap()["type"], "webauthn.get");
        assert!(validate_request(&base, "https://evil.example", &options).is_err());
        assert!(validate_request(&base, "https://sonar.example/path", &options).is_err());
        let mut changed = options.clone(); changed["rpId"] = json!("evil.example");
        assert!(validate_request(&base, "https://sonar.example", &changed).is_err());
        changed = options.clone(); changed["userVerification"] = json!("discouraged");
        assert!(validate_request(&base, "https://sonar.example", &changed).is_err());
        changed = options.clone(); changed["challenge"] = json!("bad");
        assert!(validate_request(&base, "https://sonar.example", &changed).is_err());
        changed = options.clone(); changed["allowCredentials"] = json!([{"id":"x"}]);
        assert!(validate_request(&base, "https://sonar.example", &changed).is_err());
    }
    #[test]
    fn registration_is_bound_to_origin_account_and_secure_options() {
        let base = Url::parse("https://sonar.example/").unwrap();
        let account = "0123456789abcdef01234567";
        let options = json!({ "rp": {"id":"sonar.example", "name":"Sonar"}, "user": { "id": URL_SAFE_NO_PAD.encode(account), "name":"synthetic@example.invalid", "displayName":"Synthetic" }, "challenge": URL_SAFE_NO_PAD.encode([1u8; 32]), "authenticatorSelection": { "residentKey":"required", "userVerification":"required" }, "attestation":"none", "pubKeyCredParams":[{"type":"public-key", "alg":-7}], "excludeCredentials":[{"type":"public-key", "id":URL_SAFE_NO_PAD.encode([2u8; 32])}] });
        let request = validate_registration(&base, "https://sonar.example", &options, account).unwrap();
        assert_eq!(request.exclude.len(), 1);
        assert_eq!(serde_json::from_slice::<Value>(&request.client_data).unwrap()["type"], "webauthn.create");
        assert!(validate_registration(&base, "https://other.example", &options, account).is_err());
        assert!(validate_registration(&base, "https://sonar.example", &options, "abcdef0123456789abcdef01").is_err());
        let mut bad = options.clone(); bad["authenticatorSelection"]["residentKey"] = json!("discouraged");
        assert!(validate_registration(&base, "https://sonar.example", &bad, account).is_err());
        bad = options.clone(); bad["pubKeyCredParams"] = json!([{"type":"public-key", "alg":-8}]);
        assert!(validate_registration(&base, "https://sonar.example", &bad, account).is_err());
        bad = options.clone(); bad["excludeCredentials"] = json!([{"type":"public-key", "id":"invalid="}]);
        assert!(validate_registration(&base, "https://sonar.example", &bad, account).is_err());
    }
}
