//! `POST /auth/login`, `POST /auth/logout`, `POST /auth/password`.

use openkwl_auth::{MAX_PASSWORD_LEN, PasswordError, TOKEN_LEN, validate_new_password};
use serde::{Deserialize, Serialize};

use crate::{FieldError, Validate, check};

/// Capacity of password fields. Longer bodies fail to parse (400) rather than being cut.
pub type Password = heapless::String<MAX_PASSWORD_LEN>;

/// `GET /setup`: whether first-time setup is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupStatus {
    /// No admin password exists yet.
    pub setup_required: bool,
    /// Seconds left in the setup window after power-on; `null` when setup is not required.
    /// When 0, power-cycle the unit to reopen it.
    pub window_remaining_s: Option<u32>,
}

/// Body of `POST /setup`: sets the first admin password. Accepted only while no password
/// exists and within the setup window; answers like `POST /auth/login` on success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupRequest {
    /// New admin password; same rules as a password change.
    pub password: Password,
}

impl Validate for SetupRequest {
    fn validate(&self) -> Result<(), FieldError> {
        validate_new_password(&self.password).map_err(|e| FieldError {
            field: "password",
            reason: match e {
                PasswordError::TooShort => "too short",
                PasswordError::TooLong => "too long",
            },
        })
    }
}

/// Body of `POST /auth/login`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginRequest {
    /// Admin password.
    pub password: Password,
    /// "Stay logged in on this device": a long-lived session (see [`LoginResponse`] for the
    /// timeouts). Sessions still end on logout, password change and restart of the unit.
    #[serde(default)]
    pub remember: bool,
}

impl Validate for LoginRequest {
    fn validate(&self) -> Result<(), FieldError> {
        check(!self.password.is_empty(), "password", "required")
    }
}

/// Successful login. Send the token as `Authorization: Bearer <token>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginResponse {
    /// 64 lowercase hex characters.
    pub token: heapless::String<{ TOKEN_LEN * 2 }>,
    /// Session ends after this much inactivity.
    pub idle_timeout_s: u32,
    /// Session ends this long after login regardless of activity.
    pub expires_in_s: u32,
}

/// Upper bound of the encoded [`LoginResponse`].
pub const LOGIN_RESPONSE_JSON_MAX: usize = 128;

/// Changes the admin password. On success all sessions, including the caller's, are revoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordChangeRequest {
    /// Must match the stored password.
    pub current_password: Password,
    /// Checked with [`openkwl_auth::validate_new_password`]; must differ from the current one.
    pub new_password: Password,
}

impl Validate for PasswordChangeRequest {
    fn validate(&self) -> Result<(), FieldError> {
        check(!self.current_password.is_empty(), "current_password", "required")?;
        validate_new_password(&self.new_password).map_err(|e| FieldError {
            field: "new_password",
            reason: match e {
                PasswordError::TooShort => "too short",
                PasswordError::TooLong => "too long",
            },
        })?;
        check(self.new_password != self.current_password, "new_password", "unchanged")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{parse, round_trip};

    #[test]
    fn login_round_trip_with_escapes() {
        let req: LoginRequest = parse(r#"{"password":"p\"w\\d ä"}"#).unwrap();
        assert_eq!(req.password, "p\"w\\d ä");
        assert!(req.validate().is_ok());
        round_trip::<_, 160>(&req);
        assert!(!req.remember, "defaults to a normal session");
        assert!(parse::<LoginRequest>(r#"{"password":"x","remember":true}"#).unwrap().remember);
        assert!(parse::<LoginRequest>(r#"{"password":""}"#).unwrap().validate().is_err());
    }

    #[test]
    fn overlong_password_is_rejected_by_parser() {
        let mut json = heapless::String::<128>::new();
        json.push_str(r#"{"password":""#).unwrap();
        for _ in 0..=MAX_PASSWORD_LEN {
            json.push('x').unwrap();
        }
        json.push_str(r#""}"#).unwrap();
        assert!(parse::<LoginRequest>(&json).is_err());
    }

    #[test]
    fn login_response_round_trip() {
        let mut token = heapless::String::new();
        for _ in 0..TOKEN_LEN {
            token.push_str("ab").unwrap();
        }
        let r = LoginResponse { token, idle_timeout_s: 1800, expires_in_s: 43_200 };
        round_trip::<_, LOGIN_RESPONSE_JSON_MAX>(&r);
    }

    #[test]
    fn setup() {
        let s = SetupStatus { setup_required: true, window_remaining_s: Some(1800) };
        round_trip::<_, 64>(&s);
        let r: SetupRequest = parse(r#"{"password":"correct horse battery"}"#).unwrap();
        assert!(r.validate().is_ok());
        assert_eq!(parse::<SetupRequest>(r#"{"password":"short"}"#).unwrap().validate().unwrap_err().reason, "too short");
    }

    #[test]
    fn password_change_rules() {
        let req = |cur: &str, new: &str| PasswordChangeRequest {
            current_password: cur.try_into().unwrap(),
            new_password: new.try_into().unwrap(),
        };
        assert!(req("old password", "a new long password").validate().is_ok());
        assert_eq!(req("old password", "short").validate().unwrap_err().reason, "too short");
        assert_eq!(req("same password", "same password").validate().unwrap_err().reason, "unchanged");
        assert_eq!(req("", "a new long password").validate().unwrap_err().field, "current_password");
        round_trip::<_, 192>(&req("old password", "a new long password"));
    }
}
