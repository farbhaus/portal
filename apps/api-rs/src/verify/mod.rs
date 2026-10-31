
//! Email verification (2FA-by-code) — port of `portal.verify.*`.

use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

use crate::config::Settings;
use crate::crypto::TokenCipher;
use crate::services::runtime_config::get_email_config;

pub const TRUST_COOKIE: &str = "portal_verified";

#[derive(Debug)]
pub struct VerificationError {
    pub status: u16,
    pub detail: String,
}
impl std::fmt::Display for VerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}
impl std::error::Error for VerificationError {}

fn norm(email: &str) -> String {
    email.trim().to_lowercase()
}
fn hash_code(code: &str) -> String {
    hex::encode(Sha256::digest(code.trim().as_bytes()))
}

// One plain address. Besides "@" and whitespace, refuse the characters that make an address list,
// a display name or a quoted part ("a,b@x.com", "Eve <e@x.com>"): mail would go to every recipient
// in it while the code proves control of only one. The domain is matched label by label (labels
// can't contain dots). Rust's regex engine is linear-time, so crafted input can't make it backtrack.
const NOT_SPECIAL: &str = r#"[^@\s,;:<>()\[\]"\\"#;
const MAX_EMAIL_LENGTH: usize = 254; // RFC 5321's limit for a deliverable address

fn email_re() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(&format!(
            "^{NOT_SPECIAL}]+@{NOT_SPECIAL}.]+(?:\\.{NOT_SPECIAL}.]+)+$"
        ))
        .expect("email regex")
    })
}

/// Port of `portal.verify.email_check.valid_syntax`.
pub fn valid_syntax(email: &str) -> bool {
    let email = email.trim();
    email.chars().count() <= MAX_EMAIL_LENGTH && email_re().is_match(email)
}

/// What one DNS lookup said about a domain, as `_domain_receives_mail` reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DnsAnswer {
    /// At least one record of the asked type.
    Records,
    /// The domain exists but has no record of this type (dnspython's NoAnswer).
    NoRecords,
    /// The domain doesn't exist at all (NXDOMAIN).
    NoSuchDomain,
    /// Timeout, SERVFAIL, no resolver, ... — unknown.
    Failed,
}

/// MX is the definitive signal; fall back to A/AAAA (a domain can receive mail on its A record).
/// A domain that doesn't exist fails; a transient/resolver error fails open — the code send is
/// the real gate. Port of `portal.verify.email_check._domain_receives_mail`.
fn receives_mail(answers: impl IntoIterator<Item = DnsAnswer>) -> bool {
    for answer in answers {
        match answer {
            DnsAnswer::Records | DnsAnswer::Failed => return true,
            DnsAnswer::NoSuchDomain => return false,
            DnsAnswer::NoRecords => continue,
        }
    }
    false
}

const DNS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn resolver() -> Option<&'static hickory_resolver::TokioResolver> {
    static RESOLVER: std::sync::OnceLock<Option<hickory_resolver::TokioResolver>> = std::sync::OnceLock::new();
    RESOLVER
        .get_or_init(|| {
            let mut builder = hickory_resolver::TokioResolver::builder_tokio()
                .map_err(|e| tracing::warn!("verify.dns.no_resolver error={e}"))
                .ok()?;
            let opts = builder.options_mut();
            opts.timeout = DNS_TIMEOUT;
            opts.attempts = 1;
            // dnspython asks the nameservers only; /etc/hosts is not a mail signal.
            opts.use_hosts_file = hickory_resolver::config::ResolveHosts::Never;
            builder.build().map_err(|e| tracing::warn!("verify.dns.no_resolver error={e}")).ok()
        })
        .as_ref()
}

async fn domain_receives_mail(domain: &str) -> bool {
    let Some(resolver) = resolver() else { return true }; // no resolver: fail open
    // Absolute name, so the resolver's search list is never appended (dnspython's default).
    let name = format!("{}.", domain.trim_end_matches('.'));
    let classify = |r: Result<hickory_resolver::lookup::Lookup, hickory_resolver::net::NetError>| match r {
        Ok(lookup) if !lookup.answers().is_empty() => DnsAnswer::Records,
        Ok(_) => DnsAnswer::NoRecords,
        Err(e) if e.is_nx_domain() => DnsAnswer::NoSuchDomain,
        Err(e) if e.is_no_records_found() => DnsAnswer::NoRecords,
        Err(_) => DnsAnswer::Failed,
    };
    for record in ["MX", "A", "AAAA"] {
        let answer = match record {
            "MX" => classify(resolver.mx_lookup(name.as_str()).await),
            "A" => classify(resolver.ipv4_lookup(name.as_str()).await),
            _ => classify(resolver.ipv6_lookup(name.as_str()).await),
        };
        if answer != DnsAnswer::NoRecords {
            return receives_mail([answer]);
        }
    }
    false
}

/// Syntax + (optionally) an MX/A/AAAA check that the domain can receive mail.
pub async fn is_plausible_email(email: &str, check_mx: bool) -> bool {
    let email = email.trim();
    if !valid_syntax(email) {
        return false;
    }
    if !check_mx {
        return true;
    }
    let domain = email.rsplit('@').next().unwrap_or("");
    domain_receives_mail(domain).await
}

/// Issue a fresh code for `email` (cooldown enforced). Err(detail) on failure.
/// `brand_profile`: the brand of the link asking, so the code email looks like its page.
pub async fn request_code(db: &PgPool, settings: &Settings, email: &str, brand_profile: Option<uuid::Uuid>) -> Result<(), VerificationError> {
    let email_config = get_email_config(db, settings).await;
    if !email_config.configured() {
        return Err(VerificationError { status: 503, detail: "Email verification is temporarily unavailable".into() });
    }
    let email = norm(email);
    if !is_plausible_email(&email, settings.verify_check_mx).await {
        return Err(VerificationError { status: 400, detail: "That email address looks invalid".into() });
    }
    let existing = sqlx::query("SELECT last_sent_at FROM email_verifications WHERE email = $1")
        .bind(&email)
        .fetch_optional(db)
        .await
        .ok()
        .flatten();
    let now = Utc::now();
    if let Some(row) = &existing {
        let last: chrono::DateTime<Utc> = row.get("last_sent_at");
        if (now - last).num_seconds() < settings.verify_resend_cooldown_seconds {
            return Err(VerificationError { status: 429, detail: "Please wait a moment before requesting another code".into() });
        }
    }
    let code = format!("{:06}", random_below(1_000_000));
    let expires = now + Duration::seconds(settings.verify_code_ttl_seconds);
    let brand = crate::services::runtime_config::email_brand_for(db, settings, brand_profile).await;
    // Send first; only persist if the email actually went out.
    crate::notify::send_verification_code(&email_config, &email, &code, settings.verify_code_ttl_seconds / 60, &brand)
        .await
        .map_err(|_| VerificationError { status: 503, detail: "Email verification is temporarily unavailable".into() })?;

    if existing.is_none() {
        // attempts has no DB default (SQLAlchemy sets it).
        let _ = sqlx::query(
            "INSERT INTO email_verifications (email, code_hash, expires_at, attempts, last_sent_at)
             VALUES ($1,$2,$3,0,$4)",
        )
        .bind(&email).bind(hash_code(&code)).bind(expires).bind(now)
        .execute(db).await;
    } else {
        let _ = sqlx::query(
            "UPDATE email_verifications SET code_hash=$1, expires_at=$2, attempts=0, last_sent_at=$3 WHERE email=$4",
        )
        .bind(hash_code(&code)).bind(expires).bind(now).bind(&email).execute(db).await;
    }
    Ok(())
}

/// True iff `code` matches the active, unexpired code for `email` (consumed on success).
pub async fn verify_code(db: &PgPool, settings: &Settings, email: &str, code: &str) -> bool {
    let email = norm(email);
    // Spend one attempt atomically, and only on a live code under the attempt cap, so parallel
    // guesses can't slip past the limit between the check and the increment.
    let stored: Option<String> = sqlx::query_scalar(
        "UPDATE email_verifications SET attempts = attempts + 1
          WHERE email = $1 AND expires_at >= now() AND attempts < $2
      RETURNING code_hash",
    )
    .bind(&email)
    .bind(settings.verify_max_attempts)
    .fetch_optional(db)
    .await
    .ok()
    .flatten();
    let Some(stored) = stored else { return false };
    if constant_time_eq(stored.as_bytes(), hash_code(code).as_bytes()) {
        let _ = sqlx::query("DELETE FROM email_verifications WHERE email = $1").bind(&email).execute(db).await;
        tracing::info!("verify.code_ok email={email}");
        return true;
    }
    false
}

/// A uniform integer in `0..bound` from the OS CSPRNG (Python: `secrets.randbelow`). Rejection
/// sampling keeps it unbiased.
fn random_below(bound: u32) -> u32 {
    let zone = u32::MAX - (u32::MAX % bound);
    loop {
        let mut buf = [0u8; 4];
        getrandom::fill(&mut buf).expect("os random");
        let v = u32::from_le_bytes(buf);
        if v < zone {
            return v % bound;
        }
    }
}

pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn cipher(settings: &Settings) -> TokenCipher {
    TokenCipher::new(&settings.token_encryption_key)
}

/// Is this device already trusted for `email` (encrypted cookie)?
pub fn is_trusted(settings: &Settings, cookies: &str, email: &str) -> bool {
    let raw = cookies.split(';').filter_map(|c| c.trim().split_once('=')).find(|(k, _)| *k == TRUST_COOKIE).map(|(_, v)| v.to_string());
    let Some(raw) = raw else { return false };
    let Ok(data) = cipher(settings).decrypt(&raw) else { return false };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else { return false };
    if v.get("email").and_then(|e| e.as_str()).map(|e| e.to_lowercase()) != Some(norm(email)) {
        return false;
    }
    v.get("exp").and_then(|e| e.as_f64()).map(|e| e > Utc::now().timestamp() as f64).unwrap_or(false)
}

/// Build the `Set-Cookie` value for the device-trust cookie.
pub fn trust_cookie(settings: &Settings, email: &str) -> String {
    let days = settings.verify_trust_days;
    let exp = (Utc::now() + Duration::days(days)).timestamp();
    let payload = serde_json::json!({ "email": norm(email), "exp": exp }).to_string();
    let token = cipher(settings).encrypt(&payload);
    let mut flags = "httponly; samesite=lax".to_string();
    if settings.session_cookie_secure {
        flags.push_str("; secure");
    }
    format!("{TRUST_COOKIE}={token}; Max-Age={}; path=/; {flags}", days * 86400)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Same cases as apps/api/tests/test_verify.py::test_valid_syntax.
    #[test]
    fn syntax_accepts_one_plain_address_only() {
        assert!(valid_syntax("a@b.co"));
        assert!(valid_syntax("ze.maria+x@sub.domain.com"));
        assert!(valid_syntax("  padded@example.com  "));
        assert!(!valid_syntax("ze@"));
        assert!(!valid_syntax("ze@ze")); // no dot in domain
        assert!(!valid_syntax("no-at-sign"));
        assert!(!valid_syntax("postmaster,me@evil.com"));
        assert!(!valid_syntax("a@x.com;b@y.com"));
        assert!(!valid_syntax("Eve <eve@evil.com>"));
        assert!(!valid_syntax("\"q\"@x.com"));
        assert!(!valid_syntax("a@b..com")); // empty domain label
        assert!(!valid_syntax("a b@x.com"));
        assert!(!valid_syntax(&format!("{}@b.co", "a".repeat(250)))); // over the 254-char limit
    }

    #[test]
    fn syntax_check_is_linear_on_crafted_input() {
        let evil = format!("!@!.{}@", "!.".repeat(6000));
        let started = std::time::Instant::now();
        assert!(!email_re().is_match(&evil));
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
    }

    #[test]
    fn codes_are_six_digits_in_range() {
        for _ in 0..1000 {
            assert!(random_below(1_000_000) < 1_000_000);
        }
        assert_eq!(format!("{:06}", 42), "000042");
    }

    #[test]
    fn mail_check_reads_dns_answers_like_python() {
        use DnsAnswer::*;
        assert!(receives_mail([Records])); // MX present
        assert!(receives_mail([NoRecords, Records])); // no MX, but an A record
        assert!(receives_mail([NoRecords, NoRecords, Records])); // AAAA only
        assert!(!receives_mail([NoRecords, NoRecords, NoRecords])); // nothing to deliver to
        assert!(!receives_mail([NoSuchDomain])); // NXDOMAIN
        assert!(receives_mail([Failed])); // timeout / SERVFAIL: fail open
        assert!(receives_mail([NoRecords, Failed]));
    }

    // Live DNS; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_dns_lookups() {
        assert!(domain_receives_mail("gmail.com").await);
        assert!(!domain_receives_mail("portal-mx-check-does-not-exist.invalid").await);
        assert!(is_plausible_email("someone@gmail.com", true).await);
        assert!(!is_plausible_email("someone@portal-mx-check-does-not-exist.invalid", true).await);
    }

    #[test]
    fn constant_time_eq_compares_whole_values() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
