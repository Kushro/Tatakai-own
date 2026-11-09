//! Sign-up, email verification code and password recovery against GoTrue (wave A, worker **f**;
//! docs/redesign/15-parity-wave-a-contract.md section 3). Same `Supabase` client (child module: it can use the
//! private `endpoint` / `token_request` / `parse_session` helpers of `mod.rs`). Contract stubs below.
//!
//! GoTrue endpoints to use: `POST auth/v1/signup` (`{email,password}` -> a session when confirmation is off, or
//! a user without session when it is on), `POST auth/v1/verify` (`{type:"signup"|"email", email, token}` ->
//! session), `POST auth/v1/resend` (`{type:"signup", email}`), `POST auth/v1/recover` (`{email}`),
//! `POST auth/v1/verify` with `type:"recovery"` then `PUT auth/v1/user` (`{password}`) with the returned token.
//! Every request carries `apikey: <anon key>`.

use super::{Session, Supabase};
use crate::error::SupabaseError;

/// What `sign_up` returns: a session (confirmation off) or "check your email" (confirmation on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignUpOutcome {
    SignedIn(Session),
    NeedsVerification,
}

impl Supabase {
    /// Contract stub (worker f).
    pub async fn sign_up(&self, email: &str, password: &str) -> Result<SignUpOutcome, SupabaseError> {
        let _ = (email, password);
        Err(SupabaseError::Http(501))
    }

    /// Verify the 6-digit sign-up / email code and store the session. Contract stub (worker f).
    pub async fn verify_email_code(&self, email: &str, code: &str) -> Result<Session, SupabaseError> {
        let _ = (email, code);
        Err(SupabaseError::Http(501))
    }

    /// Contract stub (worker f).
    pub async fn resend_signup_code(&self, email: &str) -> Result<(), SupabaseError> {
        let _ = email;
        Err(SupabaseError::Http(501))
    }

    /// Contract stub (worker f).
    pub async fn request_password_recovery(&self, email: &str) -> Result<(), SupabaseError> {
        let _ = email;
        Err(SupabaseError::Http(501))
    }

    /// Verify the recovery code and set `new_password`; leaves the user signed in. Contract stub (worker f).
    pub async fn reset_password_with_code(&self, email: &str, code: &str, new_password: &str) -> Result<Session, SupabaseError> {
        let _ = (email, code, new_password);
        Err(SupabaseError::Http(501))
    }
}
